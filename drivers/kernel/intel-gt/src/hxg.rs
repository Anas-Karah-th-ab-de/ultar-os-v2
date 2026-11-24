//! Messages between host and GuC over registers (the "MMIO" channel of the HXG protocol): up to
//! four words are written to the message registers, the GuC is notified, and it answers in the
//! same registers. This is the channel a freshly started GuC understands; the fast command
//! buffers (CTB) are set up through it. Follows `xe_guc_mmio_send_recv` and
//! `abi/guc_messages_abi.h` of the Linux `xe` driver (MIT).

use crate::regs::*;
use crate::{field, Error, Hw, Result};

/// Number of message registers.
pub const WORDS: usize = 4;
/// How long the GuC may take to pick a request up (50 ms).
const REPLY_TIMEOUT_US: u32 = 50_000;
/// How long the GuC may stay busy with a request (2 s).
const BUSY_TIMEOUT_US: u32 = 2_000_000;
/// How often a request the GuC asks to retry is sent again.
const MAX_RETRIES: u32 = 8;

const MSG_ORIGIN_GUC: u32 = 1 << 31;
const MSG_TYPE: u32 = 0x7 << 28;
const TYPE_NO_RESPONSE_BUSY: u32 = 3;
const TYPE_NO_RESPONSE_RETRY: u32 = 5;
const TYPE_RESPONSE_FAILURE: u32 = 6;
const TYPE_RESPONSE_SUCCESS: u32 = 7;
const RESPONSE_DATA0: u32 = 0x0fff_ffff;
const FAILURE_HINT: u32 = 0xfff << 16;
const FAILURE_ERROR: u32 = 0xffff;

/// Action: copy the hardware configuration table to a GGTT address (size 0: only tell its size).
pub const ACTION_GET_HWCONFIG: u32 = 0x4100;

/// What the GuC answered: the 28 data bits of the first word and all four words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// `DATA0` of a success response.
    pub data0: u32,
    /// The reply registers.
    pub words: [u32; WORDS],
}

fn send(hw: &mut impl Hw, request: &[u32]) {
    for (i, w) in request.iter().enumerate() {
        hw.write(vf_sw_flag(i as u32), *w);
    }
    // posting read, then the notification
    hw.read(vf_sw_flag(WORDS as u32 - 1));
    hw.write(GUC_HOST_INTERRUPT, 0);
}

/// Waits for the GuC's answer to the request just sent, through a "busy" interim answer.
fn wait_header(hw: &mut impl Hw, request: u32) -> Result<u32> {
    let reply = vf_sw_flag(0);
    let header = match hw.wait(reply, MSG_ORIGIN_GUC, MSG_ORIGIN_GUC, REPLY_TIMEOUT_US) {
        Ok(u32::MAX) => return Err(Error::DeviceGone),
        Ok(h) => h,
        Err(_) => {
            return Err(Error::GucProtocol {
                request,
                reply: hw.read(reply),
            })
        }
    };
    if field(header, MSG_TYPE) != TYPE_NO_RESPONSE_BUSY {
        return Ok(header);
    }
    // both response types have the two upper type bits set
    let done = TYPE_RESPONSE_SUCCESS & TYPE_RESPONSE_FAILURE;
    let mask = MSG_ORIGIN_GUC | crate::prep(MSG_TYPE, done);
    hw.wait(reply, mask, mask, BUSY_TIMEOUT_US)
        .map_err(|_| Error::GucProtocol {
            request,
            reply: header,
        })
}

/// Sends a request of 1 to 4 words (word 0: the action, origin host, type request) and waits for
/// the response. Needs the `Gt` domain awake and a GuC that is ready.
pub fn send_recv(hw: &mut impl Hw, request: &[u32]) -> Result<Reply> {
    if request.is_empty() || request.len() > WORDS || request[0] & (MSG_ORIGIN_GUC | MSG_TYPE) != 0
    {
        return Err(Error::Invalid("GuC request"));
    }
    let mut backoff_us = 1000;
    for _ in 0..=MAX_RETRIES {
        send(hw, request);
        let header = wait_header(hw, request[0])?;
        match field(header, MSG_TYPE) {
            TYPE_RESPONSE_SUCCESS => {
                let mut words = [header, 0, 0, 0];
                for (i, w) in words.iter_mut().enumerate().skip(1) {
                    *w = hw.read(vf_sw_flag(i as u32));
                }
                return Ok(Reply {
                    data0: header & RESPONSE_DATA0,
                    words,
                });
            }
            TYPE_RESPONSE_FAILURE => {
                return Err(Error::GucRequest {
                    request: request[0],
                    error: header & FAILURE_ERROR,
                    hint: field(header, FAILURE_HINT),
                })
            }
            TYPE_NO_RESPONSE_RETRY => {
                hw.delay_us(backoff_us);
                backoff_us = (backoff_us * 2).min(256_000);
            }
            _ => {
                return Err(Error::GucProtocol {
                    request: request[0],
                    reply: header,
                })
            }
        }
    }
    Err(Error::GucProtocol {
        request: request[0],
        reply: hw.read(vf_sw_flag(0)),
    })
}

