//! Rechenkern llama.cpp, statisch eingebunden (über llama-cpp-2). Hält Modell + Kontext,
//! verwendet den Anfang des letzten Prompts wieder (Systemprompt des Agenten wird nur einmal gerechnet).

use crate::config::Config;
use crate::engine::{chatml, GenParams, ModelInfo, Msg, Session, Stats, StopFilter};
use llama_cpp_2::context::params::{KvCacheType, LlamaContextParams};
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
#[cfg(feature = "vision")]
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::sync::OnceLock;
use std::time::Instant;

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn backend() -> Result<&'static LlamaBackend, String> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let mut b = LlamaBackend::init().map_err(|e| e.to_string())?;
    if std::env::var_os("ULTAR_LLM_DEBUG").is_none() {
        b.void_logs();
    }
    let _ = BACKEND.set(b);
    Ok(BACKEND.get().unwrap())
}

pub struct LlamaSession {
    /// Bild-Projektor (mmproj) – nur mit Feature `vision` und passender Datei neben dem Modell
    #[cfg(feature = "vision")]
    mm: Option<MtmdContext>,
    // Reihenfolge wichtig: Kontext vor Modell freigeben (siehe Drop)
    ctx: Option<LlamaContext<'static>>,
    model: *mut LlamaModel,
    /// Token, die gerade im KV-Cache stehen (Sequenz 0)
    cache: Vec<LlamaToken>,
    info: ModelInfo,
    batch: usize,
    is_qwen3: bool,
}

impl Drop for LlamaSession {
    fn drop(&mut self) {
        #[cfg(feature = "vision")]
        self.mm.take();
        self.ctx.take();
        // SAFETY: model stammt aus Box::into_raw in open(); kein Kontext verweist mehr darauf.
        unsafe { drop(Box::from_raw(self.model)) };
    }
}

pub fn open(c: &Config, id: &str, path: &str) -> Result<Box<dyn Session>, String> {
    let t0 = Instant::now();
    let be = backend()?;
    let mp = LlamaModelParams::default()
        .with_n_gpu_layers(c.gpu_layers)
        .with_use_mmap(true);
    let model =
        LlamaModel::load_from_file(be, path, &mp).map_err(|e| format!("Modell laden: {e}"))?;
    let n_ctx = c.ctx.min(model.n_ctx_train().max(512));
    let mut cp = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_batch(c.batch)
        .with_n_ubatch(c.batch)
        .with_n_threads(c.threads)
        .with_n_threads_batch(c.threads_batch);
    if c.kv_q8 {
        cp = cp
            .with_type_k(KvCacheType::Q8_0)
            .with_type_v(KvCacheType::Q8_0);
    }
    let arch = model
        .meta_val_str("general.architecture")
        .unwrap_or_default();
    let info = ModelInfo {
        id: id.to_string(),
        path: path.to_string(),
        engine: "llama.cpp".into(),
        n_ctx,
        n_params: model.n_params(),
        bytes: model.size(),
        threads: c.threads,
        load_ms: 0.0,
    };
    let model = Box::into_raw(Box::new(model));
    // SAFETY: das Modell bleibt bis Drop an fester Adresse; der Kontext wird vorher freigegeben.
    let mref: &'static LlamaModel = unsafe { &*model };
    let ctx = match mref.new_context(be, cp) {
        Ok(x) => x,
        Err(e) => {
            unsafe { drop(Box::from_raw(model)) };
            return Err(format!("Kontext anlegen: {e}"));
        }
    };
    #[cfg(feature = "vision")]
    let mm = match crate::models::mmproj_for(path) {
        Some(pp) => {
            let mp = MtmdContextParams {
                use_gpu: c.gpu_layers > 0,
                print_timings: false,
                n_threads: c.threads,
                media_marker: std::ffi::CString::new(crate::engine::MEDIA_MARKER).unwrap(),
                image_min_tokens: -1,
                image_max_tokens: -1,
            };
            MtmdContext::init_from_file(&pp, mref, &mp)
                .ok()
                .filter(|m| m.support_vision())
        }
        None => None,
    };
    let mut s = LlamaSession {
        #[cfg(feature = "vision")]
        mm,
        ctx: Some(ctx),
        model,
        cache: Vec::new(),
        info,
        batch: c.batch as usize,
        is_qwen3: arch.starts_with("qwen3"),
    };
    s.info.load_ms = t0.elapsed().as_secs_f64() * 1000.0;
    Ok(Box::new(s))
}

