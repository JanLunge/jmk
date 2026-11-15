//! Runs the device logic against simulated wiring and flash.

use std::cell::RefCell;
use std::rc::Rc;

use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use jmk_core::keycode as kc;
use jmk_core::{Action, Basic, KeyboardReport, Output};
use jmk_device::{Board, Config, Device, Pins, Pull, Storage, STORAGE_SIZE};
use jmk_proto::{
    Contact, DiodeDirection, Error, Event, HardwareConfig, MatrixConfig, Mcu, Pin, Request, Response,
};

#[derive(Clone, Copy)]
enum Wire {
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

#[derive(Clone)]
struct Sim(Rc<RefCell<SimState>>);

struct SimState {
    modes: [PinMode; 64],
    /// Switches and whether each is closed.
    switches: Vec<(Wire, bool)>,
    settles: usize,
}

impl Sim {
    fn new(switches: &[Wire]) -> Self {
        let switches = switches.iter().map(|w| (*w, false)).collect();
        Sim(Rc::new(RefCell::new(SimState { modes: [PinMode::Untouched; 64], switches, settles: 0 })))
    }
    fn set(&self, switch: usize, closed: bool) {
        self.0.borrow_mut().switches[switch].1 = closed;
    }
    fn settles(&self) -> usize {
        self.0.borrow().settles
    }
    fn touched(&self) -> Vec<Pin> {
        let state = self.0.borrow();
        (0..64).filter(|p| state.modes[*p as usize] != PinMode::Untouched).collect()
    }
}

impl Pins for Sim {
    fn usable(&self) -> u64 {
        0x3FFF_FFFF // GPIO 0..=29
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
    fn settle(&mut self) {
        self.0.borrow_mut().settles += 1;
    }
}

#[derive(Clone, Default)]
struct Flash {
    bytes: Rc<RefCell<Vec<u8>>>,
    /// Writes left before the power "fails".
    writes_left: Rc<RefCell<Option<usize>>>,
}

#[derive(Debug)]
struct PowerCut;

impl NorFlashError for PowerCut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

impl ErrorType for Flash {
    type Error = PowerCut;
}

impl ReadNorFlash for Flash {
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

impl NorFlash for Flash {
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
        // A cut write leaves half the data behind.
        let len = if cut { bytes.len() / 2 } else { bytes.len() };
        let mut flash = self.bytes.borrow_mut();
        for (cell, byte) in flash[offset as usize..].iter_mut().zip(&bytes[..len]) {
            *cell &= *byte;
        }
        if cut { Err(PowerCut) } else { Ok(()) }
    }
}

#[derive(Default)]
struct Log(Vec<String>);

impl Output for Log {
    fn keyboard(&mut self, report: &KeyboardReport) {
        let keys: Vec<String> = report.keys.iter().map(|k| format!("{k:02x}")).collect();
        self.0.push(format!("{:02x}:{}", report.mods, keys.join(",")));
    }
}

struct Rig {
    device: Device<'static, Sim, Flash>,
    sim: Sim,
    flash: Flash,
    log: Log,
    events: Vec<Event>,
    now: u64,
}

impl Rig {
    fn new(switches: &[Wire]) -> Self {
        let flash = Flash::default();
        flash.bytes.borrow_mut().resize(STORAGE_SIZE as usize, 0xFF);
        Self::boot(Sim::new(switches), flash)
    }

    /// Power up with the given wiring and flash contents.
    fn boot(sim: Sim, flash: Flash) -> Self {
        let board = Board { mcu: Mcu::Simulated, firmware: (0, 1, 0) };
        let buf = Box::leak(Box::new([0u8; 16 * 1024]));
        let config = Box::leak(Box::new(Config::new()));
        let device = Device::new(board, sim.clone(), Storage::new(flash.clone(), 0, buf), config);
        Rig { device, sim, flash, log: Log::default(), events: vec![], now: 0 }
    }

