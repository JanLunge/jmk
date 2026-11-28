//! The connection between the two halves of a split keyboard.
//!
//! Both halves do the same thing. Each sends the complete state of its own
//! keys whenever it changes and a few times a second regardless, so a lost
//! message corrects itself and no key can stay stuck. With it goes whether a
//! computer is listening to that half, which settles who types: see
//! [`Split::leading`]. Encoder clicks have no state and are sent once. Twice
//! a second each half also says which chip it is and what keymap it holds,
//! and a half whose keymap is the older one asks for the newer one piece by
//! piece: see [`crate::sync`]. The half that types also sends its clock, by
//! which the lights of both move, and what its indicator lights go by.
//! Requests from the computer for the other half and what it answers travel
//! in parts as well: see [`crate::peer`].
//!
//! Frame: 0x4A, length, type, payload, CRC-8 over length, type and payload.

use jmk_core::Millis;
use jmk_proto::MAX_KEYS;

use crate::peer::{Part, PART};
use crate::sync::Side;

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
/// Bytes of keymap in one message.
pub(crate) const CHUNK: usize = 24;
/// Type and payload of the longest message.
const MAX_BODY: usize = 5 + CHUNK;
const _: () = assert!(MAX_BODY >= 3 + BITMAP);
const RESEND_MS: Millis = 50;
const ABOUT_MS: Millis = 500;
const TIMEOUT_MS: Millis = 300;
/// How long a half that has just started waits to hear the other one before
/// it types on its own. Several resends long, and far shorter than anyone
/// takes to press a key after plugging in or changing the setup.
const SETTLE_MS: Millis = 200;

const TYPE_STATE: u8 = 1;
const TYPE_TAP: u8 = 2;
const TYPE_ABOUT: u8 = 4;
const TYPE_FETCH: u8 = 5;
const TYPE_CHUNK: u8 = 6;
const TYPE_CLOCK: u8 = 7;
const TYPE_ASK: u8 = 8;
const TYPE_ANSWER: u8 = 9;
const _: () = assert!(MAX_BODY >= 4 + PART);

const FLAG_CLEAN: u8 = 1;
const FLAG_FORCE: u8 = 2;
const FLAG_BASE: u8 = 4;

/// A piece of the other half's keymap.
#[derive(Clone, Copy)]
pub(crate) struct Chunk {
    pub offset: u16,
    /// Length of the whole.
    pub total: u16,
    pub len: u8,
    pub data: [u8; CHUNK],
}

pub(crate) enum Message {
    /// Whether a computer listens to the sender, its number of positions and
    /// one bit per position, set if pressed.
    State { host: bool, positions: u8, pressed: [u8; BITMAP] },
    Tap(u8),
    About { uid: [u8; 8], side: Side },
    /// Send me the keymap with this digest from this offset on.
    Fetch { digest: u32, offset: u16 },
    Chunk(Chunk),
    /// The sender's time in milliseconds, its active layers and lock keys.
    Clock(u32, (u8, u8)),
    Part(Part),
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
            (TYPE_STATE, [flags, positions, bits @ ..]) if bits.len() <= BITMAP => {
                let mut pressed = [0; BITMAP];
                pressed[..bits.len()].copy_from_slice(bits);
                Some(Message::State { host: flags & 1 != 0, positions: *positions, pressed })
            }
            (TYPE_TAP, [pos]) => Some(Message::Tap(*pos)),
            (TYPE_ABOUT, about) if about.len() == 21 => {
                let word = |at: usize| u32::from_le_bytes([about[at], about[at + 1], about[at + 2], about[at + 3]]);
                let mut uid = [0; 8];
                uid.copy_from_slice(&about[..8]);
                let flags = about[20];
                let side = Side {
                    digest: word(8),
                    base: (flags & FLAG_BASE != 0).then(|| word(12)),
                    revision: word(16),
                    clean: flags & FLAG_CLEAN != 0,
                    force: flags & FLAG_FORCE != 0,
                };
                Some(Message::About { uid, side })
            }
            (TYPE_FETCH, [a, b, c, d, lo, hi]) => {
                Some(Message::Fetch { digest: u32::from_le_bytes([*a, *b, *c, *d]), offset: u16::from_le_bytes([*lo, *hi]) })
            }
            (TYPE_CHUNK, [o0, o1, t0, t1, bytes @ ..]) if bytes.len() <= CHUNK => {
                let mut data = [0; CHUNK];
                data[..bytes.len()].copy_from_slice(bytes);
                Some(Message::Chunk(Chunk {
                    offset: u16::from_le_bytes([*o0, *o1]),
                    total: u16::from_le_bytes([*t0, *t1]),
                    len: bytes.len() as u8,
                    data,
                }))
            }
            (TYPE_CLOCK, [a, b, c, d, layers, locks, ..]) => {
                Some(Message::Clock(u32::from_le_bytes([*a, *b, *c, *d]), (*layers, *locks)))
            }
            (kind @ (TYPE_ASK | TYPE_ANSWER), [id, index, count, bytes @ ..])
                if !bytes.is_empty() && bytes.len() <= PART && index < count =>
            {
                let mut data = [0; PART];
                data[..bytes.len()].copy_from_slice(bytes);
                Some(Message::Part(Part {
                    ask: kind == TYPE_ASK,
                    id: *id,
                    index: *index,
                    count: *count,
                    len: bytes.len() as u8,
                    data,
                }))
            }
            _ => None,
        }
    }
}

