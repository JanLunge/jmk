//! Requests for the other half of a split, over the cable between them.
//!
//! A half on USB passes a request for the other half on as it came: in the
//! bytes the other half would have got over USB. The other half handles it
//! like one of its own and sends back the bytes it would have sent over USB,
//! which reach the computer as [`jmk_proto::Event::Peer`]. While asked this
//! way to watch keys, it sends its key events the same way. The computer can
//! so set up a half that has no USB cable of its own.
//!
//! A message travels in parts of [`PART`] bytes, paced so that they do not
//! crowd the cable, which also carries the keys. Parts get lost when both
//! halves talk at once on a cable of one wire, so the asking half asks again
//! when the answer stops coming. The answering half recognises a request it
//! has already handled by its number and sends the same answer again rather
//! than doing it twice: a request may add steps to a macro or save.

use heapless::Vec;
use jmk_core::Millis;
use jmk_proto::MAX_PEER_MESSAGE;

use crate::link::{Link, Split, CHUNK};

/// Bytes of a message in one part.
pub(crate) const PART: usize = CHUNK;
const MAX_PARTS: usize = MAX_PEER_MESSAGE.div_ceil(PART);
const _: () = assert!(MAX_PARTS <= 32, "parts received are kept as bits of a u32");
/// The time between two parts sent: a little longer than one part takes on
/// the slowest cable, so the cable's queue never fills up.
const GAP_MS: Millis = 8;
/// Ask again when nothing of the answer came for this long.
const RETRY_MS: Millis = 120;
/// Give up on an answer after this long.
const GIVE_UP_MS: Millis = 1500;
/// How long an answer is kept for a request that comes again.
const KEEP_MS: Millis = 3000;
/// Events waiting to be sent; more are dropped.
const MAX_EVENTS: usize = 16;

pub(crate) type Message = Vec<u8, MAX_PEER_MESSAGE>;

/// A piece of a message. `ask` tells a request from what comes back.
#[derive(Clone, Copy)]
pub(crate) struct Part {
    pub ask: bool,
    /// Numbers a request and its answer. 0 is for events, which need no
    /// request and fit into one part.
    pub id: u8,
    pub index: u8,
    pub count: u8,
    pub len: u8,
    pub data: [u8; PART],
}

/// A message on its way out.
struct Outgoing {
    id: u8,
    bytes: Message,
    /// The part to send next; all are sent once it reaches the count.
    next: usize,
}

impl Outgoing {
    fn count(&self) -> usize {
        self.bytes.len().div_ceil(PART).max(1)
    }

    fn done(&self) -> bool {
        self.next >= self.count()
    }

    fn part(&self, ask: bool) -> Part {
        let start = self.next * PART;
        let bytes = &self.bytes[start..(start + PART).min(self.bytes.len())];
        let mut data = [0; PART];
        data[..bytes.len()].copy_from_slice(bytes);
        Part { ask, id: self.id, index: self.next as u8, count: self.count() as u8, len: bytes.len() as u8, data }
    }
}

/// A message coming in part by part, in any order.
struct Incoming {
    id: u8,
    count: u8,
    /// One bit per part that arrived.
    got: u32,
    len: usize,
    buf: [u8; MAX_PEER_MESSAGE],
}

impl Incoming {
    const fn new() -> Self {
        Self { id: 0, count: 0, got: 0, len: 0, buf: [0; MAX_PEER_MESSAGE] }
    }

    /// Take a part. Returns the message once its last part is in.
    fn push(&mut self, part: &Part) -> Option<&[u8]> {
        let (index, count, len) = (part.index as usize, part.count as usize, part.len as usize);
        // Every part but the last is full.
        if count > MAX_PARTS || (index + 1 < count && len != PART) {
            return None;
        }
        if part.id != self.id || part.count != self.count {
            (self.id, self.count, self.got, self.len) = (part.id, part.count, 0, 0);
        }
        self.buf[index * PART..index * PART + len].copy_from_slice(&part.data[..len]);
        if index + 1 == count {
            self.len = index * PART + len;
        }
        self.got |= 1 << index;
        if self.got != (1u32 << count) - 1 {
            return None;
        }
        self.got = 0;
        // The same parts again make the same message again.
        self.count = 0;
        Some(&self.buf[..self.len])
    }
}

/// A request this half sent to the other one.
struct Asking {
    out: Outgoing,
    started: Millis,
    /// When a part of it was last sent or of its answer last arrived.
    active: Millis,
}

/// What came in over the cable on one tick.
pub(crate) enum Taken {
    Nothing,
    /// A request for this half, with its number.
    Request(u8, Message),
    /// The answer to this half's request, or an event of the other half.
    Message(Message),
}

pub(crate) struct Peer {
    next_id: u8,
    asking: Option<Asking>,
    answer_in: Incoming,
    request_in: Incoming,
    /// The answer to the last request from the other half, kept for when
    /// it asks again, and since when.
    answer: Option<(Outgoing, Millis)>,
    events: heapless::Deque<Message, MAX_EVENTS>,
    last_sent: Option<Millis>,
}

impl Peer {
    pub const fn new() -> Self {
        Self {
            next_id: 0,
            asking: None,
            answer_in: Incoming::new(),
            request_in: Incoming::new(),
            answer: None,
            events: heapless::Deque::new(),
            last_sent: None,
        }
    }

