//! Splits messages into 64-byte HID reports and puts them back together.
//!
//! Byte 0 of every report: bit 7 set if more reports follow for this
//! message, bits 0..=5 the number of payload bytes that follow.

use heapless::Vec;
use serde::{de::DeserializeOwned, Serialize};

use crate::MAX_MESSAGE;

pub const REPORT_SIZE: usize = 64;
const PAYLOAD: usize = REPORT_SIZE - 1;
const MORE: u8 = 0x80;

/// Serialise `message` and hand each report to `send`.
pub fn encode<T: Serialize>(message: &T, mut send: impl FnMut(&[u8; REPORT_SIZE])) -> Result<(), postcard::Error> {
    let mut buf = [0u8; MAX_MESSAGE];
    let bytes = postcard::to_slice(message, &mut buf)?;
    let mut chunks = bytes.chunks(PAYLOAD).peekable();
    while let Some(chunk) = chunks.next() {
        let mut report = [0u8; REPORT_SIZE];
        report[0] = chunk.len() as u8 | if chunks.peek().is_some() { MORE } else { 0 };
        report[1..=chunk.len()].copy_from_slice(chunk);
        send(&report);
    }
    Ok(())
}

/// Collects reports until a message is complete.
#[derive(Default)]
pub struct Assembler {
    buf: Vec<u8, MAX_MESSAGE>,
    overflow: bool,
}

impl Assembler {
    pub const fn new() -> Self {
        Self { buf: Vec::new(), overflow: false }
    }

    /// Feed one report. Returns the decoded message once its last report
    /// arrived, or an error if it was too long or did not decode.
    pub fn push<T: DeserializeOwned>(&mut self, report: &[u8]) -> Option<Result<T, postcard::Error>> {
        let (&head, rest) = report.split_first()?;
        let len = ((head & !MORE) as usize).min(rest.len());
        if self.buf.extend_from_slice(&rest[..len]).is_err() {
            self.overflow = true;
        }
        if head & MORE != 0 {
            return None;
        }
        let result = if self.overflow {
            Err(postcard::Error::DeserializeUnexpectedEnd)
        } else {
            postcard::from_bytes(&self.buf)
        };
        self.buf.clear();
        self.overflow = false;
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HardwareConfig, MatrixConfig, Request};

    fn round_trip(request: &Request) -> (Request, usize) {
        let mut assembler = Assembler::new();
        let mut decoded = None;
        let mut reports = 0;
        encode(request, |report| {
            reports += 1;
            assert!(decoded.is_none(), "report after end of message");
            decoded = assembler.push::<Request>(report);
        })
        .unwrap();
        (decoded.expect("message incomplete").unwrap(), reports)
    }

    #[test]
    fn small_message_fits_one_report() {
        assert_eq!(round_trip(&Request::GetInfo), (Request::GetInfo, 1));
    }

    #[test]
    fn large_message_spans_reports() {
        let request = Request::SetHardware(HardwareConfig {
            matrix: MatrixConfig::Direct { pins: (100..164).collect(), active_high: false },
            reserved_pins: (0..32).collect(),
            debounce_ms: 5,
        });
        let (decoded, reports) = round_trip(&request);
        assert_eq!(decoded, request);
        assert_eq!(reports, 2);
    }

    #[test]
    fn assembler_recovers_after_garbage() {
        let mut assembler = Assembler::new();
        assert!(assembler.push::<Request>(&[3, 0xFF, 0xFF, 0xFF]).unwrap().is_err());
        assert_eq!(round_trip(&Request::Save).0, Request::Save);
        let mut report = [0u8; REPORT_SIZE];
        encode(&Request::StopProbe, |r| report = *r).unwrap();
        assert_eq!(assembler.push::<Request>(&report).unwrap().unwrap(), Request::StopProbe);
    }
}
