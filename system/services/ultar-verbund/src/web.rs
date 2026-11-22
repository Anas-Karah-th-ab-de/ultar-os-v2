//! Fernansicht im Browser: `ultar-verbund anzeigen <gerät>` startet einen kleinen Webserver, der den Bildschirm
//! des Geräts (verschlüsselt über den Verbund geholt) zeigt und Maus, Berührung, langes Drücken, Wischen und
//! Tastatur zurückschickt. Jeder Browser genügt – auf Ultar-Geräten öffnet die Oberfläche die Ansicht selbst.
//! Zugriff nur mit dem Schlüssel in der Adresse (?t=…).

use crate::dienst;
use crate::entdeckung::Gesehen;
use crate::ident::Ident;
use crate::kanal::Kanal;
use crate::proto::Nachricht;
use crate::relay::Relais;
use base64::Engine;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Ansicht {
    ich: Ident,
    ziel: Gesehen,
    relais: Option<Arc<Relais>>,
    kanal: Mutex<Option<Kanal>>,
    token: String,
    /// letzte Anfrage (ms) – zum Beenden ungenutzter Ansichten
    pub zuletzt: std::sync::atomic::AtomicU64,
}

impl Ansicht {
    pub fn neu(
        ich: Ident,
        ziel: Gesehen,
        relais: Option<Arc<Relais>>,
        token: String,
    ) -> Arc<Ansicht> {
        Arc::new(Ansicht {
            ich,
            ziel,
            relais,
            kanal: Mutex::new(None),
            token,
            zuletzt: crate::jetzt_ms().into(),
        })
    }

