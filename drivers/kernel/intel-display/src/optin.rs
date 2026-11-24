//! Whether the driver may write to the display hardware at all. A machine nobody tested the
//! takeover on must keep the firmware picture: without an opt-in the kernel only reads the state
//! (identification, readout) and the console stays on the firmware framebuffer.
//!
//! Opt-in: the boot option [`OPTION_WORD`] (removed from the load options before the A/B start
//! chain parses them), the file [`FILE_PATH`] on the start partition with `on`, or the device id
//! in [`ALLOWLIST`] (empty: ids are added after a recorded bare-metal boot on that machine).

use alloc::vec::Vec;

/// Device ids that are driven without an opt-in.
pub const ALLOWLIST: &[u16] = &[];

/// Boot option (UEFI load options, words separated by spaces).
pub const OPTION_WORD: &str = "ultar.intel_display=on";

/// File on the start partition (UEFI path) whose content `on` opts in.
pub const FILE_PATH: &str = "\\ultar\\intel-display";

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
/// rest of `units` is zeroed, so it stays 0-terminated when shorter) and whether the word was there.
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

/// GGTT addresses of kept buffers that no plane shows (or will show): these may be released.
/// A buffer the console still draws into is passed in `shown` too.
pub fn releasable(kept: &[u32], shown: &[u32]) -> Vec<u32> {
    kept.iter()
        .copied()
        .filter(|g| !shown.contains(g))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(core::iter::once(0)).collect()
    }

    #[test]
    fn nothing_is_allowed_by_default() {
        assert!(ALLOWLIST.is_empty());
        assert!(!allowed(0x7d51, false));
        assert!(allowed(0x7d51, true));
    }

    fn take(s: &str) -> (Vec<u16>, bool) {
        let mut u = utf16(s);
        u.extend_from_slice(&[0x7777; 3]); // garbage after the terminator is ignored
        let (n, on) = take_option(&mut u);
        assert!(u[n..].iter().all(|x| *x == 0), "zeroed after the rest");
        (u[..n].to_vec(), on)
    }

    fn plain(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn the_word_is_taken_out_of_the_load_options() {
        let (rest, on) = take("ultar.slot=a ultar.intel_display=on ultar.trial=1");
        assert!(on);
        assert_eq!(rest, plain("ultar.slot=a ultar.trial=1"));
        assert_eq!(take("ultar.intel_display=on"), (Vec::new(), true));
        assert_eq!(take("ultar.intel_display=on  x"), (plain("x"), true));
        assert_eq!(take("ultar.slot=b"), (plain("ultar.slot=b"), false));
        // a similar word is not the opt-in (and stays for the strict parser to refuse)
        let off = "ultar.intel_display=off";
        assert_eq!(take(off), (plain(off), false));
        assert_eq!(take_option(&mut []), (0, false));
        let mut full = plain("a ultar.intel_display=on"); // no terminator
        assert_eq!(take_option(&mut full), (1, true));
        assert_eq!(&full[..2], &[u16::from(b'a'), 0]);
    }

    #[test]
    fn file_content() {
        assert!(file_says_on(b"on\n"));
        assert!(file_says_on(b" on "));
        assert!(!file_says_on(b"off"));
        assert!(!file_says_on(b""));
        assert!(!file_says_on(b"onx"));
    }

    #[test]
    fn only_buffers_nothing_shows_are_released() {
        assert_eq!(
            releasable(&[0x100_0000, 0x200_0000, 0x300_0000], &[0x200_0000]),
            [0x100_0000, 0x300_0000]
        );
        // old console buffer still shown/drawn into: kept until the console switched
        assert_eq!(
            releasable(&[0x100_0000, 0x200_0000], &[0x100_0000, 0x200_0000]),
            [0u32; 0]
        );
    }
}
