//! The serial cable between two halves of a split keyboard, without the
//! UART: a port opens its UART on the configured pins and hands both
//! directions to [`run`].

use embassy_futures::join::join;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::pipe::Pipe;
use embedded_io_async::{Read, Write};
use jmk_device::Link;

pub const BAUD: u32 = 115_200;

type Bytes = Pipe<NoopRawMutex, 256>;

/// The firmware's end: bytes queued here are moved by [`run`].
pub struct Queues {
    outgoing: Bytes,
    incoming: Bytes,
}

impl Queues {
    pub const fn new() -> Self {
        Self { outgoing: Pipe::new(), incoming: Pipe::new() }
    }
}

impl Queues {
    /// For a port that moves the bytes itself, as a cable of one wire
    /// needs: wait for bytes the firmware wants sent.
    pub async fn outgoing(&self, buf: &mut [u8]) -> usize {
        self.outgoing.read(buf).await
    }

    /// Hand bytes that arrived to the firmware. Dropped if it is not taking
    /// any; the other half repeats its state.
    pub fn deliver(&self, bytes: &[u8]) {
        put(&self.incoming, bytes);
    }
}

/// Write what fits. One `try_write` only fills the pipe up to the end of its
/// ring buffer, so it takes a second one for the part that wraps around to
/// the start.
fn put(pipe: &Bytes, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match pipe.try_write(bytes) {
            Ok(written) => bytes = &bytes[written..],
            Err(_) => break,
        }
    }
}

impl Default for Queues {
    fn default() -> Self {
        Self::new()
    }
}

impl Link for &Queues {
    fn send(&mut self, bytes: &[u8]) {
        // A full queue means the cable is not draining; the state is sent
        // again shortly anyway. Each call is one whole message: queue all of
        // it or none, as a message cut short garbles the one after it too.
        if self.outgoing.free_capacity() >= bytes.len() {
            put(&self.outgoing, bytes);
        }
    }

    fn receive(&mut self, buf: &mut [u8]) -> usize {
        self.incoming.try_read(buf).unwrap_or(0)
    }
}

/// Move bytes between the cable and the queues, forever.
pub async fn run(mut tx: impl Write, mut rx: impl Read, queues: &Queues) {
    let send = async {
        let mut buf = [0u8; 64];
        loop {
            let len = queues.outgoing.read(&mut buf).await;
            let _ = tx.write_all(&buf[..len]).await;
        }
    };
    let receive = async {
        let mut buf = [0u8; 64];
        loop {
            if let Ok(len) = rx.read(&mut buf).await {
                // If the firmware is not taking bytes, drop them; the
                // other half repeats its state.
                queues.deliver(&buf[..len]);
            }
        }
    };
    join(send, receive).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Messages that wrap around the end of the send buffer arrive whole.
    #[test]
    fn messages_across_the_end_of_the_buffer_are_sent_whole() {
        let queues = Queues::new();
        let mut link = &queues;
        let mut sent = Vec::new();
        let mut buf = [0u8; 64];
        // 23 bytes does not divide a power of two, so messages land across
        // the end. Enough of them to go round the buffer a few times.
        let rounds = 3 * queues.outgoing.capacity() / 23;
        for round in 0..rounds {
            link.send(&[round as u8; 23]);
            while let Ok(len) = queues.outgoing.try_read(&mut buf) {
                sent.extend_from_slice(&buf[..len]);
            }
        }
        let expected: Vec<u8> = (0..rounds).flat_map(|round| [round as u8; 23]).collect();
        assert_eq!(sent, expected);
    }

    /// Without room for all of a message, none of it is queued.
    #[test]
    fn a_message_that_does_not_fit_is_dropped_whole() {
        let queues = Queues::new();
        let mut link = &queues;
        while queues.outgoing.free_capacity() >= 23 {
            link.send(&[1; 23]);
        }
        let taken = queues.outgoing.len();
        assert!(taken < queues.outgoing.capacity(), "some room is left, only not enough");
        link.send(&[2; 23]);
        assert_eq!(queues.outgoing.len(), taken);
    }
}
