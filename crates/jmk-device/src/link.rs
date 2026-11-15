//! The connection between the two halves of a split keyboard.
//!
//! The peripheral half sends the complete state of its keys whenever it
//! changes and a few times a second regardless, so a lost message corrects
//! itself and no key can stay stuck. Encoder clicks have no state and are
//! sent once. The central half sends a short hello so the peripheral knows
//! it is being heard.
//!
//! Frame: 0x4A, length, type, payload, CRC-8 over length, type and payload.

use jmk_core::Millis;
use jmk_proto::MAX_KEYS;

/// A byte pipe to the other half. Both calls return at once.
pub trait Link {
    /// Queue bytes for sending. Bytes that do not fit are dropped.
    fn send(&mut self, bytes: &[u8]);
    /// Copy received bytes into `buf` and return how many.
    fn receive(&mut self, buf: &mut [u8]) -> usize;
}

/// For keyboards in one piece.
impl Link for () {
    fn send(&mut self, _: &[u8]) {}
    fn receive(&mut self, _: &mut [u8]) -> usize {
        0
    }
}

const START: u8 = 0x4A;
const BITMAP: usize = MAX_KEYS / 8;
const MAX_BODY: usize = 2 + BITMAP;
const RESEND_MS: Millis = 50;
const TIMEOUT_MS: Millis = 300;

const TYPE_STATE: u8 = 1;
const TYPE_TAP: u8 = 2;
const TYPE_HELLO: u8 = 3;

pub(crate) enum Message {
    /// Number of positions and one bit per position, set if pressed.
    State { positions: u8, pressed: [u8; BITMAP] },
    Tap(u8),
    Hello,
}

fn send(link: &mut impl Link, kind: u8, payload: &[u8]) {
    let mut frame = [0u8; MAX_BODY + 3];
    let len = payload.len() + 1;
    frame[0] = START;
    frame[1] = len as u8;
    frame[2] = kind;
    frame[3..3 + payload.len()].copy_from_slice(payload);
    frame[2 + len] = crc8(&frame[1..2 + len]);
    link.send(&frame[..3 + len]);
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

/// Finds frames in a byte stream and skips anything damaged.
struct Parser {
    buf: [u8; MAX_BODY + 2],
    /// Bytes collected after the start byte; `None` while hunting for one.
    filled: Option<usize>,
}

impl Parser {
    const fn new() -> Self {
        Self { buf: [0; MAX_BODY + 2], filled: None }
    }

    fn push(&mut self, byte: u8) -> Option<Message> {
        let Some(filled) = self.filled else {
            if byte == START {
                self.filled = Some(0);
            }
            return None;
        };
        self.buf[filled] = byte;
        let len = self.buf[0] as usize;
        if len == 0 || len > MAX_BODY {
            self.filled = None;
            return None;
        }
        // Length byte, body, CRC.
        if filled + 1 < len + 2 {
            self.filled = Some(filled + 1);
            return None;
        }
        self.filled = None;
        if crc8(&self.buf[..=len]) != self.buf[len + 1] {
            return None;
        }
        let payload = &self.buf[2..=len];
        match (self.buf[1], payload) {
            (TYPE_STATE, [positions, bits @ ..]) if bits.len() <= BITMAP => {
                let mut pressed = [0; BITMAP];
                pressed[..bits.len()].copy_from_slice(bits);
                Some(Message::State { positions: *positions, pressed })
            }
            (TYPE_TAP, [pos]) => Some(Message::Tap(*pos)),
            (TYPE_HELLO, []) => Some(Message::Hello),
            _ => None,
        }
    }
}

/// What the other half caused, in its own position numbers.
pub(crate) enum Remote {
    Key { pos: u8, pressed: bool },
    Tap(u8),
}

pub(crate) struct Split {
    parser: Parser,
    /// Peripheral: own key state. Central: the other half's.
    pressed: [u8; BITMAP],
    remote_positions: u8,
    last_heard: Option<Millis>,
    last_sent: Option<Millis>,
    dirty: bool,
}

impl Split {
    pub const fn new() -> Self {
        Self {
            parser: Parser::new(),
            pressed: [0; BITMAP],
            remote_positions: 0,
            last_heard: None,
            last_sent: None,
            dirty: false,
        }
    }

    pub fn connected(&self, now: Millis) -> bool {
        self.last_heard.is_some_and(|at| now - at < TIMEOUT_MS)
    }

    pub fn remote_positions(&self) -> u8 {
        self.remote_positions
    }

    /// Peripheral: note a key change to send on the next tick.
    pub fn set_local(&mut self, pos: u16, pressed: bool) {
        let Some(byte) = self.pressed.get_mut(pos as usize / 8) else { return };
        let bit = 1 << (pos % 8);
        *byte = if pressed { *byte | bit } else { *byte & !bit };
        self.dirty = true;
    }

    /// Peripheral: send an encoder click.
    pub fn send_tap(&mut self, link: &mut impl Link, pos: u16) {
        send(link, TYPE_TAP, &[pos as u8]);
    }

    /// Peripheral: send the key state if it changed or is due.
    pub fn peripheral_tick(&mut self, now: Millis, link: &mut impl Link, positions: usize) {
        self.listen(now, link, |_| {});
        if self.dirty || self.last_sent.is_none_or(|at| now - at >= RESEND_MS) {
            let bytes = positions.div_ceil(8).min(BITMAP);
            let mut payload = [0u8; 1 + BITMAP];
            payload[0] = positions as u8;
            payload[1..=bytes].copy_from_slice(&self.pressed[..bytes]);
            send(link, TYPE_STATE, &payload[..=bytes]);
            self.dirty = false;
            self.last_sent = Some(now);
        }
    }

    /// Central: take in what the other half sent and report what changed.
    pub fn central_tick(&mut self, now: Millis, link: &mut impl Link, mut on_remote: impl FnMut(Remote)) {
        let was_connected = self.last_heard.is_some();
        let mut state = None;
        self.listen(now, link, |message| match message {
            Message::State { positions, pressed } => state = Some((positions, pressed)),
            Message::Tap(pos) => on_remote(Remote::Tap(pos)),
            Message::Hello => {}
        });
        if let Some((positions, pressed)) = state {
            self.remote_positions = positions;
            self.apply(pressed, &mut on_remote);
        }
        if was_connected && !self.connected(now) {
            // The other half went away: let go of everything it held.
            self.last_heard = None;
            self.apply([0; BITMAP], &mut on_remote);
        }
        if self.last_sent.is_none_or(|at| now - at >= RESEND_MS) {
            send(link, TYPE_HELLO, &[]);
            self.last_sent = Some(now);
        }
    }

    fn listen(&mut self, now: Millis, link: &mut impl Link, mut on_message: impl FnMut(Message)) {
        let mut buf = [0u8; 32];
        loop {
            let len = link.receive(&mut buf);
            for byte in &buf[..len] {
                if let Some(message) = self.parser.push(*byte) {
                    self.last_heard = Some(now);
                    on_message(message);
                }
            }
            if len < buf.len() {
                break;
            }
        }
    }

    fn apply(&mut self, pressed: [u8; BITMAP], on_remote: &mut impl FnMut(Remote)) {
        for pos in 0..MAX_KEYS {
            let bit = |bits: &[u8; BITMAP]| bits[pos / 8] >> (pos % 8) & 1 == 1;
            if bit(&pressed) != bit(&self.pressed) {
                on_remote(Remote::Key { pos: pos as u8, pressed: bit(&pressed) });
            }
        }
        self.pressed = pressed;
    }
}