/// Asks the GuC for the hardware configuration table: with `size` 0 it only reports the table's
/// size in bytes; otherwise it copies the table to `ggtt` (a buffer of at least `size` bytes).
pub fn get_hwconfig(hw: &mut impl Hw, ggtt: u64, size: u32) -> Result<u32> {
    let request = [ACTION_GET_HWCONFIG, ggtt as u32, (ggtt >> 32) as u32, size];
    send_recv(hw, &request).map(|r| r.data0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    /// A GuC that answers each notification with the next value of `answers` in word 0.
    fn guc(answers: Vec<u32>) -> Sim {
        let mut s = Sim::new();
        let mut it = answers.into_iter();
        s.on_write(
            GUC_HOST_INTERRUPT,
            Box::new(move |r, _, _| {
                if let Some(a) = it.next() {
                    r.insert(vf_sw_flag(0), a);
                    r.insert(vf_sw_flag(1), 0x11);
                }
            }),
        );
        s
    }

    const SUCCESS: u32 = 1 << 31 | 7 << 28;

    #[test]
    fn hwconfig_size_query() {
        let mut s = guc(alloc::vec![SUCCESS | 0x1234]);
        assert_eq!(get_hwconfig(&mut s, 0, 0), Ok(0x1234));
        assert_eq!(
            &s.writes[..5],
            &[
                (0x19_0240, 0x4100),
                (0x19_0244, 0),
                (0x19_0248, 0),
                (0x19_024c, 0),
                (0x19_01f0, 0)
            ]
        );
    }

    #[test]
    fn copy_request_carries_address_and_size() {
        let mut s = guc(alloc::vec![SUCCESS]);
        get_hwconfig(&mut s, 0x1_0080_0000, 0x1234).unwrap();
        assert_eq!(
            s.writes[1..4],
            [
                (0x19_0244, 0x0080_0000),
                (0x19_0248, 1),
                (0x19_024c, 0x1234)
            ]
        );
    }

    #[test]
    fn reply_words_are_returned() {
        let mut s = guc(alloc::vec![SUCCESS | 5]);
        let r = send_recv(&mut s, &[0x4100]).unwrap();
        assert_eq!((r.data0, r.words[0], r.words[1]), (5, SUCCESS | 5, 0x11));
    }

    #[test]
    fn failure_carries_error_and_hint() {
        let mut s = guc(alloc::vec![1 << 31 | 6 << 28 | 0x7 << 16 | 0x30]);
        assert_eq!(
            send_recv(&mut s, &[0x4100]),
            Err(Error::GucRequest {
                request: 0x4100,
                error: 0x30,
                hint: 7
            })
        );
    }

    #[test]
    fn retry_is_sent_again_and_busy_is_waited_out() {
        let mut s = guc(alloc::vec![1 << 31 | 5 << 28, SUCCESS | 9]);
        assert_eq!(send_recv(&mut s, &[0x4100]).unwrap().data0, 9);
        assert_eq!(s.writes_to(GUC_HOST_INTERRUPT).len(), 2);
        let mut s = guc(alloc::vec![1 << 31 | 3 << 28]);
        s.after_reads(vf_sw_flag(0), 5, SUCCESS | 3);
        assert_eq!(send_recv(&mut s, &[0x4100]).unwrap().data0, 3);
    }

    #[test]
    fn silence_and_nonsense_are_errors() {
        let mut s = Sim::new();
        assert_eq!(
            send_recv(&mut s, &[0x4100]),
            Err(Error::GucProtocol {
                request: 0x4100,
                reply: 0x4100
            })
        );
        let mut s = guc(alloc::vec![1 << 31 | 1 << 28]);
        assert!(matches!(
            send_recv(&mut s, &[0x4100]),
            Err(Error::GucProtocol { .. })
        ));
        // a GuC that asks to retry for ever ends after the bounded number of attempts
        let mut s = guc(alloc::vec![1 << 31 | 5 << 28; 20]);
        assert!(matches!(
            send_recv(&mut s, &[0x4100]),
            Err(Error::GucProtocol { .. })
        ));
        assert_eq!(s.writes_to(GUC_HOST_INTERRUPT).len(), 9);
        let mut s = Sim::new();
        assert_eq!(send_recv(&mut s, &[]), Err(Error::Invalid("GuC request")));
        assert_eq!(
            send_recv(&mut s, &[1 << 31]),
            Err(Error::Invalid("GuC request"))
        );
        assert!(s.writes.is_empty());
    }
}
