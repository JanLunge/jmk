use embedded_storage::nor_flash::NorFlash;
use heapless::Vec;
use jmk_core::{Engine, Millis, Output};
use jmk_proto::{
    Contact, Error, Event, HardwareConfig, Info, LightingConfig, Mcu, Pin, Request, Response, SplitConfig, SplitStatus,
    ACTIONS_PER_CHUNK, LAYOUT_KEYS_PER_CHUNK, MACRO_STEPS_PER_CHUNK, MAX_CONTACTS, MAX_HOLD_TAP_PROFILES, MAX_KEYS,
    MAX_LAYERS, MAX_MACROS, PROTOCOL_VERSION,
};

use crate::config::{Config, MacroError};
use crate::lights::Lights;
use crate::link::{Link, Remote, Split};
use crate::pins::{is_usable, Pins, Pull};
use crate::probe::Prober;
use crate::scan::{Change, Scanner};
use crate::storage::Storage;

/// What the port tells the configurator about itself.
#[derive(Clone, Copy)]
pub struct Board {
    pub mcu: Mcu,
    pub firmware: (u8, u8, u8),
    pub uid: [u8; 8],
}

enum Mode {
    Typing,
    /// Wiring detection on these pins; normal scanning is off.
    Probing(Prober),
    /// One pin held low for another board to find; normal scanning is off.
    Holding(Pin),
}

/// One keyboard (or one half of a split).
pub struct Device<'a, P, F> {
    board: Board,
    pins: P,
    storage: Storage<F>,
    config: &'a mut Config,
    engine: Engine,
    scanner: Scanner,
    lights: Lights,
    split: Split,
    now: Millis,
    mode: Mode,
    watch_keys: bool,
    /// The active layers the configurator was last told about.
    reported_layers: Option<u32>,
    contacts: Vec<Contact, MAX_CONTACTS>,
    /// Set once by a reboot request: true for the bootloader.
    reboot: Option<bool>,
}

impl<'a, P: Pins, F: NorFlash> Device<'a, P, F> {
    /// Load the saved configuration and set up the pins it names.
    pub fn new(board: Board, mut pins: P, mut storage: Storage<F>, config: &'a mut Config) -> Self {
        storage.load(config);
        // A save from another chip may name pins this one does not have.
        if validate(&config.hardware, &config.lighting, &pins).is_err() {
            config.reset();
        }
        let mut scanner = Scanner::new();
        scanner.configure(&config.hardware, &mut pins);
        Self {
            board,
            pins,
            storage,
            config,
            engine: Engine::new(),
            scanner,
            lights: Lights::new(),
            split: Split::new(),
            now: 0,
            mode: Mode::Typing,
            watch_keys: false,
            reported_layers: None,
            contacts: Vec::new(),
            reboot: None,
        }
    }

    /// The configuration in force, e.g. for the port to set up the split link.
    pub fn hardware(&self) -> &HardwareConfig {
        &self.config.hardware
    }

    /// The lighting in force, for the port to set up the light chain.
    pub fn lighting(&self) -> &LightingConfig {
        &self.config.lighting
    }

