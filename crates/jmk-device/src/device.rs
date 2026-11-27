use embedded_storage::nor_flash::NorFlash;
use heapless::Vec;
use jmk_core::{Engine, Millis, Output};
use jmk_proto::{
    Contact, Error, Event, HardwareConfig, Info, LightingConfig, MatrixConfig, Mcu, Pin, Request, Response, SplitConfig, SplitStatus, SplitSync,
    ACTIONS_PER_CHUNK, LAYOUT_KEYS_PER_CHUNK, MACRO_STEPS_PER_CHUNK, MAX_CONTACTS, MAX_HOLD_TAP_PROFILES, MAX_KEYS,
    MAX_LAYERS, MAX_MACROS, PROTOCOL_VERSION,
};

use crate::config::{Config, MacroError};
use crate::lights::Lights;
use crate::link::{Link, Own, Post, Remote, Split, CHUNK};
use crate::pins::{is_usable, Pins, Pull};
use crate::probe::Prober;
use crate::scan::{Change, Scanner};
use crate::storage::Storage;
use crate::sync::{decide, Side, Verdict};

/// How long two halves must say the same before a keymap moves, so that a
/// configurator writing to both is not overtaken.
const SETTLE_MS: Millis = 400;
/// How long to wait for a piece of keymap before asking again.
const RETRY_MS: Millis = 60;

