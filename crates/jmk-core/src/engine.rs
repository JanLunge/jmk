use heapless::Vec;

use crate::action::{Action, Basic, Combo, ComboOptions, HoldTapFlavor, HoldTapProfile, MacroStep};
use crate::combo::{ComboStage, Combos};
use crate::keycode::modifier_bit;
use crate::{Event, Millis, Source};

/// Keys a report can carry. Transports with a 6-key boot report truncate.
pub const MAX_REPORT_KEYS: usize = 16;
const MAX_HELD: usize = 32;
const QUEUE_LEN: usize = 16;
const MAX_MACRO_KEYS: usize = 8;
/// Presses remembered for [`Engine::take_resolved`] between two calls.
pub const MAX_RESOLVED: usize = 8;
/// Time between macro steps, so every report reaches the host in order.
const MACRO_STEP_MS: Millis = 2;

/// The keymap the engine reads from. Implemented by the firmware over its
/// flash-backed storage and by plain arrays in tests.
pub trait Keymap {
    fn layer_count(&self) -> u8;
    fn action(&self, layer: u8, pos: u16) -> Action;
    fn hold_tap_profile(&self, _profile: u8) -> HoldTapProfile {
        HoldTapProfile::default()
    }
    fn combos(&self) -> &[Combo] {
        &[]
    }
    /// By the index of the combo; one without an entry has the defaults.
    fn combo_options(&self) -> &[ComboOptions] {
        &[]
    }
    fn macro_steps(&self, _index: u8) -> &[MacroStep] {
        &[]
    }
    /// By the number of the layer: the layers (one bit each) that turn it on
    /// while all of them are on. Nothing, or 0, for a layer without such a
    /// rule.
    fn layers_with(&self) -> &[u8] {
        &[]
    }
}

/// Where the engine sends its results.
pub trait Output {
    /// The full keyboard state after a change.
    fn keyboard(&mut self, report: &KeyboardReport);
    /// A consumer usage was pressed, or released when `usage` is 0.
    fn consumer(&mut self, _usage: u16) {}
    /// A [`Basic::Custom`] action was pressed or released.
    fn custom(&mut self, _id: u16, _pressed: bool) {}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyboardReport {
    pub mods: u8,
    pub keys: Vec<u8, MAX_REPORT_KEYS>,
}

impl KeyboardReport {
    /// The 8-byte boot protocol report: modifiers, reserved, six keys.
    pub fn boot(&self) -> [u8; 8] {
        let mut out = [0; 8];
        out[0] = self.mods;
        for (slot, key) in out[2..].iter_mut().zip(self.keys.iter()) {
            *slot = *key;
        }
        out
    }
}

/// A key that is down, with the action it resolved to when pressed. Release
/// undoes exactly this action, whatever the layers look like by then.
#[derive(Clone, Copy)]
struct Held {
    src: Source,
    action: Basic,
}

/// A key press and the layer its action was taken from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub pos: u16,
    pub layer: u8,
}

/// A macro being played back.
#[derive(Clone, Copy)]
struct Playback {
    index: u8,
    step: u16,
    next: Millis,
    /// Key to release before the next step (second half of a tap).
    tapped: Option<u8>,
}

enum Decision {
    Tap,
    Hold,
    Wait,
}

#[derive(Default)]
pub struct Engine {
    combos: ComboStage,
    held: Vec<Held, MAX_HELD>,
    /// Events waiting behind an undecided hold-tap at the front.
    queue: Vec<Event, QUEUE_LEN>,
    toggled_layers: u32,
    oneshot_mods: u8,
    last_key_press: Option<Millis>,
    last_report: KeyboardReport,
    playback: Option<Playback>,
    /// Keys currently held down by the macro.
    macro_keys: Vec<u8, MAX_MACRO_KEYS>,
    /// Presses acted on since the last [`Engine::take_resolved`].
    resolved: Vec<Resolved, MAX_RESOLVED>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a physical key change. `pos` indexes the keymap.
    pub fn key_event(&mut self, keymap: &impl Keymap, out: &mut impl Output, pos: u16, pressed: bool, now: Millis) {
        let mut combos = core::mem::take(&mut self.combos);
        let layer = (31 - self.active_layers(keymap).leading_zeros()) as u8;
        combos.key_event(all_combos(keymap), layer, pos, pressed, now, &mut |ev| self.push(keymap, out, ev, now));
        self.combos = combos;
    }

    /// Call when [`Engine::next_deadline`] has passed.
    pub fn tick(&mut self, keymap: &impl Keymap, out: &mut impl Output, now: Millis) {
        let mut combos = core::mem::take(&mut self.combos);
        combos.tick(all_combos(keymap), now, &mut |ev| self.push(keymap, out, ev, now));
        self.combos = combos;
        self.process(keymap, out, now, false);
        self.play(keymap, out, now);
    }

