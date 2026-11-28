use std::collections::VecDeque;

use jmk_core::{KeyboardReport, MouseReport, Output};
use jmk_device::{Board, Config, Device, Storage, STORAGE_SIZE};
use jmk_proto::frame::{self, Assembler, REPORT_SIZE};
use jmk_proto::{DeviceMessage, Error, Mcu, Pin, Request, Response};
use serde::{Deserialize, Serialize};

use crate::hardware::{MemFlash, Pipe, SimPins, Wire};

/// What the computer would receive from the keyboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostEvent {
    /// Modifier bits and the keys held down.
    Keyboard { mods: u8, keys: Vec<u8> },
    /// A media key went down, or all were released (0).
    Consumer(u16),
    /// Mouse buttons held and movement since the last one.
    Mouse { buttons: u8, x: i16, y: i16, wheel: i8, pan: i8 },
}

/// How a simulated keyboard is soldered together.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Wiring {
    /// One wire per key switch, in the order the builder would press them.
    pub switches: Vec<Wire>,
    /// Signal pins A and B of each rotary encoder (both switch to ground).
    pub encoders: Vec<(Pin, Pin)>,
}

impl Wiring {
    /// Keys wired straight from a pin to ground.
    pub fn direct(pins: &[Pin]) -> Self {
        Wiring { switches: pins.iter().map(|p| Wire::Ground(*p)).collect(), encoders: vec![] }
    }

    /// A diode matrix, keys in reading order. With `col_to_row` the diodes
    /// point from the column to the row, so rows must be driven.
    pub fn matrix(rows: &[Pin], cols: &[Pin], col_to_row: bool) -> Self {
        let switches = rows
            .iter()
            .flat_map(|row| cols.iter().map(move |col| (*row, *col)))
            .map(|(row, col)| match col_to_row {
                true => Wire::Diode { anode: col, cathode: row },
                false => Wire::Diode { anode: row, cathode: col },
            })
            .collect();
        Wiring { switches, encoders: vec![] }
    }

    fn wires(&self) -> Vec<Wire> {
        let encoder_signals = self.encoders.iter().flat_map(|(a, b)| [Wire::Ground(*a), Wire::Ground(*b)]);
        self.switches.iter().copied().chain(encoder_signals).collect()
    }
}

#[derive(Default)]
struct HostLog(Vec<HostEvent>);

impl Output for HostLog {
    fn keyboard(&mut self, report: &KeyboardReport) {
        self.0.push(HostEvent::Keyboard { mods: report.mods, keys: report.keys.to_vec() });
    }
    fn consumer(&mut self, usage: u16) {
        self.0.push(HostEvent::Consumer(usage));
    }
    fn mouse(&mut self, report: &MouseReport) {
        let MouseReport { buttons, x, y, wheel, pan } = *report;
        self.0.push(HostEvent::Mouse { buttons, x, y, wheel, pan });
    }
}

/// A keyboard that exists only in memory: simulated pins, flash and cable,
/// driven by the same code that runs on the microcontroller.
pub struct VirtualKeyboard {
    device: Device<'static, SimPins, MemFlash>,
    pins: SimPins,
    flash: MemFlash,
    wiring: Wiring,
    uid: [u8; 8],
    link: Option<Pipe>,
    now: u64,
    assembler: Assembler,
    to_host: VecDeque<[u8; REPORT_SIZE]>,
    host: HostLog,
    in_bootloader: bool,
    /// The USB cable to the computer is plugged in.
    usb: bool,
    /// Quadrature phase of each encoder, 0..4.
    encoder_phase: Vec<u8>,
}

impl VirtualKeyboard {
    pub fn new(wiring: Wiring, uid: [u8; 8]) -> Self {
        let pins = SimPins::new(&wiring.wires());
        let flash = MemFlash::new(STORAGE_SIZE as usize);
        let device = boot(&pins, &flash, uid);
        let encoder_phase = vec![0; wiring.encoders.len()];
        Self {
            device,
            pins,
            flash,
            wiring,
            uid,
            link: None,
            now: 0,
            assembler: Assembler::new(),
            to_host: VecDeque::new(),
            host: HostLog::default(),
            in_bootloader: false,
            usb: true,
            encoder_phase,
        }
    }

    /// Join two halves with a cable.
    pub fn connect(a: &mut VirtualKeyboard, b: &mut VirtualKeyboard) {
        let (end_a, end_b) = Pipe::pair();
        a.link = Some(end_a);
        b.link = Some(end_b);
    }

    /// Join two halves with a cable whose data wires run between the given
    /// pins: each pair is a pin of `a` and the pin of `b` it is soldered to.
    pub fn connect_wired(a: &mut VirtualKeyboard, b: &mut VirtualKeyboard, wires: &[(Pin, Pin)]) {
        Self::connect(a, b);
        for (pin_a, pin_b) in wires {
            a.pins.join(*pin_a, &b.pins, *pin_b);
        }
    }

    /// Plug the cable between the halves in or out (affects both ends).
    pub fn set_cable(&self, plugged: bool) {
        if let Some(link) = &self.link {
            link.set_plugged(plugged);
        }
    }

    /// Plug this keyboard's USB cable to the computer in or out. Without it
    /// nothing reaches the computer, but the keyboard keeps running (think
    /// of a half powered through the cable from the other half).
    pub fn set_usb(&mut self, plugged: bool) {
        self.usb = plugged;
    }