/// What the other half caused, in its own position numbers.
pub(crate) enum Remote {
    Key { pos: u8, pressed: bool },
    Tap(u8),
}

/// What this half tells the other about itself.
#[derive(Clone, Copy)]
pub(crate) struct Own {
    /// A computer is listening to this half.
    pub host: bool,
    /// This is the first half, which types when both could.
    pub first: bool,
    pub positions: usize,
    pub uid: [u8; 8],
    pub about: Side,
    /// Active layers and lock keys.
    pub shows: (u8, u8),
}

/// What the other half asked for or sent of its keymap on one tick.
#[derive(Default)]
pub(crate) struct Post {
    /// It wants the keymap with this digest from this offset on.
    pub fetch: Option<(u32, u16)>,
    pub chunk: Option<Chunk>,
    /// Parts of requests for this half, or of what the other half sent back.
    pub parts: heapless::Vec<Part, 8>,
}

pub(crate) struct Split {
    parser: Parser,
    /// This half's keys, as last sent or about to be.
    local: [u8; BITMAP],
    /// The other half's keys as far as they were passed on to the keymap.
    remote: [u8; BITMAP],
    remote_positions: u8,
    peer_host: bool,
    /// The other half has said whether a computer listens to it.
    heard_state: bool,
    /// When this half started exchanging messages.
    since: Option<Millis>,
    peer_uid: [u8; 8],
    peer: Option<Side>,
    last_heard: Option<Millis>,
    last_sent: Option<Millis>,
    last_about: Option<Millis>,
    sent_host: bool,
    sent_about: Option<Side>,
    dirty: bool,
    /// How far the typing half's clock is ahead of this one's.
    ahead: i64,
    /// The typing half's layers and lock keys, as heard and as last sent.
    peer_shows: (u8, u8),
    sent_shows: (u8, u8),
}

impl Split {
    pub const fn new() -> Self {
        Self {
            parser: Parser::new(),
            local: [0; BITMAP],
            remote: [0; BITMAP],
            remote_positions: 0,
            peer_host: false,
            heard_state: false,
            since: None,
            peer_uid: [0; 8],
            peer: None,
            last_heard: None,
            last_sent: None,
            last_about: None,
            sent_host: false,
            sent_about: None,
            dirty: false,
            ahead: 0,
            peer_shows: (1, 0),
            sent_shows: (1, 0),
        }
    }

    pub fn connected(&self, now: Millis) -> bool {
        self.last_heard.is_some_and(|at| now - at < TIMEOUT_MS)
    }

    pub fn remote_positions(&self) -> u8 {
        self.remote_positions
    }

    pub fn peer_uid(&self) -> [u8; 8] {
        self.peer_uid
    }

    /// What the other half said about its keymap, while it is connected.
    pub fn peer(&self, now: Millis) -> Option<Side> {
        self.peer.filter(|_| self.connected(now))
    }

    /// Ask the other half for a piece of its keymap.
    pub fn send_fetch(&mut self, link: &mut impl Link, digest: u32, offset: u16) {
        let mut payload = [0u8; 6];
        payload[..4].copy_from_slice(&digest.to_le_bytes());
        payload[4..].copy_from_slice(&offset.to_le_bytes());
        send(link, TYPE_FETCH, &payload);
    }

    pub fn send_chunk(&mut self, link: &mut impl Link, offset: u16, total: u16, data: &[u8]) {
        let mut payload = [0u8; 4 + CHUNK];
        payload[..2].copy_from_slice(&offset.to_le_bytes());
        payload[2..4].copy_from_slice(&total.to_le_bytes());
        payload[4..4 + data.len()].copy_from_slice(data);
        send(link, TYPE_CHUNK, &payload[..4 + data.len()]);
    }

    /// The time the lights go by: the typing half's own, and on the other
    /// half the same as far as it was told.
    pub fn clock(&self, now: Millis, leading: bool) -> Millis {
        if leading {
            now
        } else {
            (now as i64 + self.ahead) as Millis
        }
    }

    /// The typing half's active layers and lock keys.
    pub fn shows(&self) -> (u8, u8) {
        self.peer_shows
    }

    /// Send a part of a request for the other half or of an answer to one.
    pub fn send_part(&mut self, link: &mut impl Link, part: &Part) {
        let mut payload = [0u8; 3 + PART];
        payload[..3].copy_from_slice(&[part.id, part.index, part.count]);
        let len = part.len as usize;
        payload[3..3 + len].copy_from_slice(&part.data[..len]);
        send(link, if part.ask { TYPE_ASK } else { TYPE_ANSWER }, &payload[..3 + len]);
    }