    /// The earliest time at which [`Engine::tick`] has work to do.
    pub fn next_deadline(&self, keymap: &impl Keymap) -> Option<Millis> {
        let hold_tap = self.queue.first().and_then(|head| match self.resolve(keymap, head.src).0 {
            Action::HoldTap { profile, .. } if head.pressed => {
                Some(head.time + keymap.hold_tap_profile(profile).tapping_term_ms as Millis)
            }
            _ => None,
        });
        [hold_tap, self.combos.deadline(all_combos(keymap)), self.playback.map(|p| p.next)].into_iter().flatten().min()
    }

    /// Forget all held keys and layers and tell the host nothing is pressed.
    /// Used when the wiring changes underneath the engine.
    pub fn reset(&mut self, out: &mut impl Output) {
        let had_consumer = self.held.iter().any(|h| matches!(h.action, Basic::Consumer(_)));
        *self = Self { last_report: core::mem::take(&mut self.last_report), ..Self::default() };
        if had_consumer {
            out.consumer(0);
        }
        self.report(out);
    }

    /// Bitmask of active layers. Layer 0 is always active.
    pub fn active_layers(&self, keymap: &impl Keymap) -> u32 {
        let mut layers = 1 | self.toggled_layers;
        for held in &self.held {
            if let Basic::MomentaryLayer(layer) = held.action {
                layers |= layer_bit(layer);
            }
        }
        // Lowest first, so a layer turned on this way counts for the next.
        for (layer, with) in keymap.layers_with().iter().enumerate() {
            if *with != 0 && layers & *with as u32 == *with as u32 {
                layers |= layer_bit(layer as u8);
            }
        }
        layers
    }

    /// The key presses acted on since the last call, oldest first, each with
    /// the layer that supplied its action. A press waiting behind an
    /// undecided hold-tap appears once it is decided; keys swallowed by a
    /// combo never do. Only the newest [`MAX_RESOLVED`] are kept.
    pub fn take_resolved(&mut self) -> Vec<Resolved, MAX_RESOLVED> {
        core::mem::take(&mut self.resolved)
    }

    fn push(&mut self, keymap: &impl Keymap, out: &mut impl Output, ev: Event, now: Millis) {
        // A release whose press was already handled does not wait in the queue.
        let press_queued = self.queue.iter().any(|q| q.src == ev.src && q.pressed);
        if !ev.pressed && !press_queued {
            self.release(out, ev.src);
            return;
        }
        while self.queue.is_full() {
            self.process(keymap, out, now, true);
        }
        let _ = self.queue.push(ev);
        self.process(keymap, out, now, false);
    }

    /// Handle queued events in order until one cannot be decided yet.
    /// With `force`, the front event is decided as a hold if necessary.
    fn process(&mut self, keymap: &impl Keymap, out: &mut impl Output, now: Millis, mut force: bool) {
        while let Some(head) = self.queue.first().copied() {
            if !head.pressed {
                self.queue.remove(0);
                self.release(out, head.src);
                continue;
            }
            let (action, layer) = self.resolve(keymap, head.src);
            let action = match action {
                Action::Transparent => Basic::None,
                Action::Basic(basic) => basic,
                Action::HoldTap { hold, tap, profile } => {
                    match self.decide(&keymap.hold_tap_profile(profile), now) {
                        Decision::Tap => tap,
                        Decision::Hold => hold,
                        Decision::Wait if force => hold,
                        Decision::Wait => return,
                    }
                }
            };
            force = false;
            self.queue.remove(0);
            if let Source::Key(pos) = head.src {
                if self.resolved.is_full() {
                    self.resolved.remove(0);
                }
                let _ = self.resolved.push(Resolved { pos, layer });
            }
            self.press(out, head, action);
        }
    }

    /// Decide the hold-tap at the front of the queue from the events behind it.
    fn decide(&self, profile: &HoldTapProfile, now: Millis) -> Decision {
        let head = self.queue[0];
        let idle = profile.require_prior_idle_ms as Millis;
        if self.last_key_press.is_some_and(|last| head.time.saturating_sub(last) < idle) {
            return Decision::Tap;
        }
        let deadline = head.time + profile.tapping_term_ms as Millis;
        for ev in &self.queue[1..] {
            if ev.time >= deadline {
                return Decision::Hold;
            }
            match (ev.src == head.src, ev.pressed, profile.flavor) {
                (true, false, _) => return Decision::Tap,
                (false, true, HoldTapFlavor::HoldPreferred) => return Decision::Hold,
                // Only keys pressed after the hold-tap have their release queued.
                (false, false, HoldTapFlavor::Balanced) => return Decision::Hold,
                _ => {}
            }
        }
        if now >= deadline {
            Decision::Hold
        } else {
            Decision::Wait
        }
    }

