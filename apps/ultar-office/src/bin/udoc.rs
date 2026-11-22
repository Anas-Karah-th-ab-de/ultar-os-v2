//! udoc – Büro-Dateien von der Befehlszeile (für den KI-Agenten, Skripte und den Ultar-Kern).
//! Jede Ausgabe ist JSON, wenn `--json` angegeben ist; sonst Text.

use ultar_office as uo;

const HILFE: &str = "udoc – PDF, Word (.docx), Excel (.xlsx/.csv) und Textdateien ohne Fremdprogramm

  udoc info   <datei>                       Art, Seiten/Blätter/Zeilen, Kodierung
  udoc text   <datei> [--max N]             Inhalt als Text (PDF, DOCX als Markdown, Tabellen)
  udoc pdf-text   <pdf> <seite>             Text einer Seite (ab 1)
  udoc pdf-search <pdf> <suchwort>          Fundstellen mit Seite
  udoc pdf-render <pdf> <seite> <breite> <aus.png>
  udoc create <datei> <inhalt|->            neue Datei (.docx aus Markdown, .xlsx/.csv aus CSV, sonst Text)
  udoc replace <datei> <suche> <ersatz> [--regex]
  udoc sheet-get <xlsx|csv> [Blatt!A1:C5]   Werte (Formeln in [ ])
  udoc sheet-set <xlsx|csv> <Blatt!B3> <wert|=formel>
  udoc eval <formel>                        Formel ohne Datei berechnen
  udoc convert <ein> <aus>                  docx↔md/txt, xlsx↔csv
  udoc lines <datei> <von> <bis>            Zeilen einer Textdatei (ab 1)
  udoc insert <datei> <zeile> <text>        Zeile einfügen (ab 1, 0 = Ende)

