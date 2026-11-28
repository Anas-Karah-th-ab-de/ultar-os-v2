//! Remote hardware test (`/boot/efi/ultar/ferntest`): a test computer started straight into Ultar
//! comes back by itself and leaves its record behind.
//!
//! When the file is on the start partition, the kernel
//! * arms the restart deadline ([`crate::apic::arm_deadline`]): after the seconds in the file
//!   (60…3600, default 300) the timer interrupt restarts the machine – the firmware then starts
//!   its first boot entry again (on server3: Ubuntu), even when nothing in Ultar answers;
//! * starts a small shell loop that copies the kernel log, the devices, the network and the
//!   services to `/data/ultar/ferntest/` every 10 seconds, so the other system can read them.
//!
//! Without the file nothing of this happens. The file is written by the operator from the other
//! system on the same disk; it never comes with an image.

use alloc::string::String;
use alloc::vec::Vec;

/// Where the operator asks for the test (the Ultar start partition).
const FLAG: &str = "/boot/efi/ultar/ferntest";
/// Shell of the image (busybox `sh`).
const SHELL: &str = "/bin/sh";
/// The copy loop: records into the data partition, written whole and then renamed.
const LOOP: &str = "d=/data/ultar/ferntest; mkdir -p $d; i=0; \
while true; do i=$((i+1)); \
for f in protokoll geraet anzeige firewall boot_slot sicherheit; do cat /proc/ultar/$f > $d/$f.tmp 2>&1; mv $d/$f.tmp $d/$f.txt; done; \
(uinit status; echo; ps; echo; cat /proc/mounts; echo; ls -la /dev/input /dev/ultar 2>&1) > $d/system.tmp 2>&1; mv $d/system.tmp $d/system.txt; \
echo \"$i $(cat /proc/uptime)\" > $d/lauf.txt; sync; sleep 10; done";

/// Seconds until the restart, from the flag file's first number.
fn seconds(text: &[u8]) -> u64 {
    core::str::from_utf8(text)
        .ok()
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(300)
        .clamp(60, 3600)
}

/// Called once at start, after the file systems are mounted.
pub fn starten() {
    let Some(text) = crate::fs::read_all(FLAG) else { return };
    let secs = seconds(&text);
    crate::apic::arm_deadline(secs * 1000);
    crate::kprintln!("[ferntest] restart in {secs} s, records in /data/ultar/ferntest");
    if crate::fs::stat(SHELL).is_none() {
        return;
    }
    let envp: Vec<String> = ["PATH=/usr/bin:/bin:/usr/sbin:/sbin", "HOME=/root", "LANG=C.UTF-8", "ULTAR_KERN=1"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    let argv = alloc::vec![String::from("sh"), String::from("-c"), String::from(LOOP)];
    if let Err(e) = crate::proc::spawn_hintergrund(SHELL, argv, envp) {
        crate::kprintln!("[ferntest] no record loop: {e}");
    }
}