    /// Call every millisecond. Each call does a bounded amount of work.
    /// `link` leads to the other half; pass `&mut ()` if there is none.
    pub fn poll(&mut self, now: Millis, out: &mut impl Output, link: &mut impl Link, mut event: impl FnMut(Event)) {
        self.now = now;
        let Self { scanner, config, engine, pins, watch_keys, reported_layers, split, lights, .. } = self;
        lights.tick(&config.lighting, now, pins);
        let prober = match &mut self.mode {
            Mode::Probing(prober) => prober,
            Mode::Holding(_) => return,
            Mode::Typing => {
                let config: &Config = config;
                let hardware = &config.hardware;
                let mut changes: Vec<Change, 16> = Vec::new();
                // More than 16 changes in one millisecond wait for the next
                // scan; by hand this is unreachable.
                scanner.scan(hardware, pins, |change| {
                    let _ = changes.push(change);
                });

                let mut key = |pos: u16, pressed: bool| {
                    if pressed {
                        lights.key_pressed(&config.lighting, pos);
                    }
                    engine.key_event(config, out, pos, pressed, now);
                    if *watch_keys {
                        event(Event::Key { pos, pressed });
                    }
                };
                if let SplitConfig::Peripheral(_) = hardware.split {
                    // This half only reports; the central half has the keymap.
                    for change in changes {
                        match change {
                            Change::Key { pos, pressed } => {
                                if pressed {
                                    lights.key_pressed(&config.lighting, pos);
                                }
                                split.set_local(pos, pressed);
                                if *watch_keys {
                                    event(Event::Key { pos, pressed });
                                }
                            }
                            Change::Tap(pos) => split.send_tap(link, pos),
                        }
                    }
                    split.peripheral_tick(now, link, hardware.local_positions());
                    return;
                }
                for change in changes {
                    match change {
                        Change::Key { pos, pressed } => key(pos, pressed),
                        Change::Tap(pos) => {
                            key(pos, true);
                            key(pos, false);
                        }
                    }
                }
                if let SplitConfig::Central(_) = hardware.split {
                    let local = hardware.local_positions();
                    split.central_tick(now, link, |remote| {
                        let global = |pos: u8| (local + pos as usize) as u16;
                        match remote {
                            Remote::Key { pos, pressed } if local + (pos as usize) < MAX_KEYS => key(global(pos), pressed),
                            Remote::Tap(pos) if local + (pos as usize) < MAX_KEYS => {
                                key(global(pos), true);
                                key(global(pos), false);
                            }
                            _ => {}
                        }
                    });
                }
                if engine.next_deadline(config).is_some_and(|deadline| now >= deadline) {
                    engine.tick(config, out, now);
                }
                // Tell a watching configurator which layer each press used
                // and which layers are on, so it can show what a key did.
                let resolved = engine.take_resolved();
                if *watch_keys {
                    for key in resolved {
                        event(Event::Resolved { pos: key.pos, layer: key.layer });
                    }
                    let layers = engine.active_layers();
                    if *reported_layers != Some(layers) {
                        *reported_layers = Some(layers);
                        event(Event::Layers(layers));
                    }
                }
                return;
            }
        };
        if let Some(contacts) = prober.step(pins) {
            if contacts != self.contacts {
                self.contacts = contacts.clone();
                event(Event::Probe(contacts));
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
                uid: self.board.uid,
            })),
            Request::GetHardware => Ok(Response::Hardware(self.config.hardware.clone())),
            Request::SetHardware(hardware) => {
                validate(&hardware, &self.config.lighting, &self.pins)?;
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
            Request::GetMacro { index, start } => {
                let steps = jmk_core::Keymap::macro_steps(&*self.config, index);
                if index as usize >= MAX_MACROS || start as usize > steps.len() {
                    return Err(Error::OutOfRange);
                }
                let chunk = steps[start as usize..].iter().take(MACRO_STEPS_PER_CHUNK).copied().collect();
                Ok(Response::Macro { total: steps.len() as u16, steps: chunk })
            }
            Request::SetMacro { index, start, steps } => {
                // A macro being played must not change underneath the engine.
                self.engine.reset(out);
                match self.config.set_macro(index as usize, start as usize, &steps) {
                    Ok(()) => Ok(Response::Ok),
                    Err(MacroError::OutOfRange) => Err(Error::OutOfRange),
                    Err(MacroError::Full) => Err(Error::Full),
                }
            }
            Request::GetMeta => Ok(Response::Meta(self.config.meta.clone())),
            Request::SetMeta(meta) => {
                self.config.meta = meta;
                Ok(Response::Ok)
            }
            Request::GetLayout { start } => {
                let layout = &self.config.layout;
                let from = layout.get(start as usize..).ok_or(Error::OutOfRange)?;
                let keys = from.iter().take(LAYOUT_KEYS_PER_CHUNK).copied().collect();
                Ok(Response::Layout { total: layout.len() as u16, keys })
            }
            Request::SetLayout { start, keys } => {
                let layout = &mut self.config.layout;
                if start as usize > layout.len() {
                    return Err(Error::OutOfRange);
                }
                layout.truncate(start as usize);
                layout.extend_from_slice(&keys).map_err(|_| Error::TooManyKeys)?;
                Ok(Response::Ok)
            }
            Request::GetSplitStatus => Ok(Response::SplitStatus(SplitStatus {
                connected: self.split.connected(self.now),
                remote_positions: self.split.remote_positions() as u16,
            })),
            Request::Save => {
                self.storage.save(self.config).map_err(|_| Error::Storage)?;
                Ok(Response::Ok)
            }
            Request::FactoryReset => {
                self.lights.identify(None);
                self.stop_probe();
                self.release_matrix_pins();
                self.config.reset();
                self.restart_scanning(out);
                self.storage.erase().map_err(|_| Error::Storage)?;
                Ok(Response::Ok)
            }
            Request::WatchKeys(on) => {
                self.watch_keys = on;
                // Report the layers afresh to whoever starts watching.
                self.reported_layers = None;
                Ok(Response::Ok)
            }
            Request::StartProbe(candidates) => {
                self.check_free(candidates.iter().copied())?;
                self.stop_probe();
                self.release_matrix_pins();
                self.engine.reset(out);
                self.contacts.clear();
                self.mode = Mode::Probing(Prober::new(candidates));
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
            Request::HoldLow(None) => {
                if matches!(self.mode, Mode::Holding(_)) {
                    self.stop_probe();
                    self.restart_scanning(out);
                }
                Ok(Response::Ok)
            }
            Request::HoldLow(Some(pin)) => {
                self.check_free([pin].into_iter())?;
                self.stop_probe();
                self.release_matrix_pins();
                self.engine.reset(out);
                self.pins.drive_low(pin);
                self.mode = Mode::Holding(pin);
                Ok(Response::Ok)
            }
            Request::GetLighting => Ok(Response::Lighting(self.config.lighting.clone())),
            Request::SetLighting(lighting) => {
                validate(&self.config.hardware, &lighting, &self.pins)?;
                self.config.lighting = lighting;
                self.lights.changed();
                Ok(Response::Ok)
            }
            Request::IdentifyLed(led) => {
                if led.is_some_and(|led| led as usize >= self.config.lighting.leds.len()) {
                    return Err(Error::OutOfRange);
                }
                self.lights.identify(led);
                Ok(Response::Ok)
            }
            Request::GetLayerColors => Ok(Response::LayerColors(self.config.layer_colors.iter().copied().collect())),
            Request::SetLayerColors(colors) => {
                // Layers not mentioned go back to having no colour.
                self.config.layer_colors = [0; MAX_LAYERS];
                self.config.layer_colors[..colors.len()].copy_from_slice(&colors);
                Ok(Response::Ok)
            }
        }
    }