impl LlamaSession {
    fn model(&self) -> &LlamaModel {
        unsafe { &*self.model }
    }
}

impl Session for LlamaSession {
    fn info(&self) -> ModelInfo {
        self.info.clone()
    }

    fn chat_prompt(&self, msgs: &[Msg], json_only: bool) -> Result<String, String> {
        let m = self.model();
        let mut p = match m.chat_template(None) {
            Ok(t) => {
                let chat: Result<Vec<LlamaChatMessage>, _> = msgs
                    .iter()
                    .map(|x| LlamaChatMessage::new(x.role.clone(), x.content_mm()))
                    .collect();
                let chat = chat.map_err(|e| e.to_string())?;
                m.apply_chat_template(&t, &chat, true)
                    .unwrap_or_else(|_| chatml(msgs))
            }
            Err(_) => chatml(msgs),
        };
        // Qwen3 ohne Denkphase: leeren <think>-Block vorgeben (wie enable_thinking=false), sonst
        // würde die JSON-Grammatik das Denken abwürgen bzw. /no_think wäre nur ein Hinweis.
        let no_think = json_only || msgs.iter().any(|x| x.content.contains("/no_think"));
        if self.is_qwen3 && no_think && p.ends_with("<|im_start|>assistant\n") {
            p.push_str("<think>\n\n</think>\n\n");
        }
        Ok(p)
    }