    fn kanal<'a>(&self, k: &'a mut Option<Kanal>) -> Result<&'a mut Kanal, String> {
        if k.is_none() {
            let mut neu = dienst::verbinden(&self.ich, &self.ziel, self.relais.as_ref())
                .map_err(|e| e.to_string())?;
            neu.zeitlimit(Some(Duration::from_secs(30)));
            neu.nachricht_senden(&Nachricht::Teilen)
                .map_err(|e| e.to_string())?;
            match neu.nachricht_empfangen().map_err(|e| e.to_string())? {
                Nachricht::Ok { .. } => {}
                Nachricht::Fehler { text } => return Err(text),
                _ => return Err("unerwartet".into()),
            }
            *k = Some(neu);
        }
        Ok(k.as_mut().unwrap())
    }

    /// eine Anfrage über den Kanal; bei Fehler einmal neu verbinden
    pub fn frage(&self, n: &Nachricht) -> Result<Nachricht, String> {
        let mut g = self.kanal.lock().unwrap();
        for versuch in 0..2 {
            let r = (|| -> Result<Nachricht, String> {
                let k = self.kanal(&mut g)?;
                k.nachricht_senden(n).map_err(|e| e.to_string())?;
                k.nachricht_empfangen().map_err(|e| e.to_string())
            })();
            match r {
                Ok(x) => return Ok(x),
                Err(e) => {
                    *g = None;
                    if versuch == 1 {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    /// Bild holen: Antwort [w u32][h u32][nr u64][neu u8][Kachelpaket]
    pub fn bild(&self, eingaben: Vec<String>, voll: bool) -> Result<Vec<u8>, String> {
        let neu_verbunden = self.kanal.lock().unwrap().is_none();
        match self.frage(&Nachricht::Fern {
            eingaben,
            warte_ms: 300,
            voll: voll || neu_verbunden,
        })? {
            Nachricht::Bild { w, h, nr, daten } => {
                let mut out = Vec::new();
                out.extend_from_slice(&w.to_le_bytes());
                out.extend_from_slice(&h.to_le_bytes());
                out.extend_from_slice(&nr.to_le_bytes());
                out.push((voll || neu_verbunden) as u8);
                out.extend(
                    base64::engine::general_purpose::STANDARD
                        .decode(daten)
                        .map_err(|e| e.to_string())?,
                );
                Ok(out)
            }
            Nachricht::Fehler { text } => Err(text),
            _ => Err("unerwartet".into()),
        }
    }

    pub fn starten(self: &Arc<Self>, adresse: &str) -> std::io::Result<String> {
        let l = TcpListener::bind(adresse)?;
        let a = l.local_addr()?;
        let host = if a.ip().is_unspecified() {
            "127.0.0.1".to_string()
        } else {
            a.ip().to_string()
        };
        let url = format!("http://{host}:{}/?t={}", a.port(), self.token);
        let s = self.clone();
        std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                let s = s.clone();
                std::thread::spawn(move || {
                    let _ = s.http(c);
                });
            }
        });
        Ok(url)
    }

    fn http(&self, mut c: TcpStream) -> std::io::Result<()> {
        c.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut r = BufReader::new(c.try_clone()?);
        loop {
            let mut zeile = String::new();
            if r.read_line(&mut zeile)? == 0 {
                return Ok(());
            }
            let mut teile = zeile.split_whitespace();
            let (methode, ziel) = (
                teile.next().unwrap_or("").to_string(),
                teile.next().unwrap_or("/").to_string(),
            );
            let mut laenge = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h)? == 0 {
                    return Ok(());
                }
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        laenge = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            if laenge > crate::weitergeben::MAX_DATEI {
                return antwort(&mut c, 413, "text/plain", b"zu gross");
            }
            let mut body = vec![0u8; laenge];
            r.read_exact(&mut body)?;
            let (pfad, query) = ziel.split_once('?').unwrap_or((&ziel, ""));
            let q = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
                    .map(url_dekodieren)
            };
            self.zuletzt
                .store(crate::jetzt_ms(), std::sync::atomic::Ordering::Relaxed);
            if q("t").as_deref() != Some(self.token.as_str()) {
                antwort(&mut c, 403, "text/plain", b"Zugriff verweigert")?;
                continue;
            }
            match (methode.as_str(), pfad) {
                ("GET", "/") => {
                    let seite = SEITE
                        .replace("{{NAME}}", &html_esc(&self.ziel.name))
                        .replace("{{T}}", &self.token);
                    antwort(&mut c, 200, "text/html; charset=utf-8", seite.as_bytes())?
                }
                ("POST", "/bild") => {
                    let eingaben: Vec<String> = String::from_utf8_lossy(&body)
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(String::from)
                        .take(200)
                        .collect();
                    match self.bild(eingaben, q("voll").as_deref() == Some("1")) {
                        Ok(b) => antwort(&mut c, 200, "application/octet-stream", &b)?,
                        Err(e) => antwort(&mut c, 503, "text/plain", e.as_bytes())?,
                    }
                }
                ("POST", "/aktion") => {
                    let zeile = String::from_utf8_lossy(&body).trim().to_string();
                    match self.frage(&Nachricht::Aktion { zeile }) {
                        Ok(Nachricht::Ok { text }) => {
                            antwort(&mut c, 200, "text/plain", text.as_bytes())?
                        }
                        Ok(Nachricht::Fehler { text }) => {
                            antwort(&mut c, 400, "text/plain", text.as_bytes())?
                        }
                        _ => antwort(&mut c, 503, "text/plain", b"keine Antwort")?,
                    }
                }
                ("POST", "/datei") => {
                    let name = q("name").unwrap_or_else(|| "datei".into());
                    let n = Nachricht::Datei {
                        name,
                        daten: base64::engine::general_purpose::STANDARD.encode(&body),
                        oeffnen: true,
                    };
                    match self.frage(&n) {
                        Ok(Nachricht::Ok { text }) => {
                            antwort(&mut c, 200, "text/plain", text.as_bytes())?
                        }
                        Ok(Nachricht::Fehler { text }) => {
                            antwort(&mut c, 400, "text/plain", text.as_bytes())?
                        }
                        _ => antwort(&mut c, 503, "text/plain", b"keine Antwort")?,
                    }
                }
                _ => antwort(&mut c, 404, "text/plain", b"nicht gefunden")?,
            }
        }
    }
}