    fn reboot(self) -> Self {
        Self::boot(self.sim, self.flash)
    }

    fn ok(&mut self, request: Request) {
        assert_eq!(self.device.handle(request, &mut self.log), Response::Ok);
    }

    fn run(&mut self, ms: u64) {
        for _ in 0..ms {
            self.now += 1;
            self.device.poll(self.now, &mut self.log, |event| self.events.push(event));
        }
    }

    fn out(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log.0)
    }

    fn set_keys(&mut self, layer: u8, keys: &[u8]) {
        let actions = keys.iter().map(|k| Action::key(*k)).collect();
        self.ok(Request::SetActions { layer, pos: 0, actions });
    }
}

fn hardware(matrix: MatrixConfig) -> HardwareConfig {
    HardwareConfig { matrix, reserved_pins: Default::default(), debounce_ms: 5 }
}

fn pins(list: &[Pin]) -> heapless::Vec<Pin, 64> {
    list.iter().copied().collect()
}

/// Polls that are certain to contain one complete probe pass in these tests.
const PASS: u64 = 20;

/// 2x2 matrix, rows on 2 and 3, columns on 4 and 5, diodes column to row.
fn diode_matrix() -> [Wire; 4] {
    let key = |col, row| Wire::Diode { anode: col, cathode: row };
    [key(4, 2), key(5, 2), key(4, 3), key(5, 3)]
}

#[test]
fn unconfigured_device_touches_no_pins() {
    let mut rig = Rig::new(&[Wire::Ground(7)]);
    rig.sim.set(0, true);
    rig.run(20);
    assert!(rig.sim.touched().is_empty());
    assert!(rig.out().is_empty());
}

#[test]
fn direct_wiring_types_keys() {
    let mut rig = Rig::new(&[Wire::Ground(7), Wire::Ground(9)]);
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Direct { pins: pins(&[7, 9]), active_high: false })));
    rig.set_keys(0, &[kc::A, kc::B]);
    rig.sim.set(1, true);
    rig.run(2);
    rig.sim.set(1, false);
    rig.run(10);
    assert_eq!(rig.out(), ["00:05", "00:"]);
    assert_eq!(rig.sim.touched(), [7, 9]);
}

#[test]
fn active_high_direct_wiring() {
    let mut rig = Rig::new(&[Wire::Supply(7)]);
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Direct { pins: pins(&[7]), active_high: true })));
    rig.set_keys(0, &[kc::A]);
    rig.run(5);
    assert!(rig.out().is_empty());
    rig.sim.set(0, true);
    rig.run(2);
    assert_eq!(rig.out(), ["00:04"]);
}

#[test]
fn diode_matrix_scans_in_the_configured_direction() {
    let matrix = |diodes| {
        hardware(MatrixConfig::Matrix { rows: pins(&[2, 3]).iter().copied().collect(), cols: pins(&[4, 5]).iter().copied().collect(), diodes })
    };
    let mut rig = Rig::new(&diode_matrix());
    rig.ok(Request::SetHardware(matrix(DiodeDirection::Col2Row)));
    rig.set_keys(0, &[kc::A, kc::B, kc::C, kc::D]);
    // Three keys of a square: with diodes the fourth must not ghost.
    for switch in [0, 1, 2] {
        rig.sim.set(switch, true);
    }
    rig.run(2);
    assert_eq!(rig.out(), ["00:04", "00:04,05", "00:04,05,06"]);

    // Scanning against the diodes sees nothing.
    rig.ok(Request::SetHardware(matrix(DiodeDirection::Row2Col)));
    assert_eq!(rig.out(), ["00:"]);
    rig.run(10);
    assert!(rig.out().is_empty());
}

#[test]
fn bounce_is_ignored() {
    let mut rig = Rig::new(&[Wire::Ground(7)]);
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Direct { pins: pins(&[7]), active_high: false })));
    rig.set_keys(0, &[kc::A]);
    for closed in [true, false, true, false, true] {
        rig.sim.set(0, closed);
        rig.run(1);
    }
    rig.run(10);
    assert_eq!(rig.out(), ["00:04"]);
}

