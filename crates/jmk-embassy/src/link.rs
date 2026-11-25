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
        let _ = self.incoming.try_write(bytes);
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
        // again shortly anyway.
        let _ = self.outgoing.try_write(bytes);
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
                let _ = queues.incoming.try_write(&buf[..len]);
            }
        }
    };
    join(send, receive).await;
}
