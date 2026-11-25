//! The identity every page sees. It is the same for everyone (a crowd to hide in) and has
//! nothing to do with the device: Tor Browser's "Firefox on Windows" in English, UTC, a
//! rounded screen, a few cores. A page that asks where the device is gets a place invented
//! for this session (a large city somewhere in the world), never the real one.

/// What a page may read about "the device".
#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    /// `User-Agent` and `navigator.userAgent`.
    pub user_agent: &'static str,
    /// `Accept-Language` and `navigator.languages`.
    pub accept_language: &'static str,
    /// `navigator.platform`.
    pub platform: &'static str,
    /// The time zone shown (`Intl`, `Date`).
    pub time_zone: &'static str,
    /// Screen size shown (rounded, as letterboxing does).
    pub screen: (u32, u32),
    /// `navigator.hardwareConcurrency`.
    pub cores: u32,
    /// The invented position for the geolocation API.
    pub place: Place,
}

/// An invented position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Accuracy shown, in metres (a city, not a street).
    pub accuracy_m: f64,
}

/// Large cities the invented position is taken from (city centres, public knowledge).
const CITIES: [(f64, f64); 12] = [
    (40.7128, -74.0060),
    (51.5074, -0.1278),
    (48.8566, 2.3522),
    (35.6762, 139.6503),
    (-23.5505, -46.6333),
    (19.4326, -99.1332),
    (52.5200, 13.4050),
    (-33.8688, 151.2093),
    (1.3521, 103.8198),
    (43.6532, -79.3832),
    (55.7558, 37.6173),
    (-26.2041, 28.0473),
];

/// The user agent of Tor Browser (Firefox ESR on Windows): what the largest group of anonymous
/// users sends.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0";
/// The language every page is asked in.
pub const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.5";
/// What pages are asked for.
pub const ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

impl Persona {
    /// The persona of a session; `seed` picks the invented place (a new one each session).
    pub fn for_session(seed: u64) -> Persona {
        let (lat, lon) = CITIES[(seed % CITIES.len() as u64) as usize];
        Persona {
            user_agent: USER_AGENT,
            accept_language: ACCEPT_LANGUAGE,
            platform: "Win32",
            time_zone: "UTC",
            screen: (1400, 900),
            cores: 4,
            place: Place {
                lat,
                lon,
                accuracy_m: 5000.0,
            },
        }
    }

    /// A seed for a new session (time and process: not derived from anything about the user).
    pub fn new_seed() -> u64 {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        t ^ (u64::from(std::process::id()) << 32)
    }

    /// The request headers a page fetch carries: these and nothing else (no `Referer`, no
    /// cookies, no client hints, no `DNT` – a rare header is a mark).
    pub fn headers(&self) -> [(&'static str, &'static str); 3] {
        [
            ("User-Agent", self.user_agent),
            ("Accept", ACCEPT),
            ("Accept-Language", self.accept_language),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_session_looks_the_same_but_the_place_is_invented() {
        let a = Persona::for_session(1);
        let b = Persona::for_session(2);
        assert_eq!(
            (a.user_agent, a.time_zone, a.screen),
            (b.user_agent, b.time_zone, b.screen)
        );
        assert_ne!(a.place, b.place);
        assert!(!a.user_agent.contains("Ultar") && !a.user_agent.contains("Linux"));
        assert_eq!(a.headers().len(), 3);
        assert!(a
            .headers()
            .iter()
            .all(|(k, _)| *k != "Referer" && *k != "Cookie"));
    }
}