fn antwort(c: &mut TcpStream, code: u16, typ: &str, body: &[u8]) -> std::io::Result<()> {
    let text = match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        400 => "Bad Request",
        _ => "Service Unavailable",
    };
    write!(c, "HTTP/1.1 {code} {text}\r\nContent-Type: {typ}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: SAMEORIGIN\r\n\r\n", body.len())?;
    c.write_all(body)?;
    c.flush()
}

fn url_dekodieren(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            x => out.push(x),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn token() -> String {
    use rand::Rng;
    let mut b = [0u8; 16];
    rand::thread_rng().fill(&mut b);
    crate::ident::hex(&b)
}

const SEITE: &str = r##"<!DOCTYPE html>
<html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,user-scalable=no">
<title>{{NAME}} – Ultar-Verbund</title>
<style>
html,body{margin:0;height:100%;background:#14161b;color:#e6e8ee;font:14px system-ui,sans-serif;overflow:hidden}
#leiste{display:flex;gap:6px;align-items:center;padding:6px 8px;background:#1d2027;flex-wrap:wrap}
#leiste b{color:#e6c454;margin-right:6px}
button,label.k{background:#2a2d35;color:#e6e8ee;border:0;border-radius:8px;padding:7px 11px;font:inherit;cursor:pointer}
button:hover,label.k:hover{background:#353945}
#zust{margin-left:auto;color:#9aa0ab;font-size:12px}
#flaeche{position:absolute;top:46px;left:0;right:0;bottom:0;display:flex;align-items:center;justify-content:center}
canvas{max-width:100%;max-height:100%;touch-action:none;image-rendering:auto;box-shadow:0 0 0 1px #2a2d35}
input[type=range]{width:90px}
</style></head><body>
<div id="leiste"><b>{{NAME}}</b>
<button data-z="zurueck" title="Zurück">←</button><button data-a="oeffne home" title="Start">⌂</button>
<button data-z="leiser">🔉</button><button data-z="lauter">🔊</button>
<span>☀<input id="hell" type="range" min="5" max="100" value="80"></span>
<button id="dreh" title="Drehen">⟳</button><button data-a="bildschirm aus">Bildschirm aus</button><button data-a="bildschirm an">an</button>
<label class="k">Datei senden<input id="datei" type="file" hidden></label>
<button id="voll">Vollbild</button><span id="zust">verbinde …</span></div>
<div id="flaeche"><canvas id="c" width="320" height="200" tabindex="0"></canvas></div>
<script>
const T="{{T}}",c=document.getElementById("c"),g=c.getContext("2d"),zust=document.getElementById("zust");
let w=0,h=0,bild=null,warte=[],zug=null,voll=1,dreh=0,fehler=0,n=0,t0=performance.now();
function lz4(s,o,e,d){let p=0;while(o<e){const t=s[o++];let l=t>>4;if(l==15){let b;do{b=s[o++];l+=b}while(b==255)}for(let i=0;i<l;i++)d[p++]=s[o++];if(o>=e)break;const f=s[o]|(s[o+1]<<8);o+=2;let m=t&15;if(m==15){let b;do{b=s[o++];m+=b}while(b==255)}m+=4;let q=p-f;for(let i=0;i<m;i++)d[p++]=d[q++]}return p}
function einspielen(buf){const v=new DataView(buf),u=new Uint8Array(buf);const nw=v.getUint32(0,true),nh=v.getUint32(4,true);
 if(nw!=w||nh!=h){w=nw;h=nh;c.width=w;c.height=h;bild=g.createImageData(w,h)}
 let p=17;const tmp=new Uint8Array(64*64*3);
 while(p+12<=u.length){const x=v.getUint16(p,true),y=v.getUint16(p+2,true),kw=v.getUint16(p+4,true),kh=v.getUint16(p+6,true),len=v.getUint32(p+8,true);p+=12;
  const roh=v.getUint32(p,true);lz4(u,p+4,p+len,tmp);p+=len;
  for(let yy=0;yy<kh;yy++)for(let xx=0;xx<kw;xx++){const s=(yy*kw+xx)*3,d=((y+yy)*w+x+xx)*4;bild.data[d]=tmp[s];bild.data[d+1]=tmp[s+1];bild.data[d+2]=tmp[s+2];bild.data[d+3]=255}
  n++}
 if(bild)g.putImageData(bild,0,0)}
async function schleife(){for(;;){const z=warte.splice(0);if(zug){z.push(zug);zug=null}
 try{const r=await fetch("/bild?t="+T+"&voll="+voll,{method:"POST",body:z.join("\n")});
  if(!r.ok)throw new Error(await r.text());voll=0;fehler=0;einspielen(await r.arrayBuffer());
  const s=(performance.now()-t0)/1000;zust.textContent=w+"×"+h+" · verbunden";}
 catch(e){fehler++;voll=1;zust.textContent="getrennt – neuer Versuch ("+e.message+")";await new Promise(r=>setTimeout(r,Math.min(5000,300*fehler)))}}}
function pos(e){const r=c.getBoundingClientRect();return [((e.clientX-r.left)/r.width).toFixed(4),((e.clientY-r.top)/r.height).toFixed(4)]}
let unten=false;
c.addEventListener("pointerdown",e=>{c.setPointerCapture(e.pointerId);c.focus();unten=true;const[x,y]=pos(e);warte.push("ab "+x+" "+y);e.preventDefault()});
c.addEventListener("pointermove",e=>{if(!unten)return;const[x,y]=pos(e);zug="zieh "+x+" "+y});
c.addEventListener("pointerup",e=>{if(!unten)return;unten=false;if(zug){warte.push(zug);zug=null}warte.push("auf")});
c.addEventListener("pointercancel",()=>{if(unten){unten=false;warte.push("auf")}});
c.addEventListener("wheel",e=>{const[x,y]=pos(e);const d=Math.max(-0.3,Math.min(0.3,-e.deltaY/1500));warte.push("ab "+x+" "+y,"zieh "+x+" "+(+y+d/2).toFixed(4),"zieh "+x+" "+(+y+d).toFixed(4),"auf");e.preventDefault()},{passive:false});
c.addEventListener("contextmenu",e=>e.preventDefault());
document.addEventListener("keydown",e=>{if(e.target.tagName=="INPUT")return;if(e.key=="Enter")warte.push("enter");else if(e.key=="Backspace")warte.push("loeschen");else if(e.key=="Escape")warte.push("zurueck");else if(e.key.length==1&&!e.ctrlKey&&!e.metaKey)warte.push("zeichen "+e.key);else return;e.preventDefault()});
document.addEventListener("paste",e=>{const t=e.clipboardData.getData("text");if(t)warte.push("zeichen "+t.slice(0,400))});
async function aktion(a){const r=await fetch("/aktion?t="+T,{method:"POST",body:a});zust.textContent=await r.text()||a}
document.querySelectorAll("[data-z]").forEach(b=>b.onclick=()=>warte.push(b.dataset.z));
document.querySelectorAll("[data-a]").forEach(b=>b.onclick=()=>aktion(b.dataset.a));
document.getElementById("hell").onchange=e=>aktion("hell "+e.target.value);
document.getElementById("dreh").onclick=()=>{dreh=(dreh+90)%360;aktion("drehen "+dreh)};
document.getElementById("voll").onclick=()=>document.documentElement.requestFullscreen&&document.documentElement.requestFullscreen();
document.getElementById("datei").onchange=async e=>{const f=e.target.files[0];if(!f)return;zust.textContent="sende "+f.name+" …";
 const r=await fetch("/datei?t="+T+"&name="+encodeURIComponent(f.name),{method:"POST",body:f});zust.textContent=await r.text()};
c.addEventListener("dragover",e=>e.preventDefault());
c.addEventListener("drop",async e=>{e.preventDefault();for(const f of e.dataTransfer.files){const r=await fetch("/datei?t="+T+"&name="+encodeURIComponent(f.name),{method:"POST",body:f});zust.textContent=await r.text()}});
schleife();
</script></body></html>"##;

#[cfg(test)]
mod tests {
    #[test]
    fn url() {
        assert_eq!(super::url_dekodieren("Bild%20%C3%A4.png"), "Bild ä.png");
        assert_eq!(super::url_dekodieren("a+b%"), "a b%");
    }
}