    /// Feed raw bytes into this half's end of the cable.
    pub fn inject_link_bytes(&self, bytes: &[u8]) {
        if let Some(link) = &self.link {
            link.inject(bytes);
        }
    }

    pub fn pins(&self) -> &SimPins {
        &self.pins
    }

    pub fn flash(&self) -> &MemFlash {
        &self.flash
    }

    pub fn key_count(&self) -> usize {
        self.wiring.switches.len()
    }

    /// Hold down or let go of the key wired as switch number `key`.
    pub fn set_key(&mut self, key: usize, pressed: bool) {
        if key < self.wiring.switches.len() {
            self.pins.set(key, pressed);
        }
    }

    /// Turn an encoder by one click, taking a few milliseconds like a hand.
    pub fn turn_encoder(&mut self, encoder: usize, clockwise: bool) {
        // The two signals close in turn; which one leads gives the direction.
        const PHASES: [(bool, bool); 4] = [(false, false), (true, false), (true, true), (false, true)];
        let Some(phase) = self.encoder_phase.get(encoder).copied() else { return };
        let base = self.wiring.switches.len() + 2 * encoder;
        let mut phase = phase;
        for _ in 0..4 {
            phase = if clockwise { (phase + 1) % 4 } else { (phase + 3) % 4 };
            let (a, b) = PHASES[phase as usize];
            self.pins.set(base, a);
            self.pins.set(base + 1, b);
            self.advance(2);
        }
        self.encoder_phase[encoder] = phase;
    }

    /// The computer says which lock keys are on (bit 0 num, 1 caps, 2 scroll).
    pub fn set_locks(&mut self, locks: u8) {
        self.device.set_locks(locks);
    }

    /// Let simulated time pass.
    pub fn advance(&mut self, ms: u64) {
        for _ in 0..ms {
            self.now += 1;
            if self.in_bootloader {
                continue;
            }
            let Self { device, host, to_host, link, now, usb, .. } = self;
            device.set_host(*usb);
            let mut nowhere = HostLog::default();
            let host = if *usb { host } else { &mut nowhere };
            let event = |event| push(to_host, &DeviceMessage::Event(event));
            match link {
                Some(link) => device.poll(*now, host, link, event),
                None => device.poll(*now, host, &mut (), event),
            }
            // A request passed on over the cable may restart the board.
            match self.device.take_reboot() {
                Some(true) => self.in_bootloader = true,
                Some(false) => self.reboot(),
                None => {}
            }
        }
    }

    /// Deliver one 64-byte report from the configurator.
    pub fn send_report(&mut self, report: &[u8]) {
        if self.in_bootloader {
            return;
        }
        let Some(request) = self.assembler.push::<Request>(report) else { return };
        let response = match request {
            Ok(request) => self.device.handle(request, &mut self.host),
            Err(_) => Response::Error(Error::Malformed),
        };
        push(&mut self.to_host, &DeviceMessage::Response(response));
        match self.device.take_reboot() {
            Some(true) => self.in_bootloader = true,
            Some(false) => self.reboot(),
            None => {}
        }
    }

    /// Reports waiting to be read by the configurator.
    pub fn take_reports(&mut self) -> Vec<[u8; REPORT_SIZE]> {
        self.to_host.drain(..).collect()
    }

    /// What the computer received since the last call.
    pub fn take_host_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.host.0)
    }

    pub fn in_bootloader(&self) -> bool {
        self.in_bootloader
    }

    /// Power cycle: unsaved changes are lost, saved ones come back.
    pub fn reboot(&mut self) {
        self.in_bootloader = false;
        self.assembler = Assembler::new();
        self.device = boot(&self.pins, &self.flash, self.uid);
        self.device.set_host(self.usb);
    }

    /// Convenience for tests and scripts: send a request, return the reply.
    pub fn request(&mut self, request: Request) -> Response {
        frame::encode(&request, |report| self.send_report(report)).expect("request fits a message");
        let mut assembler = Assembler::new();
        let mut reply = None;
        // Events that were queued before the reply stay queued.
        let mut kept = VecDeque::new();
        for report in self.to_host.drain(..) {
            if reply.is_some() {
                kept.push_back(report);
                continue;
            }
            match assembler.push::<DeviceMessage>(&report) {
                Some(Ok(DeviceMessage::Response(response))) => reply = Some(response),
                Some(Ok(DeviceMessage::Event(event))) => push(&mut kept, &DeviceMessage::Event(event)),
                _ => {}
            }
        }
        self.to_host = kept;
        reply.expect("the device answers every request")
    }
}

fn push(queue: &mut VecDeque<[u8; REPORT_SIZE]>, message: &DeviceMessage) {
    let _ = frame::encode(message, |report| queue.push_back(*report));
}

fn boot(pins: &SimPins, flash: &MemFlash, uid: [u8; 8]) -> Device<'static, SimPins, MemFlash> {
    // A real chip has this memory for its whole life; the simulation leaks
    // it per boot, which is a few kilobytes.
    let buf = Box::leak(Box::new([0u8; 16 * 1024]));
    let config = Box::leak(Box::new(Config::new()));
    let board = Board { mcu: Mcu::Simulated, firmware: jmk_proto::FIRMWARE_VERSION, uid };
    Device::new(board, pins.clone(), Storage::new(flash.clone(), 0, buf), config)
}
