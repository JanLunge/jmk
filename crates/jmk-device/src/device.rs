use heapless::Vec;
use embedded_storage::nor_flash::NorFlash;
use jmk_core::{Engine, Millis, Output};
use jmk_proto::{
    Contact, Error, Event, HardwareConfig, Info, Mcu, Pin, Request, Response, ACTIONS_PER_CHUNK, MAX_CONTACTS,
    MAX_HOLD_TAP_PROFILES, MAX_KEYS, MAX_LAYERS, MAX_PROBE_PINS, PROTOCOL_VERSION,
};

use crate::config::Config;
use crate::pins::{is_usable, Pins, Pull};
use crate::probe::probe;
use crate::scan::Scanner;
use crate::storage::Storage;

/// What the port tells the configurator about itself.
#[derive(Clone, Copy)]
pub struct Board {
    pub mcu: Mcu,
    pub firmware: (u8, u8, u8),
}

enum Mode {
    Typing,
    /// Wiring detection on these pins; normal scanning is off.
    Probing(Vec<Pin, MAX_PROBE_PINS>),
}

/// One keyboard (or one half of a split).
pub struct Device<'a, P, F> {
    board: Board,
    pins: P,
    storage: Storage<F>,
    config: &'a mut Config,
    engine: Engine,
    scanner: Scanner,
    mode: Mode,
    watch_keys: bool,
    contacts: Vec<Contact, MAX_CONTACTS>,
    /// Set once by a reboot request: true for the bootloader.
    reboot: Option<bool>,
}

impl<'a, P: Pins, F: NorFlash> Device<'a, P, F> {
    /// Load the saved configuration and set up the pins it names.
    pub fn new(board: Board, mut pins: P, mut storage: Storage<F>, config: &'a mut Config) -> Self {
        storage.load(config);
        // A save from another chip may name pins this one does not have.
        if validate(&config.hardware, &pins).is_err() {
            config.reset();
        }
        let mut scanner = Scanner::new();
        scanner.configure(&config.hardware.matrix, &mut pins);
        Self {
            board,
            pins,
            storage,
            config,
            engine: Engine::new(),
            scanner,
            mode: Mode::Typing,
            watch_keys: false,
            contacts: Vec::new(),
            reboot: None,
        }
    }

    /// Call every millisecond.
    pub fn poll(&mut self, now: Millis, out: &mut impl Output, mut event: impl FnMut(Event)) {
        match &self.mode {
            Mode::Typing => {
                let Self { scanner, config, engine, pins, watch_keys, .. } = self;
                let hardware = &config.hardware;
                let mut changes: Vec<(u16, bool), 16> = Vec::new();
                scanner.scan(&hardware.matrix, hardware.debounce_ms, pins, |pos, pressed| {
                    // More than 16 changes in one millisecond are picked up on
                    // the next scan only as far as debouncing allows; in
                    // practice this is unreachable by hand.
                    let _ = changes.push((pos, pressed));
                });
                for (pos, pressed) in changes {
                    engine.key_event(&**config, out, pos, pressed, now);
                    if *watch_keys {
                        event(Event::Key { pos, pressed });
                    }
                }
                if engine.next_deadline(&**config).is_some_and(|deadline| now >= deadline) {
                    engine.tick(&**config, out, now);
                }
            }
            Mode::Probing(candidates) => {
                let contacts = probe(candidates, &mut self.pins);
                if contacts != self.contacts {
                    self.contacts = contacts.clone();
                    event(Event::Probe(contacts));
                }
            }
        }
    }

    /// Pending reboot request, if any: `Some(true)` for the bootloader.
    pub fn take_reboot(&mut self) -> Option<bool> {
        self.reboot.take()
    }

    pub fn handle(&mut self, request: Request, out: &mut impl Output) -> Response {
        match self.try_handle(request, out) {
            Ok(response) => response,
            Err(error) => Response::Error(error),
        }
    }