    fn generate(
        &mut self,
        prompt: &str,
        p: &GenParams,
        sink: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Stats, String> {
        let t0 = Instant::now();
        let n_ctx = self.info.n_ctx as usize;
        let batch_cap = self.batch;
        let mref: &'static LlamaModel = unsafe { &*self.model };
        let vocab = mref.vocab();
        let tokens = vocab.tokenize(prompt.as_bytes(), vocab.should_add_bos(), true);
        if tokens.is_empty() {
            return Err("leerer Prompt".into());
        }
        if tokens.len() + 8 > n_ctx {
            return Err(format!(
                "Prompt zu lang: {} Token, Kontext {}",
                tokens.len(),
                n_ctx
            ));
        }
        // gemeinsamen Anfang mit dem KV-Cache wiederverwenden; das letzte Token immer neu rechnen (für die Logits)
        let mut keep = self
            .cache
            .iter()
            .zip(tokens.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if keep >= tokens.len() {
            keep = tokens.len() - 1;
        }
        let ctx = self.ctx.as_mut().ok_or("kein Kontext")?;
        if ctx.kv_cache_seq_rm(0, Some(keep as u32), None).is_err() {
            ctx.clear_kv_cache();
            keep = 0;
        }
        self.cache.truncate(keep);
        let mut st = Stats {
            prompt_tokens: tokens.len() as u32,
            cached_tokens: keep as u32,
            ..Default::default()
        };

        let mut batch = LlamaBatch::new(batch_cap.max(1), 1);
        let rest = &tokens[keep..];
        let mut pos = keep;
        for chunk in rest.chunks(batch_cap) {
            batch.clear();
            for (i, t) in chunk.iter().enumerate() {
                let last = pos + i == tokens.len() - 1;
                batch
                    .add(*t, (pos + i) as i32, &[0], last)
                    .map_err(|e| e.to_string())?;
            }
            ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
            pos += chunk.len();
        }
        self.cache.extend_from_slice(rest);
        st.prompt_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let idx = batch.n_tokens() - 1;
        sample_loop(
            ctx,
            mref,
            &mut self.cache,
            batch_cap,
            n_ctx,
            p,
            pos,
            idx,
            st,
            sink,
        )
    }

    fn vision(&self) -> bool {
        #[cfg(feature = "vision")]
        {
            return self.mm.is_some();
        }
        #[allow(unreachable_code)]
        false
    }

    #[cfg(feature = "vision")]
    fn generate_mm(
        &mut self,
        prompt: &str,
        images: &[String],
        p: &GenParams,
        sink: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Stats, String> {
        let t0 = Instant::now();
        let n_ctx = self.info.n_ctx as usize;
        let batch_cap = self.batch;
        let mref: &'static LlamaModel = unsafe { &*self.model };
        let mm = self
            .mm
            .as_mut()
            .ok_or("this model cannot see images (no mmproj next to the model)")?;
        let mut bitmaps = Vec::new();
        for f in images {
            bitmaps.push(
                MtmdBitmap::from_file(mm, f, false).map_err(|e| format!("image {f}: {e:?}"))?,
            );
        }
        let refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();
        let chunks = mm
            .tokenize(
                MtmdInputText {
                    text: prompt.to_string(),
                    add_special: true,
                    parse_special: true,
                },
                &refs,
            )
            .map_err(|e| format!("image tokens: {e:?}"))?;
        if chunks.total_tokens() + 8 > n_ctx {
            return Err(format!(
                "Prompt zu lang: {} Token (mit Bild), Kontext {}",
                chunks.total_tokens(),
                n_ctx
            ));
        }
        let ctx = self.ctx.as_mut().ok_or("kein Kontext")?;
        // Bilder stehen nicht als Token im Zwischenspeicher → neu beginnen
        ctx.clear_kv_cache();
        self.cache.clear();
        let n_past = chunks
            .eval_chunks(mm, ctx, 0, 0, batch_cap as i32, true)
            .map_err(|e| format!("image eval: {e:?}"))?;
        let st = Stats {
            prompt_tokens: chunks.total_tokens() as u32,
            prompt_ms: t0.elapsed().as_secs_f64() * 1000.0,
            ..Default::default()
        };
        let mut scratch = Vec::new();
        let r = sample_loop(
            ctx,
            mref,
            &mut scratch,
            batch_cap,
            n_ctx,
            p,
            n_past as usize,
            -1,
            st,
            sink,
        );
        // Nach einem Bild ist der Cache nicht mehr wiederverwendbar
        ctx.clear_kv_cache();
        r
    }
}

/// Erzeugen ab Position `pos`; `idx` = Logit-Index der letzten Eingabe (-1 = letzte Ausgabe).
#[allow(clippy::too_many_arguments)]
fn sample_loop(
    ctx: &mut LlamaContext<'static>,
    mref: &'static LlamaModel,
    cache: &mut Vec<LlamaToken>,
    batch_cap: usize,
    n_ctx: usize,
    p: &GenParams,
    mut pos: usize,
    first_idx: i32,
    mut st: Stats,
    sink: &mut dyn FnMut(&str) -> bool,
) -> Result<Stats, String> {
    let vocab = mref.vocab();
    let mut batch = LlamaBatch::new(batch_cap.max(1), 1);
    // Sampler-Kette
    let mut chain = Vec::new();
    if let Some(g) = &p.grammar {
        chain.push(LlamaSampler::grammar(mref, g, "root").map_err(|e| format!("Grammatik: {e}"))?);
    }
    if p.repeat_penalty > 1.0 {
        chain.push(LlamaSampler::penalties(
            mref.n_vocab(),
            64,
            p.repeat_penalty,
            0.0,
            0.0,
        ));
    }
    if p.temperature <= 0.0 {
        chain.push(LlamaSampler::greedy());
    } else {
        chain.push(LlamaSampler::top_k(p.top_k.max(1)));
        chain.push(LlamaSampler::top_p(p.top_p.clamp(0.01, 1.0), 1));
        if p.min_p > 0.0 {
            chain.push(LlamaSampler::min_p(p.min_p, 1));
        }
        chain.push(LlamaSampler::temp(p.temperature));
        chain.push(LlamaSampler::dist(p.seed));
    }
    let mut sampler = LlamaSampler::chain_simple(chain);

    let t1 = Instant::now();
    let mut filter = StopFilter::new(&p.stop);
    let budget = (p.max_tokens as usize).min(n_ctx.saturating_sub(pos + 1));
    st.finish = "length".into();
    let mut idx = first_idx;
    let mut cancelled = false;
    for _ in 0..budget {
        // sample() nimmt das Token bereits an (Grammatik/Strafen) – kein zweites accept()
        let tok = sampler.sample(ctx, idx);
        if vocab.is_eog(tok) {
            st.finish = "stop".into();
            break;
        }
        st.completion_tokens += 1;
        let piece = vocab.token_to_piece(tok, false, None);
        let (txt, stop) = filter.push(&piece);
        if !txt.is_empty() && !sink(&txt) {
            cancelled = true;
        }
        batch.clear();
        batch
            .add(tok, pos as i32, &[0], true)
            .map_err(|e| e.to_string())?;
        cache.push(tok);
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
        pos += 1;
        idx = 0;
        if stop {
            st.finish = "stop".into();
            break;
        }
        if cancelled {
            st.finish = "cancelled".into();
            break;
        }
    }
    if !cancelled {
        let r = filter.finish();
        if !r.is_empty() {
            sink(&r);
        }
    }
    st.gen_ms = t1.elapsed().as_secs_f64() * 1000.0;
    Ok(st)
}
