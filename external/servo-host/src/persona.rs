//! The persona in Servo (`ultar_web::privacy::persona`): preferences that switch off what
//! identifies or reaches the device, the page script that answers for the device, and the screen
//! pages are told about.

use servo::{DeviceIntRect, DeviceIntSize, Opts, Preferences, ScreenGeometry};
use ultar_web::privacy::persona::Persona;

/// Steps of the clock pages can read (milliseconds), as Tor Browser.
const CLOCK_STEP_MS: u32 = 100;

/// Servo's options: nothing is kept after the session (cookies, storage, cache in memory).
pub fn opts() -> Opts {
    Opts {
        temporary_storage: true,
        ..Opts::default()
    }
}

/// Servo's preferences for `persona`, sending everything through `proxy`.
pub fn preferences(persona: &Persona, proxy: &str) -> Preferences {
    let mut p = Preferences::default();
    p.user_agent = persona.user_agent.into();
    p.intl_locale_override = "en-US".into();
    // the only way out: Tor; plain HTTP is upgraded to HTTPS (onion sites keep HTTP)
    p.network_http_proxy_uri = proxy.into();
    p.network_https_proxy_uri = proxy.into();
    p.network_http_no_proxy = String::new();
    p.network_enforce_tls_enabled = true;
    p.network_enforce_tls_onion = false;
    // what could reach or identify the device
    p.dom_webrtc_enabled = false;
    p.dom_webrtc_transceiver_enabled = false;
    p.dom_bluetooth_enabled = false;
    p.dom_gamepad_enabled = false;
    p.dom_webgpu_enabled = false;
    p.dom_webxr_enabled = false;
    p.dom_serviceworker_enabled = false;
    p.dom_async_clipboard_enabled = false;
    p.devtools_server_enabled = false;
    // a page that asks for the location (and is allowed) gets the invented place
    p.dom_geolocation_enabled = true;
    p.dom_geolocation_test_position_enabled = true;
    p.dom_geolocation_test_latitude = persona.place.lat;
    p.dom_geolocation_test_longitude = persona.place.lon;
    p.dom_geolocation_test_accuracy = persona.place.accuracy_m;
    p
}

/// The page script with the persona's values filled in.
pub fn script(persona: &Persona) -> String {
    let languages: Vec<String> = persona
        .accept_language
        .split(',')
        .map(|l| l.split(';').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| format!("\"{l}\""))
        .collect();
    let values = format!(
        "{{platform:\"{}\",oscpu:\"Windows NT 10.0; Win64; x64\",cores:{},language:\"en-US\",\
         languages:[{}],screen:[{},{}],clockStepMs:{}}}",
        persona.platform,
        persona.cores,
        languages.join(","),
        persona.screen.0,
        persona.screen.1,
        CLOCK_STEP_MS
    );
    include_str!("persona.js").replace("__PERSONA__", &values)
}

/// The screen pages are told about: the persona's, in device pixels at `scale`.
pub fn screen(persona: &Persona, scale: f32, view: (u32, u32)) -> ScreenGeometry {
    let px = |v: u32| (v as f32 * scale).round() as i32;
    let size = DeviceIntSize::new(px(persona.screen.0), px(persona.screen.1));
    ScreenGeometry {
        size,
        available_size: size,
        window_rect: DeviceIntRect::from_size(DeviceIntSize::new(view.0 as i32, view.1 as i32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_names_the_persona_and_nothing_else() {
        let persona = Persona::for_session(3);
        let s = script(&persona);
        assert!(s.contains("platform:\"Win32\""));
        assert!(s.contains("languages:[\"en-US\",\"en\"]"));
        assert!(s.contains("screen:[1400,900]"));
        assert!(!s.contains("__PERSONA__"));
        let p = preferences(&persona, "http://u:p@127.0.0.1:1");
        assert_eq!(p.network_https_proxy_uri, "http://u:p@127.0.0.1:1");
        assert!(!p.dom_webrtc_enabled && p.network_enforce_tls_enabled);
        assert_eq!(screen(&persona, 2.0, (10, 10)).size.width, 2800);
    }
}