    fn try_handle(&mut self, request: Request, out: &mut impl Output) -> Result<Response, Error> {
        match request {
            Request::GetInfo => Ok(Response::Info(Info {
                protocol: PROTOCOL_VERSION,
                firmware: self.board.firmware,
                mcu: self.board.mcu,
                usable_pins: self.pins.usable(),
                max_keys: MAX_KEYS as u16,
                max_layers: MAX_LAYERS as u8,
            })),
            Request::GetHardware => Ok(Response::Hardware(self.config.hardware.clone())),
            Request::SetHardware(hardware) => {
                validate(&hardware, &self.pins)?;
                self.stop_probe();
                self.release_matrix_pins();
                self.config.hardware = hardware;
                self.restart_scanning(out);
                Ok(Response::Ok)
            }
            Request::GetLayerCount => Ok(Response::LayerCount(self.config.layer_count)),
            Request::SetLayerCount(count) => {
                if count == 0 || count as usize > MAX_LAYERS {
                    return Err(Error::OutOfRange);
                }
                self.config.layer_count = count;
                self.engine.reset(out);
                Ok(Response::Ok)
            }
            Request::GetActions { layer, pos } => {
                let layer = self.config.actions.get(layer as usize).ok_or(Error::OutOfRange)?;
                let from = layer.get(pos as usize..).ok_or(Error::OutOfRange)?;
                Ok(Response::Actions(from.iter().take(ACTIONS_PER_CHUNK).copied().collect()))
            }
            Request::SetActions { layer, pos, actions } => {
                let layer = self.config.actions.get_mut(layer as usize).ok_or(Error::OutOfRange)?;
                let end = pos as usize + actions.len();
                let target = layer.get_mut(pos as usize..end).ok_or(Error::OutOfRange)?;
                target.copy_from_slice(&actions);
                Ok(Response::Ok)
            }
            Request::GetHoldTapProfiles => {
                Ok(Response::HoldTapProfiles(self.config.profiles.iter().copied().collect()))
            }
            Request::SetHoldTapProfile { index, profile } => {
                if index as usize >= MAX_HOLD_TAP_PROFILES {
                    return Err(Error::OutOfRange);
                }
                self.config.profiles[index as usize] = profile;
                Ok(Response::Ok)
            }
            Request::GetCombos => Ok(Response::Combos(self.config.combos.clone())),
            Request::SetCombos(combos) => {
                // Held combo keys refer to combos by index.
                self.engine.reset(out);
                self.config.combos = combos;
                Ok(Response::Ok)
            }
            Request::Save => {
                self.storage.save(self.config).map_err(|_| Error::Storage)?;
                Ok(Response::Ok)
            }
            Request::FactoryReset => {
                self.stop_probe();
                self.release_matrix_pins();
                self.config.reset();
                self.restart_scanning(out);
                self.storage.erase().map_err(|_| Error::Storage)?;
                Ok(Response::Ok)
            }
            Request::WatchKeys(on) => {
                self.watch_keys = on;
                Ok(Response::Ok)
            }
            Request::StartProbe(candidates) => {
                check_pins(candidates.iter().copied(), &self.config.hardware.reserved_pins, &self.pins)?;
                self.stop_probe();
                self.release_matrix_pins();
                self.engine.reset(out);
                self.contacts.clear();
                self.mode = Mode::Probing(candidates);
                Ok(Response::Ok)
            }
            Request::StopProbe => {
                self.stop_probe();
                self.restart_scanning(out);
                Ok(Response::Ok)
            }
            Request::Reboot { bootloader } => {
                self.reboot = Some(bootloader);
                Ok(Response::Ok)
            }
        }
    }

    fn stop_probe(&mut self) {
        if let Mode::Probing(candidates) = core::mem::replace(&mut self.mode, Mode::Typing) {
            for pin in candidates {
                self.pins.input(pin, Pull::None);
            }
        }
    }

    fn release_matrix_pins(&mut self) {
        for pin in self.config.hardware.matrix.pins() {
            self.pins.input(pin, Pull::None);
        }
    }

    fn restart_scanning(&mut self, out: &mut impl Output) {
        self.engine.reset(out);
        self.scanner.configure(&self.config.hardware.matrix, &mut self.pins);
    }
}

fn validate(hardware: &HardwareConfig, pins: &impl Pins) -> Result<(), Error> {
    if hardware.matrix.key_count() > MAX_KEYS {
        return Err(Error::TooManyKeys);
    }
    check_pins(hardware.matrix.pins(), &hardware.reserved_pins, pins)
}

/// Every pin must exist, be free for wiring, not be reserved and appear once.
fn check_pins(wanted: impl Iterator<Item = Pin>, reserved: &[Pin], pins: &impl Pins) -> Result<(), Error> {
    let mut seen = 0u64;
    for pin in wanted {
        if !is_usable(pins, pin) {
            return Err(Error::UnusablePin(pin));
        }
        if reserved.contains(&pin) {
            return Err(Error::ReservedPin(pin));
        }
        if seen & (1 << pin) != 0 {
            return Err(Error::DuplicatePin(pin));
        }
        seen |= 1 << pin;
    }
    Ok(())
}
