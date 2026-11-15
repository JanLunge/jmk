use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use jmk_device::{Link, Pins, Pull};
use jmk_proto::{LinkConfig, Pin};

/// Something soldered between pins. Each wire is a switch that can be
/// closed or open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Ground(Pin),
    Supply(Pin),
    Plain(Pin, Pin),
    /// Conducts from anode to cathode.
    Diode { anode: Pin, cathode: Pin },
}

#[derive(Clone, Copy, PartialEq)]
enum PinMode {
    Untouched,
    Input(Pull),
    Low,
}

/// The pins of a simulated chip and what is wired to them. Clones share
/// the same hardware.
#[derive(Clone)]
pub struct SimPins(Rc<RefCell<State>>);

struct State {
    usable: u64,
    modes: [PinMode; 64],
    switches: Vec<(Wire, bool)>,
    settles: usize,
}

impl SimPins {
    /// A chip with GPIO 0..=29, like the RP2040.
    pub fn new(wires: &[Wire]) -> Self {
        let switches = wires.iter().map(|w| (*w, false)).collect();
        SimPins(Rc::new(RefCell::new(State {
            usable: 0x3FFF_FFFF,
            modes: [PinMode::Untouched; 64],
            switches,
            settles: 0,
        })))
    }

    /// Close or open a wire.
    pub fn set(&self, wire: usize, closed: bool) {
        if let Some(switch) = self.0.borrow_mut().switches.get_mut(wire) {
            switch.1 = closed;
        }
    }

    /// How often the firmware waited for levels to settle.
    pub fn settles(&self) -> usize {
        self.0.borrow().settles
    }

    /// Pins the firmware has configured at least once.
    pub fn touched(&self) -> Vec<Pin> {
        let state = self.0.borrow();
        (0..64).filter(|p| state.modes[*p as usize] != PinMode::Untouched).collect()
    }
}

impl Pins for SimPins {
    fn usable(&self) -> u64 {
        self.0.borrow().usable
    }

    fn input(&mut self, pin: Pin, pull: Pull) {
        self.0.borrow_mut().modes[pin as usize] = PinMode::Input(pull);
    }

    fn drive_low(&mut self, pin: Pin) {
        self.0.borrow_mut().modes[pin as usize] = PinMode::Low;
    }

    fn read(&mut self, pin: Pin) -> bool {
        let state = self.0.borrow();
        let closed = || state.switches.iter().filter(|s| s.1).map(|s| s.0);
        let mut low = [false; 64];
        for (pin, mode) in state.modes.iter().enumerate() {
            low[pin] = *mode == PinMode::Low;
        }
        for wire in closed() {
            if let Wire::Ground(pin) = wire {
                low[pin as usize] = true;
            }
        }
        // Spread low levels through closed switches until nothing changes.
        loop {
            let before = low;
            for wire in closed() {
                match wire {
                    Wire::Plain(a, b) => {
                        let either = low[a as usize] || low[b as usize];
                        low[a as usize] = either;
                        low[b as usize] = either;
                    }
                    Wire::Diode { anode, cathode } => low[anode as usize] |= low[cathode as usize],
                    _ => {}
                }
            }
            if low == before {
                break;
            }
        }
        let supplied = closed().any(|w| matches!(w, Wire::Supply(p) if p == pin));
        !low[pin as usize] && (supplied || state.modes[pin as usize] != PinMode::Input(Pull::Down))
    }

    fn supports_link(&self, link: &LinkConfig) -> bool {
        matches!(link, LinkConfig::Uart { .. })
    }

    fn settle(&mut self) {
        self.0.borrow_mut().settles += 1;
    }
}

/// Flash memory in RAM. Clones share the same contents, so a "reboot" can
/// hand the same flash to a new device.
#[derive(Clone)]
pub struct MemFlash {
    bytes: Rc<RefCell<Vec<u8>>>,
    writes_left: Rc<RefCell<Option<usize>>>,
}

impl MemFlash {
    pub fn new(size: usize) -> Self {
        Self { bytes: Rc::new(RefCell::new(vec![0xFF; size])), writes_left: Default::default() }
    }

    /// Let this many writes succeed, then fail each one half way through,
    /// like a power cut. `None` restores normal operation.
    pub fn fail_after_writes(&self, writes: Option<usize>) {
        *self.writes_left.borrow_mut() = writes;
    }
}

#[derive(Debug)]
pub struct PowerCut;

impl NorFlashError for PowerCut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

impl ErrorType for MemFlash {
    type Error = PowerCut;
}

impl ReadNorFlash for MemFlash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), PowerCut> {
        let offset = offset as usize;
        bytes.copy_from_slice(&self.bytes.borrow()[offset..offset + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.bytes.borrow().len()
    }
}

impl NorFlash for MemFlash {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = 4096;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), PowerCut> {
        assert!(from as usize % Self::ERASE_SIZE == 0 && to as usize % Self::ERASE_SIZE == 0);
        self.bytes.borrow_mut()[from as usize..to as usize].fill(0xFF);
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), PowerCut> {
        assert!(offset % 4 == 0 && bytes.len() % 4 == 0);
        let mut left = self.writes_left.borrow_mut();
        let cut = left.is_some_and(|n| n == 0);
        if let Some(n) = left.as_mut() {
            *n = n.saturating_sub(1);
        }
        let len = if cut { bytes.len() / 2 } else { bytes.len() };
        let mut flash = self.bytes.borrow_mut();
        for (cell, byte) in flash[offset as usize..].iter_mut().zip(&bytes[..len]) {
            *cell &= *byte;
        }
        if cut {
            Err(PowerCut)
        } else {
            Ok(())
        }
    }
}

/// One end of a cable between two halves.
#[derive(Clone)]
pub struct Pipe {
    outgoing: Rc<RefCell<VecDeque<u8>>>,
    incoming: Rc<RefCell<VecDeque<u8>>>,
    /// Shared by both ends: false while the cable is unplugged.
    plugged: Rc<RefCell<bool>>,
}

impl Pipe {
    /// A cable; returns its two ends.
    pub fn pair() -> (Pipe, Pipe) {
        let (a, b): (Rc<RefCell<VecDeque<u8>>>, Rc<RefCell<VecDeque<u8>>>) = Default::default();
        let plugged = Rc::new(RefCell::new(true));
        (
            Pipe { outgoing: a.clone(), incoming: b.clone(), plugged: plugged.clone() },
            Pipe { outgoing: b, incoming: a, plugged },
        )
    }

    /// Put bytes on the wire towards this end, as interference would.
    pub fn inject(&self, bytes: &[u8]) {
        self.incoming.borrow_mut().extend(bytes);
    }

    pub fn set_plugged(&self, plugged: bool) {
        *self.plugged.borrow_mut() = plugged;
    }
}

impl Link for Pipe {
    fn send(&mut self, bytes: &[u8]) {
        if *self.plugged.borrow() {
            self.outgoing.borrow_mut().extend(bytes);
        }
    }

    fn receive(&mut self, buf: &mut [u8]) -> usize {
        let mut incoming = self.incoming.borrow_mut();
        let len = buf.len().min(incoming.len());
        for (slot, byte) in buf.iter_mut().zip(incoming.drain(..len)) {
            *slot = byte;
        }
        len
    }
}