/// A keymap on its way from the other half.
struct Fetch {
    digest: u32,
    /// Bytes received; they wait in the storage's scratch memory.
    got: usize,
    asked: Option<Millis>,
}

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
    /// A computer is listening to this board (see [`Device::set_host`]).
    host: bool,
    /// This half typed on the last pass; see [`Split::leading`].
    leading: bool,
    /// [`Config::shared_digest`], worked out again after a change.
    digest: Option<u32>,
    /// `digest` of what is saved in flash.
    stored_digest: u32,
    /// The user chose this half's keymap for both, or the other half's.
    force: bool,
    take: bool,
    /// The two digests last compared and since when they have been these.
    compared: Option<((u32, u32), Millis)>,
    fetch: Option<Fetch>,
    sync: SplitSync,
    /// The digest and saved state `sync` was worked out for.
    sync_of: Option<(u32, bool)>,
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
        if validate(&config.hardware, &config.more_wiring, &config.lighting, &pins).is_err() {
            config.reset();
        }
        let mut scanner = Scanner::new();
        scanner.configure(&config.hardware, &config.more_wiring, &config.doubles, &mut pins);
        let stored_digest = config.shared_digest();
        // A half first listens for the other one; see `Split::leading`.
        let leading = config.hardware.split == SplitConfig::None;
        Self {
            board,
            pins,
            storage,
            config,
            engine: Engine::new(),
            scanner,
            lights: Lights::new(),
            split: Split::new(),
            host: true,
            leading,
            digest: Some(stored_digest),
            stored_digest,
            force: false,
            take: false,
            compared: None,
            fetch: None,
            sync: SplitSync::Unknown,
            sync_of: None,
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

    /// Tell the device whether a computer is listening to it, e.g. whether
    /// USB is configured. Of two connected halves the one with a computer
    /// types for both. Assumed true until the port says otherwise.
    pub fn set_host(&mut self, present: bool) {
        self.host = present;
    }

    /// Call every millisecond. Each call does a bounded amount of work.
    /// `link` leads to the other half; pass `&mut ()` if there is none.
    pub fn poll(&mut self, now: Millis, out: &mut impl Output, link: &mut impl Link, mut event: impl FnMut(Event)) {
        self.now = now;
        let Self { scanner, config, engine, pins, watch_keys, reported_layers, split, lights, leading, digest, .. } = self;
        let (stored_digest, force) = (self.stored_digest, self.force);
        let (host, uid) = (self.host, self.board.uid);
        lights.tick(&config.lighting, now, pins);
        let prober = match &mut self.mode {
            Mode::Probing(prober) => prober,
            Mode::Holding(_) => return,
            Mode::Typing => {
                let config: &Config = config;
                let hardware = &config.hardware;
                let offset = hardware.split.offset();
                let mut changes: Vec<Change, 16> = Vec::new();
                // More than 16 changes in one millisecond wait for the next
                // scan; by hand this is unreachable.
                scanner.scan(hardware, &config.more_wiring, pins, |change| {
                    let _ = changes.push(change);
                });

                // While the other half types, this one only reports its keys.
                let leads = *leading;
                let mut key = |pos: usize, pressed: bool, own: bool| {
                    if pos >= MAX_KEYS {
                        return;
                    }
                    let pos = pos as u16;
                    if pressed && (own || leads) {
                        lights.key_pressed(&config.lighting, pos);
                    }
                    if leads {
                        engine.key_event(config, out, pos, pressed, now);
                    }
                    if *watch_keys && (own || leads) {
                        event(Event::Key { pos, pressed });
                    }
                };
                let is_half = hardware.split != SplitConfig::None;
                let mut post = Post::default();
                for change in changes {
                    match change {
                        Change::Key { pos, pressed } => {
                            split.set_local(pos, pressed);
                            key(offset + pos as usize, pressed, true);
                        }
                        Change::Tap(pos) => {
                            if is_half && !leads {
                                split.send_tap(link, pos);
                            }
                            key(offset + pos as usize, true, true);
                            key(offset + pos as usize, false, true);
                        }
                    }
                }
                if is_half {
                    let local = hardware.local_positions(&config.more_wiring);
                    // The first half's positions come first in the keymap.
                    let first = offset == 0;
                    let theirs = if first { local } else { 0 };
                    let now_digest = *digest.get_or_insert_with(|| config.shared_digest());
                    let about = Side {
                        // Without a computer nobody can save what is unsaved
                        // here: this half counts with what it has in flash.
                        digest: if host { now_digest } else { stored_digest },
                        base: config.base,
                        revision: config.revision,
                        clean: !host || now_digest == stored_digest,
                        force,
                    };
                    let own = Own { host, first, positions: local, uid, about };
                    post = split.tick(now, link, own, |remote| match remote {
                        Remote::Key { pos, pressed } => key(theirs + pos as usize, pressed, false),
                        Remote::Tap(pos) => {
                            key(theirs + pos as usize, true, false);
                            key(theirs + pos as usize, false, false);
                        }
                    });
                    let now_leads = split.leading(now, host, first);
                    if now_leads != leads {
                        // Hand over cleanly: nothing stays held on the half
                        // that stops, and the one that starts hears afresh
                        // what the other holds.
                        engine.reset(out);
                        split.forget_remote();
                        *leading = now_leads;
                    }
                } else {
                    *leading = true;
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
                if is_half {
                    self.sync_keymap(now, out, link, post);
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

    /// Bring the two halves' keymaps together; see [`crate::sync`].
    fn sync_keymap(&mut self, now: Millis, out: &mut impl Output, link: &mut impl Link, post: Post) {
        let live = *self.digest.get_or_insert_with(|| self.config.shared_digest());
        let saved = live == self.stored_digest;
        // A half that no computer listens to counts with what it has saved.
        // Changes made on it and never saved give way once the other half
        // needs the saved keymap or brings a newer one.
        let (digest, clean) = if self.host { (live, saved) } else { (self.stored_digest, true) };
        if let Some((wanted, offset)) = post.fetch {
            if !saved && clean && wanted == digest {
                self.engine.reset(out);
                self.storage.load(self.config);
                self.digest = None;
            } else if clean && wanted == digest {
                let mut data = [0u8; CHUNK];
                let (len, total) = self.config.shared_chunk(offset as usize, &mut data);
                self.split.send_chunk(link, offset, total as u16, &data[..len]);
            }
        }
        let Some(peer) = self.split.peer(now) else {
            self.compared = None;
            self.fetch = None;
            self.sync = SplitSync::Unknown;
            return;
        };
        let me = Side { digest, base: self.config.base, revision: self.config.revision, clean, force: self.force };
        self.sync_of = Some((live, saved));
        let pair = (digest, peer.digest);
        let since = match self.compared {
            Some((compared, since)) if compared == pair => since,
            _ => now,
        };
        self.compared = Some((pair, since));
        let settled = now - since >= SETTLE_MS && clean && peer.clean;
        let mut verdict = decide(&me, &peer);
        if self.take && verdict != Verdict::Same {
            verdict = Verdict::Fetch;
        }
        if verdict != Verdict::Fetch {
            self.fetch = None;
        }
        self.sync = match verdict {
            Verdict::Same => {
                self.force = false;
                self.take = false;
                if settled {
                    // Remember the agreement, so a later change on one half
                    // is known to be the newer keymap.
                    let revision = me.revision.max(peer.revision);
                    if saved && (self.config.base != Some(digest) || self.config.revision != revision) {
                        self.config.base = Some(digest);
                        self.config.revision = revision;
                        let _ = self.storage.save(self.config);
                    }
                }
                SplitSync::Same
            }
            Verdict::Fetch if settled => {
                self.fetch_from(now, out, link, peer, post);
                SplitSync::Behind
            }
            Verdict::Offer if settled => SplitSync::Ahead,
            Verdict::Conflict if settled => SplitSync::Conflict,
            _ => SplitSync::Unknown,
        };
    }

    /// Take the next piece of the other half's keymap, and the keymap into
    /// use once it is complete.
    fn fetch_from(&mut self, now: Millis, out: &mut impl Output, link: &mut impl Link, peer: Side, post: Post) {
        let fetch = match &mut self.fetch {
            Some(fetch) if fetch.digest == peer.digest => fetch,
            _ => self.fetch.insert(Fetch { digest: peer.digest, got: 0, asked: None }),
        };
        let scratch = self.storage.scratch();
        let mut total = None;
        if let Some(chunk) = post.chunk.filter(|chunk| chunk.offset as usize == fetch.got) {
            let (len, whole) = (chunk.len as usize, chunk.total as usize);
            if whole > scratch.len() || fetch.got + len > whole || len == 0 {
                self.fetch = None;
                return;
            }
            scratch[fetch.got..fetch.got + len].copy_from_slice(&chunk.data[..len]);
            fetch.got += len;
            fetch.asked = None;
            total = Some(whole);
        }
        if total != Some(fetch.got) {
            if fetch.asked.is_none_or(|at| now - at >= RETRY_MS) {
                fetch.asked = Some(now);
                let (digest, offset) = (fetch.digest, fetch.got as u16);
                self.split.send_fetch(link, digest, offset);
            }
            return;
        }
        let Some(Fetch { digest, got, .. }) = self.fetch.take() else { return };
        self.engine.reset(out);
        let taken = self.config.set_shared(&self.storage.scratch()[..got]).is_some();
        if taken && self.config.shared_digest() == digest {
            self.config.revision = peer.revision;
            self.config.base = Some(digest);
            if self.storage.save(self.config).is_ok() {
                self.stored_digest = digest;
            }
        } else {
            // Damaged on the way: back to what was saved, and ask again.
            self.storage.load(self.config);
        }
        self.digest = None;
        self.take = false;
    }

    /// Call once the port's drivers for the lights and the cable follow a
    /// changed [`lighting`](Self::lighting) pin or split link: a pin they
    /// let go of may be wired to a key now, and letting go undid its setup.
    pub fn claim_pins(&mut self) {
        self.pins.reclaim();
        crate::scan::idle_pins(&self.config.hardware, &self.config.more_wiring, &mut self.pins);
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
        // Any request may change the keymap; work its digest out again.
        self.digest = None;
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
                validate(&hardware, &self.config.more_wiring, &self.config.lighting, &self.pins)?;
                self.stop_probe();
                self.release_matrix_pins();
                self.config.hardware = hardware;
                // They were counted in the old wiring's positions.
                self.config.doubles.clear();
                self.restart_scanning(out);
                Ok(Response::Ok)
            }
            Request::GetMoreWiring => Ok(Response::MoreWiring(self.config.more_wiring.clone())),
            Request::SetMoreWiring(more) => {
                if more.iter().any(|part| *part == MatrixConfig::None) {
                    return Err(Error::OutOfRange);
                }
                validate(&self.config.hardware, &more, &self.config.lighting, &self.pins)?;
                self.stop_probe();
                self.release_matrix_pins();
                self.config.more_wiring = more;
                self.config.doubles.clear();
                self.restart_scanning(out);
                Ok(Response::Ok)
            }
            Request::GetDoubles => Ok(Response::Doubles(self.config.doubles.clone())),
            Request::SetDoubles(doubles) => {
                let switches = self.config.hardware.switch_count(&self.config.more_wiring);
                if doubles.iter().any(|pos| *pos as usize >= switches) {
                    return Err(Error::OutOfRange);
                }
                self.stop_probe();
                self.release_matrix_pins();
                self.config.doubles = doubles;
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
                self.config.combo_options.truncate(combos.len());
                self.config.combos = combos;
                Ok(Response::Ok)
            }
            Request::GetComboOptions => Ok(Response::ComboOptions(self.config.combo_options.clone())),
            Request::SetComboOptions(options) => {
                if options.len() > self.config.combos.len() {
                    return Err(Error::OutOfRange);
                }
                self.engine.reset(out);
                self.config.combo_options = options;
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
            Request::GetSplitStatus => {
                let digest = *self.digest.get_or_insert_with(|| self.config.shared_digest());
                Ok(Response::SplitStatus(SplitStatus {
                    connected: self.split.connected(self.now),
                    remote_positions: self.split.remote_positions() as u16,
                    leading: self.leading,
                    // A change made since the last pass has not been compared yet.
                    sync: if self.sync_of == Some((digest, digest == self.stored_digest)) { self.sync } else { SplitSync::Unknown },
                    peer_uid: self.split.peer_uid(),
                    revision: self.config.revision,
                    peer_revision: self.split.peer(self.now).map_or(0, |peer| peer.revision),
                    digest,
                    base: self.config.base,
                }))
            }
            Request::Save => {
                // A keymap on its way in waits in the memory a save uses.
                self.fetch = None;
                let digest = self.config.shared_digest();
                let changed = digest != self.stored_digest;
                if changed {
                    self.config.revision += 1;
                }
                if self.storage.save(self.config).is_err() {
                    self.config.revision -= changed as u32;
                    return Err(Error::Storage);
                }
                self.stored_digest = digest;
                Ok(Response::Ok)
            }
            Request::ResolveSplit { keep_this } => {
                if self.config.hardware.split == SplitConfig::None {
                    return Err(Error::Unsupported);
                }
                self.force = keep_this;
                self.take = !keep_this;
                Ok(Response::Ok)
            }
            Request::SplitAgreed { digest, revision } => {
                let mine = self.config.shared_digest();
                if mine != digest {
                    return Err(Error::OutOfRange);
                }
                if self.config.base != Some(digest) || self.config.revision < revision {
                    self.config.base = Some(digest);
                    self.config.revision = self.config.revision.max(revision);
                    if mine == self.stored_digest {
                        self.storage.save(self.config).map_err(|_| Error::Storage)?;
                    }
                }
                Ok(Response::Ok)
            }
            Request::FactoryReset => {
                self.lights.identify(None);
                self.stop_probe();
                self.release_matrix_pins();
                self.config.reset();
                self.restart_scanning(out);
                self.storage.erase().map_err(|_| Error::Storage)?;
                self.stored_digest = self.config.shared_digest();
                (self.force, self.take) = (false, false);
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
                validate(&self.config.hardware, &self.config.more_wiring, &lighting, &self.pins)?;
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
            Request::GetMacroName { index } => match self.config.macro_names.get(index as usize) {
                Some(name) => Ok(Response::MacroName(name.clone())),
                None => Err(Error::OutOfRange),
            },
            Request::GetLetterRows => Ok(Response::LetterRows(self.config.letter_rows.clone())),
            Request::SetLetterRows(rows) => {
                self.config.letter_rows = rows;
                Ok(Response::Ok)
            }
            Request::SetMacroName { index, name } => match self.config.macro_names.get_mut(index as usize) {
                Some(slot) => {
                    *slot = name;
                    Ok(Response::Ok)
                }
                None => Err(Error::OutOfRange),
            },
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
        let switches = hardware.wiring(&self.config.more_wiring).flat_map(MatrixConfig::pins);
        for pin in switches.chain(hardware.encoders.iter().flat_map(|e| [e.a, e.b])) {
            self.pins.input(pin, Pull::None);
        }
    }

    fn restart_scanning(&mut self, out: &mut impl Output) {
        self.engine.reset(out);
        self.split = Split::new();
        // A half first listens for the other one; see `Split::leading`.
        self.leading = self.config.hardware.split == SplitConfig::None;
        self.compared = None;
        self.fetch = None;
        self.sync = SplitSync::Unknown;
        self.scanner.configure(&self.config.hardware, &self.config.more_wiring, &self.config.doubles, &mut self.pins);
    }
}

fn validate(hardware: &HardwareConfig, more: &[MatrixConfig], lighting: &LightingConfig, pins: &impl Pins) -> Result<(), Error> {
    if hardware.split.offset() + hardware.local_positions(more) > MAX_KEYS {
        return Err(Error::TooManyKeys);
    }
    if hardware.split.link().is_some_and(|link| !pins.supports_link(&link)) {
        return Err(Error::Unsupported);
    }
    if lighting.pin.is_some_and(|pin| is_usable(pins, pin) && !pins.supports_lights(pin)) {
        return Err(Error::Unsupported);
    }
    check_pins(hardware.used_pins(more).chain(lighting.pin), &hardware.reserved_pins, pins)
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