    /// Send a request to the other half, instead of one still unanswered.
    pub fn ask(&mut self, now: Millis, bytes: Message) {
        if self.next_id == 0 {
            // A fresh start begins somewhere else every time, so that a half
            // that restarted is not answered from the other half's memory.
            self.next_id = (now as u8).max(1);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.asking = Some(Asking { out: Outgoing { id, bytes, next: 0 }, started: now, active: now });
    }

    /// Take a part that arrived.
    pub fn take(&mut self, now: Millis, part: &Part) -> Taken {
        if part.ask {
            if let Some((answer, since)) = &mut self.answer {
                if answer.id == part.id && now - *since < KEEP_MS {
                    // Asked again: the answer got lost. Send it once more
                    // when the whole request is in.
                    if self.request_in.push(part).is_some() {
                        answer.next = 0;
                    }
                    return Taken::Nothing;
                }
            }
            return match self.request_in.push(part) {
                Some(bytes) => Taken::Request(part.id, bytes.iter().copied().collect()),
                None => Taken::Nothing,
            };
        }
        if part.id == 0 {
            return if part.count == 1 { Taken::Message(part.data[..part.len as usize].iter().copied().collect()) } else { Taken::Nothing };
        }
        let Some(asking) = &mut self.asking else { return Taken::Nothing };
        if asking.out.id != part.id {
            return Taken::Nothing;
        }
        asking.active = now;
        match self.answer_in.push(part) {
            Some(bytes) => {
                let message = bytes.iter().copied().collect();
                self.asking = None;
                Taken::Message(message)
            }
            None => Taken::Nothing,
        }
    }

    /// Send back the answer to request `id`.
    pub fn answer(&mut self, now: Millis, id: u8, bytes: Message) {
        self.answer = Some((Outgoing { id, bytes, next: 0 }, now));
    }

    /// Send an event of this half; one longer than a part is dropped.
    pub fn event(&mut self, bytes: &[u8]) {
        if bytes.len() <= PART {
            let _ = self.events.push_back(bytes.iter().copied().collect());
        }
    }

    /// Whether the request in flight was given up on. It also is, at once,
    /// when the other half is not there.
    pub fn given_up(&mut self, now: Millis, connected: bool) -> bool {
        let Some(asking) = &self.asking else { return false };
        if connected && now - asking.started < GIVE_UP_MS {
            return false;
        }
        self.asking = None;
        true
    }

    /// Forget events nobody will receive.
    pub fn forget_events(&mut self) {
        self.events.clear();
    }

    /// Send the next part, if it is time for one.
    pub fn send(&mut self, now: Millis, split: &mut Split, link: &mut impl Link) {
        if self.last_sent.is_some_and(|at| now - at < GAP_MS) {
            return;
        }
        if let Some(asking) = &mut self.asking {
            if asking.out.done() && now - asking.active >= RETRY_MS {
                asking.out.next = 0;
            }
        }
        let part = if let Some(event) = self.events.pop_front() {
            let mut data = [0; PART];
            data[..event.len()].copy_from_slice(&event);
            Part { ask: false, id: 0, index: 0, count: 1, len: event.len() as u8, data }
        } else if let Some((answer, _)) = self.answer.as_mut().filter(|(answer, _)| !answer.done()) {
            let part = answer.part(false);
            answer.next += 1;
            part
        } else if let Some(asking) = self.asking.as_mut().filter(|asking| !asking.out.done()) {
            let part = asking.out.part(true);
            asking.out.next += 1;
            asking.active = now;
            part
        } else {
            return;
        };
        split.send_part(link, &part);
        self.last_sent = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(id: u8, bytes: &[u8]) -> std::vec::Vec<Part> {
        let mut out = Outgoing { id, bytes: bytes.iter().copied().collect(), next: 0 };
        let mut parts = std::vec::Vec::new();
        while !out.done() {
            parts.push(out.part(true));
            out.next += 1;
        }
        parts
    }

    #[test]
    fn a_message_comes_together_in_any_order() {
        let bytes: std::vec::Vec<u8> = (0..60).collect();
        let mut sent = parts(5, &bytes);
        assert_eq!(sent.len(), 3);
        sent.swap(0, 2);
        let mut incoming = Incoming::new();
        assert!(incoming.push(&sent[0]).is_none());
        assert!(incoming.push(&sent[1]).is_none());
        assert_eq!(incoming.push(&sent[2]), Some(&bytes[..]));
        // The same parts again make the message again.
        for part in &sent[..2] {
            assert!(incoming.push(part).is_none());
        }
        assert_eq!(incoming.push(&sent[2]), Some(&bytes[..]));
    }

    #[test]
    fn parts_of_another_request_start_over() {
        let mut incoming = Incoming::new();
        let old = parts(1, &[7; 30]);
        let new = parts(2, &[9; 30]);
        assert!(incoming.push(&old[0]).is_none());
        assert!(incoming.push(&new[1]).is_none());
        assert!(incoming.push(&old[1]).is_none());
        assert!(incoming.push(&new[0]).is_none());
        assert!(incoming.push(&new[1]).is_some());
    }
}
