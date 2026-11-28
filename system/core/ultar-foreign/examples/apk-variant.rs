//! `apk-variant` – test copies of a real APK for `tools/foreign/android-e2e.sh`
//! (feature `test-sign`, never part of the service):
//!
//! ```text
//! apk-variant resign IN OUT SIGNER   signed again (v2) by the test signer SIGNER
//! apk-variant tamper IN OUT ENTRY    one byte of ENTRY's data changed
//! ```

use ultar_foreign::apk::testsign;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [what, input, output, extra] = args.as_slice() else {
        eprintln!("apk-variant resign|tamper IN OUT SIGNER|ENTRY");
        std::process::exit(2);
    };
    if let Err(e) = variant(what, input, output, extra) {
        eprintln!("apk-variant: {e}");
        std::process::exit(1);
    }
}

fn variant(what: &str, input: &str, output: &str, extra: &str) -> Result<(), String> {
    let apk = std::fs::read(input).map_err(|e| format!("read {input}: {e}"))?;
    let out = match what {
        "resign" => testsign::resign(&apk, extra),
        "tamper" => testsign::tamper(&apk, extra),
        _ => None,
    }
    .ok_or("no such variant")?;
    std::fs::write(output, out).map_err(|e| format!("write {output}: {e}"))
}