Option --json: Ergebnis als JSON {\"ok\":…,\"result\":…}";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    args.retain(|a| a != "--json");
    let regex = args.iter().any(|a| a == "--regex");
    args.retain(|a| a != "--regex");
    let mut max = 20_000usize;
    if let Some(i) = args.iter().position(|a| a == "--max") {
        if let Some(v) = args.get(i + 1).and_then(|v| v.parse().ok()) {
            max = v;
        }
        args.drain(i..(i + 2).min(args.len()));
    }
    let a = |i: usize| args.get(i).cloned().unwrap_or_default();
    let res: Result<serde_json::Value, String> = (|| {
        Ok(match a(0).as_str() {
            "info" => uo::info(&a(1))?,
            "text" => uo::read_text(&a(1), max)?.into(),
            "pdf-text" => {
                let d = uo::pdf::PdfDoc::open(std::fs::read(a(1)).map_err(|e| e.to_string())?)?;
                let p: usize = a(2).parse().unwrap_or(1);
                if p == 0 || p > d.pages() {
                    return Err(format!("page 1..{}", d.pages()));
                }
                d.page_text(p - 1).into()
            }
            "pdf-search" => {
                let d = uo::pdf::PdfDoc::open(std::fs::read(a(1)).map_err(|e| e.to_string())?)?;
                d.search(&a(2), 50)
                    .into_iter()
                    .map(|(p, s)| serde_json::json!({"page": p + 1, "text": s}))
                    .collect::<Vec<_>>()
                    .into()
            }
            "pdf-render" => {
                let d = uo::pdf::PdfDoc::open(std::fs::read(a(1)).map_err(|e| e.to_string())?)?;
                let p: usize = a(2).parse().unwrap_or(1);
                let w: u32 = a(3).parse().unwrap_or(1080);
                let img = d.render(p.saturating_sub(1), w).ok_or("no such page")?;
                let f = std::fs::File::create(a(4)).map_err(|e| e.to_string())?;
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.w, img.h);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header()
                    .and_then(|mut wr| wr.write_image_data(&img.rgba))
                    .map_err(|e| e.to_string())?;
                serde_json::json!({"png": a(4), "width": img.w, "height": img.h})
            }
            "create" => {
                let content = if a(2) == "-" {
                    let mut s = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
                        .map_err(|e| e.to_string())?;
                    s
                } else {
                    a(2).replace("\\n", "\n")
                };
                uo::create(&a(1), &content)?;
                "ok".into()
            }
            "replace" => uo::replace_in_file(&a(1), &a(2), &a(3), regex)?.into(),
            "sheet-get" => uo::sheet_get(&a(1), &a(2))?.into(),
            "sheet-set" => uo::sheet_set(&a(1), &a(2), &a(3))?.into(),
            "eval" => {
                let wb = uo::sheet::Workbook::new();
                let v = wb.eval_formula(0, &a(1));
                if let uo::sheet::Value::Err(e) = &v {
                    return Err(e.clone());
                }
                v.text().into()
            }
            "convert" => {
                let (src, dst) = (a(1), a(2));
                match (uo::Kind::of(&src), uo::Kind::of(&dst)) {
                    (uo::Kind::Sheet, uo::Kind::Sheet) => {
                        uo::write_book(&dst, &uo::read_book(&src)?, 0)?
                    }
                    (uo::Kind::Doc | uo::Kind::Text, _) | (_, uo::Kind::Doc)
                        if uo::Kind::of(&src) != uo::Kind::Pdf =>
                    {
                        uo::write_doc(&dst, &uo::read_doc(&src)?)?
                    }
                    (uo::Kind::Pdf, uo::Kind::Text) => {
                        uo::write_atomic(&dst, uo::read_text(&src, usize::MAX)?.as_bytes())?
                    }
                    (uo::Kind::Pdf, uo::Kind::Doc) => uo::write_doc(
                        &dst,
                        &uo::doc::Document::from_text(&uo::read_text(&src, usize::MAX)?),
                    )?,
                    (uo::Kind::Sheet, uo::Kind::Text) => {
                        uo::write_atomic(&dst, uo::read_text(&src, usize::MAX)?.as_bytes())?
                    }
                    _ => return Err("conversion not supported".into()),
                }
                "ok".into()
            }
            "lines" => {
                let b = std::fs::read(a(1)).map_err(|e| e.to_string())?;
                let buf = uo::code::Buffer::from_bytes(&b);
                let from: usize = a(2).parse().unwrap_or(1).max(1);
                let to: usize = a(3).parse().unwrap_or(from + 40);
                (from..=to.min(buf.line_count()))
                    .map(|l| format!("{l}: {}", buf.line(l - 1)))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into()
            }
            "insert" => {
                let b = std::fs::read(a(1)).unwrap_or_default();
                let mut buf = uo::code::Buffer::from_bytes(&b);
                let l: usize = a(2).parse().unwrap_or(0);
                if l == 0 || l > buf.line_count() {
                    buf.cur = buf.text.len();
                    let nl = if buf.text.is_empty() || buf.text.ends_with('\n') {
                        ""
                    } else {
                        "\n"
                    };
                    buf.insert(&format!("{nl}{}", a(3)));
                } else {
                    buf.cur = buf.pos(l - 1, 0);
                    buf.insert(&format!("{}\n", a(3)));
                }
                uo::write_atomic(&a(1), &buf.to_bytes())?;
                "ok".into()
            }
            "" | "-h" | "--help" | "hilfe" | "help" => {
                println!("{HILFE}");
                std::process::exit(0);
            }
            x => return Err(format!("unknown command: {x} (udoc --help)")),
        })
    })();
    match res {
        Ok(v) => {
            if json {
                println!("{}", serde_json::json!({"ok": true, "result": v}));
            } else if let Some(s) = v.as_str() {
                print!("{s}");
                if !s.ends_with('\n') {
                    println!();
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            }
        }
        Err(e) => {
            if json {
                println!("{}", serde_json::json!({"ok": false, "error": e}));
            } else {
                eprintln!("udoc: {e}");
            }
            std::process::exit(1);
        }
    }
}
