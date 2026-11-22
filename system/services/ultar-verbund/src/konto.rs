//! Ultar-Konto: Geräte desselben Kontos vertrauen sich automatisch (Server-Liste).

use crate::config::{Config, Partner, Rolle};
use crate::ident::Ident;

/// https-Basis aus der Relais-Adresse ableiten (wss://h/v1/... → https://h)
pub fn basis(relais: &str) -> String {
    let r = relais
        .replacen("wss://", "https://", 1)
        .replacen("ws://", "http://", 1);
    match r.find("/v1/") {
        Some(i) => r[..i].to_string(),
        None => r.trim_end_matches('/').to_string(),
    }
}

pub fn anmelden(c: &Config, benutzer: &str, passwort: &str) -> Result<String, String> {
    let r = ureq::post(&format!("{}/v1/account/login", basis(&c.relais)))
        .timeout(std::time::Duration::from_secs(15))
        .send_json(serde_json::json!({"username":benutzer,"password":passwort}))
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
    v["token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "kein Token".into())
}

/// eigenes Gerät beim Konto melden und Liste der Kontogeräte holen → als Partner (Rolle voll) übernehmen
pub fn abgleichen(c: &mut Config, ich: &Ident) -> Result<usize, String> {
    if c.token.is_empty() {
        return Err("kein Konto".into());
    }
    let b = basis(&c.relais);
    let auth = format!("Bearer {}", c.token);
    ureq::post(&format!("{b}/v1/verbund/geraet"))
        .set("authorization", &auth)
        .timeout(std::time::Duration::from_secs(15))
        .send_json(serde_json::json!({"pk":ich.pk_hex(),"name":c.name,"ausgabe":crate::ressourcen::ausgabe()}))
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = ureq::get(&format!("{b}/v1/verbund/geraete"))
        .set("authorization", &auth)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for g in v["geraete"].as_array().cloned().unwrap_or_default() {
        let pk = g["pk"].as_str().unwrap_or("").to_string();
        if pk == ich.pk_hex() || pk.len() != 64 {
            continue;
        }
        let Some(roh) = crate::ident::unhex(&pk) else {
            continue;
        };
        if c.partner(&pk).map(|p| p.quelle == "code").unwrap_or(false) {
            continue; // per Code gekoppelt: Rolle des Nutzers behalten
        }
        c.hinzufuegen(Partner {
            kennung: crate::ident::kennung(&roh),
            pk,
            name: g["name"].as_str().unwrap_or("").to_string(),
            rolle: Rolle::Voll,
            quelle: "konto".into(),
        });
        n += 1;
    }
    Ok(n)
}
