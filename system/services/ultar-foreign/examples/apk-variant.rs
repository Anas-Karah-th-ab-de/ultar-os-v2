//! `apk-variant` – test copies of a real APK for `tools/foreign/android-e2e.sh`
//! (feature `test-sign`, never part of the service):
//!
//! ```text
//! apk-variant resign IN OUT SIGNER   signed again (v2) by the test signer SIGNER
//! apk-variant tamper IN OUT ENTRY    one byte of ENTRY's data changed
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ultar_foreign::apk::testsign;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [what, input, output, extra] = args.as_slice() else {
        eprintln!("apk-variant resign|tamper IN OUT SIGNER|ENTRY");
        std::process::exit(2);
    };
    let apk = std::fs::read(input).expect("read IN");
    let out = match what.as_str() {
        "resign" => testsign::resign(&apk, extra),
        "tamper" => testsign::tamper(&apk, extra),
        _ => None,
    }
    .expect("no variant");
    std::fs::write(output, out).expect("write OUT");
}