    /// The action for an event and the layer it comes from: the highest
    /// active layer that is not see-through at that key.
    fn resolve(&self, keymap: &impl Keymap, src: Source) -> (Action, u8) {
        let pos = match src {
            Source::Key(pos) => pos,
            Source::Combo(index) => {
                return (keymap.combos().get(index as usize).map_or(Action::NONE, |c| c.action), 0);
            }
        };
        let active = self.active_layers(keymap);
        (0..keymap.layer_count().min(32))
            .rev()
            .filter(|layer| active & layer_bit(*layer) != 0)
            .map(|layer| (keymap.action(layer, pos), layer))
            .find(|(action, _)| *action != Action::Transparent)
            .unwrap_or((Action::NONE, 0))
    }

    fn press(&mut self, out: &mut impl Output, ev: Event, action: Basic) {
        // If the table is full the key is dropped; its release is then a no-op.
        if self.held.push(Held { src: ev.src, action }).is_err() {
            return;
        }
        match action {
            Basic::Key(_) | Basic::ModKey { .. } => {
                self.last_key_press = Some(ev.time);
                self.report(out);
                if self.oneshot_mods != 0 {
                    self.oneshot_mods = 0;
                    self.report(out);
                }
            }
            Basic::OneShotMods(mods) => {
                self.oneshot_mods |= mods;
                self.report(out);
            }
            Basic::Consumer(usage) => out.consumer(usage),
            Basic::ToggleLayer(layer) => self.toggled_layers ^= layer_bit(layer),
            Basic::ToLayer(layer) => self.toggled_layers = layer_bit(layer),
            Basic::Custom(id) => out.custom(id, true),
            Basic::Macro(index) => {
                // One macro at a time; a second one is ignored until it ends.
                if self.playback.is_none() {
                    self.playback = Some(Playback { index, step: 0, next: ev.time, tapped: None });
                }
            }
            Basic::Mods(_) => self.report(out),
            Basic::MomentaryLayer(_) | Basic::None => {}
        }
    }

    fn release(&mut self, out: &mut impl Output, src: Source) {
        let Some(index) = self.held.iter().position(|h| h.src == src) else { return };
        let action = self.held.remove(index).action;
        match action {
            Basic::Consumer(_) => out.consumer(0),
            Basic::Custom(id) => out.custom(id, false),
            _ => self.report(out),
        }
    }

    /// Run the next macro step if it is due.
    fn play(&mut self, keymap: &impl Keymap, out: &mut impl Output, now: Millis) {
        let Some(mut playback) = self.playback else { return };
        if now < playback.next {
            return;
        }
        playback.next = now + MACRO_STEP_MS;
        if let Some(key) = playback.tapped.take() {
            self.macro_keys.retain(|k| *k != key);
        } else {
            match keymap.macro_steps(playback.index).get(playback.step as usize) {
                None => {
                    self.macro_keys.clear();
                    self.playback = None;
                    return self.report(out);
                }
                Some(MacroStep::Tap(key)) => {
                    let _ = self.macro_keys.push(*key);
                    playback.tapped = Some(*key);
                }
                Some(MacroStep::Press(key)) => {
                    let _ = self.macro_keys.push(*key);
                }
                Some(MacroStep::Release(key)) => self.macro_keys.retain(|k| k != key),
                Some(MacroStep::DelayMs(ms)) => playback.next = now + *ms as Millis,
            }
            playback.step += 1;
        }
        self.playback = Some(playback);
        self.report(out);
    }

    /// Send the keyboard state if it differs from what the host last saw.
    fn report(&mut self, out: &mut impl Output) {
        let mut report = KeyboardReport { mods: self.oneshot_mods, keys: Vec::new() };
        let add_key = |report: &mut KeyboardReport, key: u8| match modifier_bit(key) {
            Some(bit) => report.mods |= bit,
            None if key != 0 && !report.keys.contains(&key) => {
                let _ = report.keys.push(key);
            }
            None => {}
        };
        for key in &self.macro_keys {
            add_key(&mut report, *key);
        }
        for held in &self.held {
            match held.action {
                Basic::Key(key) => add_key(&mut report, key),
                Basic::Mods(mods) | Basic::OneShotMods(mods) => report.mods |= mods,
                Basic::ModKey { mods, key } => {
                    report.mods |= mods;
                    add_key(&mut report, key);
                }
                _ => {}
            }
        }
        if report != self.last_report {
            out.keyboard(&report);
            self.last_report = report;
        }
    }
}

fn all_combos(keymap: &impl Keymap) -> Combos<'_> {
    Combos { combos: keymap.combos(), options: keymap.combo_options() }
}

fn layer_bit(layer: u8) -> u32 {
    1u32.checked_shl(layer as u32).unwrap_or(0)
}