    /// Pins the user wants to probe must be free: not reserved and not the
    /// light chain's.
    fn check_free(&self, wanted: impl Iterator<Item = Pin> + Clone) -> Result<(), Error> {
        if let Some(pin) = self.config.lighting.pin.filter(|pin| wanted.clone().any(|w| w == *pin)) {
            return Err(Error::ReservedPin(pin));
        }
        check_pins(wanted, &self.config.hardware.reserved_pins, &self.pins)
    }

    fn stop_probe(&mut self) {
        match core::mem::replace(&mut self.mode, Mode::Typing) {
            Mode::Probing(prober) => {
                for pin in prober.candidates() {
                    self.pins.input(*pin, Pull::None);
                }
            }
            Mode::Holding(pin) => self.pins.input(pin, Pull::None),
            Mode::Typing => {}
        }
    }

    fn release_matrix_pins(&mut self) {
        // The link pins belong to the port's serial driver, not to us.
        let hardware = &self.config.hardware;
        for pin in hardware.matrix.pins().chain(hardware.encoders.iter().flat_map(|e| [e.a, e.b])) {
            self.pins.input(pin, Pull::None);
        }
    }

    fn restart_scanning(&mut self, out: &mut impl Output) {
        self.engine.reset(out);
        self.split = Split::new();
        self.scanner.configure(&self.config.hardware, &mut self.pins);
    }
}

fn validate(hardware: &HardwareConfig, lighting: &LightingConfig, pins: &impl Pins) -> Result<(), Error> {
    if hardware.local_positions() > MAX_KEYS {
        return Err(Error::TooManyKeys);
    }
    if hardware.split.link().is_some_and(|link| !pins.supports_link(&link)) {
        return Err(Error::Unsupported);
    }
    if lighting.pin.is_some_and(|pin| is_usable(pins, pin) && !pins.supports_lights(pin)) {
        return Err(Error::Unsupported);
    }
    check_pins(hardware.used_pins().chain(lighting.pin), &hardware.reserved_pins, pins)
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
