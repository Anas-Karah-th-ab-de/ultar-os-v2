//! Whether the driver may write to the NPU at all. Without an opt-in the kernel only reads the
//! buttress and reports what it finds.
//!
//! Opt-in: the boot option [`OPTION_WORD`] (removed from the load options before the A/B start
//! chain parses them), the file [`FILE_PATH`] on the start partition with `on`, or the device id
//! in [`ALLOWLIST`] (empty: ids are added after a recorded run on that machine).

/// Device ids that are driven without an opt-in.
pub const ALLOWLIST: &[u16] = &[];
/// Boot option (UEFI load options, words separated by spaces).
pub const OPTION_WORD: &str = "ultar.intel_npu=on";
/// File on the start partition (UEFI path) whose content `on` opts in.
pub const FILE_PATH: &str = "\\ultar\\intel-npu";

/// Whether device `id` may be driven.
pub fn allowed(id: u16, opted_in: bool) -> bool {
    opted_in || ALLOWLIST.contains(&id)
}

/// Whether the content of [`FILE_PATH`] opts in (`on`, surrounding white space ignored).
pub fn file_says_on(bytes: &[u8]) -> bool {
    bytes.trim_ascii() == b"on"
}

/// Removes every [`OPTION_WORD`] from UTF-16 load options (up to the first 0) in place, without
/// allocating (it runs before the kernel heap exists). Returns the length of what is left (the
/// rest of `units` is zeroed) and whether the word was there.
pub fn take_option(units: &mut [u16]) -> (usize, bool) {
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    let space = u16::from(b' ');
    let (mut read, mut write, mut found) = (0, 0, false);
    while read < end {
        let stop = units[read..end]
            .iter()
            .position(|u| *u == space)
            .map_or(end, |n| read + n);
        let word = read..stop;
        read = stop + 1;
        if word.is_empty() {
            continue;
        }
        if OPTION_WORD
            .encode_utf16()
            .eq(units[word.clone()].iter().copied())
        {
            found = true;
            continue;
        }
        if write > 0 {
            units[write] = space;
            write += 1;
        }
        // write <= word.start: copying forward never overwrites unread units
        units.copy_within(word.clone(), write);
        write += word.len();
    }
    units[write..].fill(0);
    (write, found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;

    fn run(s: &str) -> (String, bool) {
        let mut u: Vec<u16> = s.encode_utf16().collect();
        u.push(0);
        let (n, found) = take_option(&mut u);
        assert!(u[n..].iter().all(|x| *x == 0));
        (String::from_utf16(&u[..n]).unwrap(), found)
    }

    #[test]
    fn takes_only_its_own_word() {
        assert_eq!(
            run("ultar.slot=a ultar.intel_npu=on ultar.trial=1"),
            ("ultar.slot=a ultar.trial=1".into(), true)
        );
        assert_eq!(run("ultar.intel_npu=on"), (String::new(), true));
        assert_eq!(
            run("ultar.slot=b  ultar.intel_npu=off"),
            ("ultar.slot=b ultar.intel_npu=off".into(), false)
        );
        assert_eq!(
            run("ultar.intel_npu=on ultar.intel_npu=on x"),
            ("x".into(), true)
        );
        assert_eq!(run(""), (String::new(), false));
    }

    #[test]
    fn file_and_allowlist() {
        assert!(file_says_on(b" on\r\n") && !file_says_on(b"off") && !file_says_on(b""));
        assert!(allowed(0x7d1d, true) && !allowed(0x7d1d, false));
    }
}
