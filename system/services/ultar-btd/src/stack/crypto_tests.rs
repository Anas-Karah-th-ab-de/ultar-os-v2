//! The sample data of the Core specification (Vol 3 Part H Appendix D for f4/f5/f6/g2;
//! Vol 2 Part G 7.1.2 for the P-256 samples), as the Linux kernel's self test carries
//! them (wire order, little-endian) – converted to the specification's order here.

use super::*;

/// Hex text in wire order (little-endian) → big-endian array.
fn le<const N: usize>(hex: &str) -> [u8; N] {
    let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(hex.len(), 2 * N);
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
    }
    swap(&out)
}

const PKAX: &str = "e69d350e480103ccdbfdf4ac1191f4efb9a5f9e9a7832c5e2cbe97f2d203b020";
const PKBX: &str = "fdc57ff449dd4f6bfb7c9df1c29acb592ae7d4eefbfc0a909abbf6323d8b1855";
const NA: &str = "abae2b71ecb2ffff3e7377d15484cbd5";
const NB: &str = "cfc43dfff78365216e5fa725cce7e8a6";
const A1: &str = "cebf3737125600";
const A2: &str = "c1cf2d7013a700";

#[test]
fn f4_sample() {
    let r = f4(&le(PKAX), &le(PKBX), &le(NA), 0);
    assert_eq!(r, le::<16>("2d8774a9bea1edf11cbda907f116c9f2"));
}

#[test]
fn f5_sample() {
    let w = le("98a6bf73f3348d86f166f8b4136b79999b7d390aa610103405adc857a33402ec");
    let (mac_key, ltk) = f5(&w, &le(NA), &le(NB), &le(A1), &le(A2));
    assert_eq!(*mac_key, le::<16>("206e63ce206a3ffd024a08a176f16529"));
    assert_eq!(*ltk, le::<16>("380a7594b522059823cdd76911798669"));
}

#[test]
fn f6_sample() {
    let r = f6(
        &le("206e63ce206a3ffd024a08a176f16529"),
        &le(NA),
        &le(NB),
        &le("c80f2d0cd242da0854bb53b43b34a312"),
        &le("020101"),
        &le(A1),
        &le(A2),
    );
    assert_eq!(r, le::<16>("618f95da090b6cd2c5e8d09c9873c4e3"));
}

#[test]
fn g2_sample() {
    assert_eq!(
        g2(&le(PKAX), &le(PKBX), &le(NA), &le(NB)),
        0x2f9e_d5ba % 1_000_000
    );
}

fn p256_sample(priv_a: &str, priv_b: &str, pub_a: &str, pub_b: &str, dh: &str) {
    let (pa, pb) = (le::<64>(pub_a), le::<64>(pub_b));
    // Wire order is X (LE) || Y (LE); reversed whole it is Y (BE) || X (BE).
    let split = |p: [u8; 64]| {
        let (mut x, mut y) = ([0u8; 32], [0u8; 32]);
        y.copy_from_slice(&p[..32]);
        x.copy_from_slice(&p[32..]);
        (x, y)
    };
    let ((ax, ay), (bx, by)) = (split(pa), split(pb));
    let a = KeyPair::from_private(&le(priv_a)).unwrap();
    let b = KeyPair::from_private(&le(priv_b)).unwrap();
    assert_eq!((a.x, a.y), (ax, ay), "public key A");
    assert_eq!((b.x, b.y), (bx, by), "public key B");
    let expected: [u8; 32] = le(dh);
    assert_eq!(*a.dh_key(&bx, &by).unwrap(), expected);
    assert_eq!(*b.dh_key(&ax, &ay).unwrap(), expected);
}

#[test]
fn p256_samples_of_the_specification() {
    p256_sample(
        "bd1a3ccda6b8995899b740eb7b60ff4a503f10d2e3b3c974385fc5a3d4f6493f",
        "fdc57ff449dd4f6bfb7c9df1c29acb592ae7d4eefbfc0a909abbf6323d8b1855",
        "e69d350e480103ccdbfdf4ac1191f4efb9a5f9e9a7832c5e2cbe97f2d203b020\
         8bd28915d08e1c742430ed8fc245637 65c15525abf9a32636deb2a65499c80dc",
        "90a1aa2fb27790559fa61586fd8ab547004c9ef1842259099 61daf1ff0f0a11e\
         4a21b115f9af895f76368ee230112d476051b89a3a70567337ad9d423ef3554c",
        "98a6bf73f3348d86f166f8b4136b79999b7d390aa610103405adc857a33402ec",
    );
    p256_sample(
        "637645d0f773acb7ffdd0372b97285b441b65d0c5d5484601aa39a3c6916a506",
        "ba30555019a2caa3a52908c6b503887e032b5073d42e509764cd720d67a09a52",
        "dd785c74039b7e98cb94874aadfaf8d5433e5cafeab54cf49e8079577ba4312c\
         4f5d71437743f8ead43ebd17911021d01f87438e40e252cdbedf983818129591",
        "cc0065e1f56c0dcfec96472066c9db848175a84dc0dfc79d1b3f3df23fe465f4\
         79b2ecd8ca55a1a8434d6bca10b0c201c2334e1624c4efee99d8bbbc48d00102",
        "69eb2132f2c605416019cd5e94e1e65f3307e3384b68e5623f886d2f3a8485ab",
    );
}

#[test]
fn points_off_the_curve_are_refused() {
    let a = KeyPair::generate().unwrap();
    let mut y = a.y;
    y[31] ^= 1;
    assert_eq!(a.dh_key(&a.x, &y).unwrap_err(), CryptoError::BadKey);
    assert_eq!(
        a.dh_key(&[0; 32], &[0; 32]).unwrap_err(),
        CryptoError::BadKey
    );
    assert!(KeyPair::from_private(&[0; 32]).is_err());
    let b = KeyPair::generate().unwrap();
    assert_eq!(a.dh_key(&b.x, &b.y).unwrap(), b.dh_key(&a.x, &a.y).unwrap());
    assert_ne!(a.x, b.x);
}

#[test]
fn ah_sample_and_address_resolution() {
    let irk = le::<16>("9b7d390aa610103405adc857a33402ec");
    assert_eq!(ah(&irk, &le("948170")), le::<3>("aafb0d"));
    // The address 70:81:94:0D:FB:AA (prand 0x708194, hash 0x0dfbaa) in wire order.
    let rpa = [0xaa, 0xfb, 0x0d, 0x94, 0x81, 0x70];
    assert!(resolves(&irk, &rpa));
    let mut other = rpa;
    other[0] ^= 1;
    assert!(!resolves(&irk, &other));
}