    /// Whether this half does the typing. A half on its own always does.
    /// Of two that hear each other it is the one a computer listens to,
    /// and the first half if that does not decide it, so that two halves
    /// plugged into the same computer never both type.
    pub fn leading(&self, now: Millis, host: bool, first: bool) -> bool {
        if !self.connected(now) || !self.heard_state {
            // Fresh from a start the other half may be there and simply not
            // heard yet: typing now could type its keys a second time.
            return self.since.is_some_and(|at| now - at >= SETTLE_MS);
        }
        if host != self.peer_host {
            host
        } else {
            first
        }
    }

    /// Note a change of one of this half's keys, to send on the next tick.
    pub fn set_local(&mut self, pos: u16, pressed: bool) {
        let Some(byte) = self.local.get_mut(pos as usize / 8) else { return };
        let bit = 1 << (pos % 8);
        *byte = if pressed { *byte | bit } else { *byte & !bit };
        self.dirty = true;
    }

    /// Send an encoder click.
    pub fn send_tap(&mut self, link: &mut impl Link, pos: u16) {
        send(link, TYPE_TAP, &[pos as u8]);
    }

    /// Forget what the other half holds down. Whatever it still holds comes
    /// back as fresh presses with its next message.
    pub fn forget_remote(&mut self) {
        self.remote = [0; BITMAP];
    }

    /// Exchange messages with the other half. `on_remote` gets what changed
    /// over there, whichever half types.
    pub fn tick(&mut self, now: Millis, link: &mut impl Link, own: Own, mut on_remote: impl FnMut(Remote)) -> Post {
        let was_connected = self.last_heard.is_some();
        self.since.get_or_insert(now);
        let mut state = None;
        let mut taps: heapless::Vec<u8, 8> = heapless::Vec::new();
        let mut about = None;
        let mut clock = None;
        let mut post = Post::default();
        self.listen(now, link, |message| match message {
            Message::State { host, positions, pressed } => state = Some((host, positions, pressed)),
            Message::Tap(pos) => {
                let _ = taps.push(pos);
            }
            Message::About { uid, side } => about = Some((uid, side)),
            Message::Fetch { digest, offset } => post.fetch = Some((digest, offset)),
            Message::Chunk(chunk) => post.chunk = Some(chunk),
            Message::Clock(at, shows) => clock = Some((at, shows)),
            Message::Part(part) => {
                let _ = post.parts.push(part);
            }
        });
        if let Some((clock, shows)) = clock {
            self.ahead = clock as i64 - now as i64;
            self.peer_shows = shows;
        }
        if let Some((uid, side)) = about {
            self.peer_uid = uid;
            self.peer = Some(side);
        }
        if let Some((host, positions, _)) = state {
            self.peer_host = host;
            self.heard_state = true;
            self.remote_positions = positions;
        }
        if was_connected && !self.connected(now) {
            // The other half went away: let go of everything it held.
            self.last_heard = None;
            self.peer_host = false;
            self.heard_state = false;
            self.peer = None;
            self.apply([0; BITMAP], &mut on_remote);
        } else {
            if let Some((_, _, pressed)) = state {
                self.apply(pressed, &mut on_remote);
            }
            for pos in taps {
                on_remote(Remote::Tap(pos));
            }
        }
        if self.dirty || own.host != self.sent_host || self.last_sent.is_none_or(|at| now - at >= RESEND_MS) {
            let bytes = own.positions.div_ceil(8).min(BITMAP);
            let mut payload = [0u8; 2 + BITMAP];
            payload[0] = own.host as u8;
            payload[1] = own.positions as u8;
            payload[2..2 + bytes].copy_from_slice(&self.local[..bytes]);
            send(link, TYPE_STATE, &payload[..2 + bytes]);
            self.dirty = false;
            self.sent_host = own.host;
            self.last_sent = Some(now);
        }
        let about_due = self.sent_about != Some(own.about) || self.last_about.is_none_or(|at| now - at >= ABOUT_MS);
        if (about_due || own.shows != self.sent_shows) && self.leading(now, own.host, own.first) {
            let mut payload = [0u8; 6];
            payload[..4].copy_from_slice(&(now as u32).to_le_bytes());
            (payload[4], payload[5]) = own.shows;
            send(link, TYPE_CLOCK, &payload);
            self.sent_shows = own.shows;
        }
        if about_due {
            let about = own.about;
            let mut payload = [0u8; 21];
            payload[..8].copy_from_slice(&own.uid);
            payload[8..12].copy_from_slice(&about.digest.to_le_bytes());
            payload[12..16].copy_from_slice(&about.base.unwrap_or(0).to_le_bytes());
            payload[16..20].copy_from_slice(&about.revision.to_le_bytes());
            payload[20] = if about.clean { FLAG_CLEAN } else { 0 }
                | if about.force { FLAG_FORCE } else { 0 }
                | if about.base.is_some() { FLAG_BASE } else { 0 };
            send(link, TYPE_ABOUT, &payload);
            self.sent_about = Some(about);
            self.last_about = Some(now);
        }
        post
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
            if bit(&pressed) != bit(&self.remote) {
                on_remote(Remote::Key { pos: pos as u8, pressed: bit(&pressed) });
            }
        }
        self.remote = pressed;
    }
}