#[test]
fn watch_keys_reports_positions() {
    let mut rig = Rig::new(&diode_matrix());
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Matrix {
        rows: pins(&[2, 3]).iter().copied().collect(),
        cols: pins(&[4, 5]).iter().copied().collect(),
        diodes: DiodeDirection::Col2Row,
    })));
    rig.ok(Request::WatchKeys(true));
    rig.sim.set(3, true);
    rig.run(2);
    assert_eq!(rig.events, [Event::Key { pos: 3, pressed: true }]);
}

#[test]
fn probe_finds_matrix_links_and_diode_direction() {
    let mut rig = Rig::new(&diode_matrix());
    rig.ok(Request::StartProbe(pins(&[2, 3, 4, 5, 6]).iter().copied().collect()));
    rig.run(30);
    assert!(rig.events.is_empty(), "nothing pressed yet");

    rig.sim.set(1, true); // column 5, row 2
    rig.run(PASS);
    let link = Contact::Link { driven: 2, sensed: 5 };
    assert_eq!(rig.events, [Event::Probe([link].into_iter().collect())]);

    rig.sim.set(1, false);
    rig.run(PASS);
    assert_eq!(rig.events[1], Event::Probe(Default::default()));
    assert!(rig.out().is_empty(), "probing must not type");
}

#[test]
fn probe_finds_direct_and_diodeless_switches() {
    let mut rig = Rig::new(&[Wire::Ground(7), Wire::Supply(8), Wire::Plain(10, 11)]);
    rig.ok(Request::StartProbe(pins(&[7, 8, 10, 11]).iter().copied().collect()));
    for switch in 0..3 {
        rig.sim.set(switch, true);
    }
    rig.run(PASS);
    let expected = [
        Contact::Supply(8),
        Contact::Ground(7),
        Contact::Link { driven: 10, sensed: 11 },
        Contact::Link { driven: 11, sensed: 10 },
    ];
    assert_eq!(rig.events, [Event::Probe(expected.into_iter().collect())]);
}

#[test]
fn reserved_and_unusable_pins_are_never_touched() {
    let mut rig = Rig::new(&[]);
    let mut hw = hardware(MatrixConfig::None);
    hw.reserved_pins = pins(&[16, 17]).iter().copied().collect();
    rig.ok(Request::SetHardware(hw.clone()));

    let probe = Request::StartProbe(pins(&[2, 3, 17]).iter().copied().collect());
    assert_eq!(rig.device.handle(probe, &mut rig.log), Response::Error(Error::ReservedPin(17)));
    let probe = Request::StartProbe(pins(&[2, 30]).iter().copied().collect());
    assert_eq!(rig.device.handle(probe, &mut rig.log), Response::Error(Error::UnusablePin(30)));

    hw.matrix = MatrixConfig::Direct { pins: pins(&[4, 16]), active_high: false };
    assert_eq!(rig.device.handle(Request::SetHardware(hw.clone()), &mut rig.log), Response::Error(Error::ReservedPin(16)));
    hw.matrix = MatrixConfig::Direct { pins: pins(&[4, 4]), active_high: false };
    assert_eq!(rig.device.handle(Request::SetHardware(hw), &mut rig.log), Response::Error(Error::DuplicatePin(4)));

    rig.run(10);
    assert!(rig.sim.touched().is_empty());
}

#[test]
fn probing_many_pins_keeps_each_poll_short() {
    // Each poll may wait for levels to settle only a couple of times,
    // however many pins are probed; a long poll starves USB on real hardware.
    let mut rig = Rig::new(&[]);
    rig.ok(Request::StartProbe((0..30).collect()));
    for _ in 0..100 {
        let before = rig.sim.settles();
        rig.run(1);
        assert!(rig.sim.settles() - before <= 2);
    }
}

#[test]
fn stopping_a_probe_resumes_typing() {
    let mut rig = Rig::new(&[Wire::Ground(7)]);
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Direct { pins: pins(&[7]), active_high: false })));
    rig.set_keys(0, &[kc::A]);
    rig.ok(Request::StartProbe(pins(&[7, 8]).iter().copied().collect()));
    rig.sim.set(0, true);
    rig.run(5);
    assert!(rig.out().is_empty());
    rig.ok(Request::StopProbe);
    rig.run(2);
    assert_eq!(rig.out(), ["00:04"]);
}

#[test]
fn configuration_survives_reboot_after_save() {
    let mut rig = Rig::new(&[Wire::Ground(7)]);
    let hw = hardware(MatrixConfig::Direct { pins: pins(&[7]), active_high: false });
    rig.ok(Request::SetHardware(hw.clone()));
    rig.ok(Request::SetLayerCount(2));
    let hold_tap = Action::HoldTap { hold: Basic::MomentaryLayer(1), tap: Basic::Key(kc::SPACE), profile: 1 };
    rig.ok(Request::SetActions { layer: 1, pos: 0, actions: [hold_tap].into_iter().collect() });

    // Not saved: gone after a reboot.
    let mut rig = rig.reboot();
    assert_eq!(rig.device.handle(Request::GetHardware, &mut rig.log), Response::Hardware(hardware(MatrixConfig::None)));

    rig.ok(Request::SetHardware(hw.clone()));
    rig.ok(Request::SetLayerCount(2));
    rig.ok(Request::SetActions { layer: 1, pos: 0, actions: [hold_tap].into_iter().collect() });
    rig.ok(Request::Save);
    let mut rig = rig.reboot();
    assert_eq!(rig.device.handle(Request::GetHardware, &mut rig.log), Response::Hardware(hw));
    assert_eq!(rig.device.handle(Request::GetLayerCount, &mut rig.log), Response::LayerCount(2));
    let Response::Actions(actions) = rig.device.handle(Request::GetActions { layer: 1, pos: 0 }, &mut rig.log) else {
        panic!("expected actions");
    };
    assert_eq!(actions[0], hold_tap);
    assert_eq!(actions[1], Action::Transparent);
}

#[test]
fn power_cut_during_save_keeps_previous_configuration() {
    let mut rig = Rig::new(&[]);
    rig.set_keys(0, &[kc::A]);
    rig.ok(Request::Save);
    rig.set_keys(0, &[kc::B]);
    rig.ok(Request::Save);

    rig.set_keys(0, &[kc::C]);
    *rig.flash.writes_left.borrow_mut() = Some(0);
    assert_eq!(rig.device.handle(Request::Save, &mut rig.log), Response::Error(Error::Storage));
    *rig.flash.writes_left.borrow_mut() = None;

    let mut rig = rig.reboot();
    let Response::Actions(actions) = rig.device.handle(Request::GetActions { layer: 0, pos: 0 }, &mut rig.log) else {
        panic!("expected actions");
    };
    assert_eq!(actions[0], Action::key(kc::B));

    // The next save must still work and win.
    rig.set_keys(0, &[kc::D]);
    rig.ok(Request::Save);
    let mut rig = rig.reboot();
    let Response::Actions(actions) = rig.device.handle(Request::GetActions { layer: 0, pos: 0 }, &mut rig.log) else {
        panic!("expected actions");
    };
    assert_eq!(actions[0], Action::key(kc::D));
}

#[test]
fn factory_reset_erases_everything() {
    let mut rig = Rig::new(&[Wire::Ground(7)]);
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Direct { pins: pins(&[7]), active_high: false })));
    rig.ok(Request::Save);
    rig.ok(Request::FactoryReset);
    let mut rig = rig.reboot();
    assert_eq!(rig.device.handle(Request::GetHardware, &mut rig.log), Response::Hardware(hardware(MatrixConfig::None)));
}
