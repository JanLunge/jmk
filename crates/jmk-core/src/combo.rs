//! Combo detection. Sits in front of the main engine and either passes key
//! events through (possibly delayed, with their original timestamps) or
//! replaces a set of presses with one combo event.
//!
//! A combo is either a chord (its keys go down together, in any order) or a
//! sequence (its keys are pressed one after the other, in order, and may be
//! let go in between).

use heapless::Vec;

use crate::action::{Combo, ComboOptions, MAX_COMBO_KEYS};
use crate::{Event, Millis, Source};

const MAX_ACTIVE: usize = 4;
/// Every key of the longest sequence, pressed and let go again.
const MAX_PENDING: usize = 2 * MAX_COMBO_KEYS;

struct Active {
    index: u8,
    keys_down: Vec<u16, MAX_COMBO_KEYS>,
    released: bool,
}

#[derive(Clone, Copy)]
struct Change {
    pos: u16,
    pressed: bool,
    time: Millis,
}

/// The combos and, by the same index, what else is set for each. A combo
/// without an entry in `options` has the defaults.
#[derive(Clone, Copy)]
pub(crate) struct Combos<'a> {
    pub combos: &'a [Combo],
    pub options: &'a [ComboOptions],
}

#[derive(Default)]
pub(crate) struct ComboStage {
    /// Key changes held back because they may still become a combo. The
    /// first is always a press.
    pending: Vec<Change, MAX_PENDING>,
    /// The highest active layer when the first of them was pressed.
    layer: u8,
    active: Vec<Active, MAX_ACTIVE>,
}

impl ComboStage {
    /// `layer` is the highest active layer.
    pub fn key_event(
        &mut self,
        combos: Combos,
        layer: u8,
        pos: u16,
        pressed: bool,
        now: Millis,
        emit: &mut impl FnMut(Event),
    ) {
        if pressed {
            if self.pending.is_empty() {
                self.layer = layer;
            }
            self.press(combos, pos, now, emit);
        } else {
            self.release(combos, pos, now, emit);
        }
    }

    pub fn tick(&mut self, combos: Combos, now: Millis, emit: &mut impl FnMut(Event)) {
        let Some(deadline) = self.deadline(combos) else { return };
        if now >= deadline {
            self.resolve(combos, deadline, emit);
        }
    }

    /// When the held-back changes must be resolved at the latest.
    pub fn deadline(&self, combos: Combos) -> Option<Millis> {
        let first = self.pending.first()?.time;
        let latest = self.candidates(combos, None).map(|(_, c, o)| self.since(o, first) + c.timeout_ms as Millis).max();
        // Without a candidate there is nothing to wait for.
        Some(latest.unwrap_or(first))
    }

    fn presses(&self) -> impl Iterator<Item = &Change> + '_ {
        self.pending.iter().filter(|change| change.pressed)
    }

    /// Whether a held-back press of this key has no release after it.
    fn is_down(&self, pos: u16) -> bool {
        self.pending.iter().rev().find(|change| change.pos == pos).is_some_and(|change| change.pressed)
    }

    /// From when a combo's time runs: for a chord from its first key, for a
    /// sequence from the key before.
    fn since(&self, options: ComboOptions, now: Millis) -> Millis {
        let mut presses = self.presses();
        let press = if options.sequence { presses.last() } else { presses.next() };
        press.map_or(now, |change| change.time)
    }

    /// Whether the held-back presses, and `extra` after them, are part of
    /// this combo.
    fn fits(&self, combo: &Combo, options: ComboOptions, extra: Option<u16>) -> bool {
        if options.layers != 0 && 1u32.checked_shl(self.layer as u32).unwrap_or(0) & options.layers == 0 {
            return false;
        }
        if self.presses().count() + extra.is_some() as usize > combo.keys.len() {
            return false;
        }
        if options.sequence {
            return self.presses().map(|change| change.pos).chain(extra).zip(&combo.keys).all(|(pos, key)| pos == *key);
        }
        self.pending.iter().all(|change| change.pressed && combo.keys.contains(&change.pos))
            && extra.is_none_or(|pos| combo.keys.contains(&pos) && !self.is_down(pos))
    }

    fn candidates<'a>(
        &'a self,
        combos: Combos<'a>,
        extra: Option<u16>,
    ) -> impl Iterator<Item = (usize, &'a Combo, ComboOptions)> + 'a {
        combos.combos.iter().enumerate().filter_map(move |(index, combo)| {
            let options = combos.options.get(index).copied().unwrap_or_default();
            self.fits(combo, options, extra).then_some((index, combo, options))
        })
    }

    /// The combo that the held-back presses make up in full, if any.
    fn complete(&self, combos: Combos) -> Option<u8> {
        let count = self.presses().count();
        self.candidates(combos, None).find(|(_, combo, _)| combo.keys.len() == count).map(|(index, ..)| index as u8)
    }

    fn press(&mut self, combos: Combos, pos: u16, now: Millis, emit: &mut impl FnMut(Event)) {
        let fits = !self.pending.is_full()
            && self
                .candidates(combos, Some(pos))
                .any(|(_, combo, options)| now - self.since(options, now) <= combo.timeout_ms as Millis);

        if !fits {
            if self.pending.is_empty() {
                emit(Event { src: Source::Key(pos), pressed: true, time: now });
                return;
            }
            // The held-back keys cannot form a combo with this one. A complete
            // shorter combo still fires; otherwise they become normal presses.
            self.resolve(combos, now, emit);
            return self.press(combos, pos, now, emit);
        }

        let _ = self.pending.push(Change { pos, pressed: true, time: now });
        let count = self.presses().count();
        let longer_possible = self.candidates(combos, None).any(|(_, combo, _)| combo.keys.len() > count);
        if !longer_possible {
            if let Some(index) = self.complete(combos) {
                self.fire(index, now, emit);
            }
        }
    }

    fn release(&mut self, combos: Combos, pos: u16, now: Millis, emit: &mut impl FnMut(Event)) {
        if let Some(i) = self.active.iter().position(|a| a.keys_down.contains(&pos)) {
            let active = &mut self.active[i];
            active.keys_down.retain(|k| *k != pos);
            if !active.released {
                active.released = true;
                emit(Event { src: Source::Combo(active.index), pressed: false, time: now });
            }
            if active.keys_down.is_empty() {
                self.active.swap_remove(i);
            }
            return;
        }
        if self.is_down(pos) {
            // The keys of a sequence may be let go on the way; a chord ends here.
            let sequence = self.candidates(combos, None).any(|(.., options)| options.sequence);
            if sequence && self.pending.push(Change { pos, pressed: false, time: now }).is_ok() {
                return;
            }
            self.resolve(combos, now, emit);
            return self.release(combos, pos, now, emit);
        }
        emit(Event { src: Source::Key(pos), pressed: false, time: now });
    }

    /// Stop waiting: fire the combo that is complete, or pass on what was
    /// held back.
    fn resolve(&mut self, combos: Combos, now: Millis, emit: &mut impl FnMut(Event)) {
        match self.complete(combos) {
            Some(index) => self.fire(index, now, emit),
            None => self.flush(emit),
        }
    }

    fn fire(&mut self, index: u8, now: Millis, emit: &mut impl FnMut(Event)) {
        let mut keys_down: Vec<u16, MAX_COMBO_KEYS> = Vec::new();
        for change in &self.pending {
            keys_down.retain(|k| *k != change.pos);
            if change.pressed {
                let _ = keys_down.push(change.pos);
            }
        }
        self.pending.clear();
        let src = Source::Combo(index);
        if keys_down.is_empty() {
            // A sequence whose keys are all up again: a tap.
            emit(Event { src, pressed: true, time: now });
            emit(Event { src, pressed: false, time: now });
            return;
        }
        if self.active.push(Active { index, keys_down, released: false }).is_err() {
            return;
        }
        emit(Event { src, pressed: true, time: now });
    }

    fn flush(&mut self, emit: &mut impl FnMut(Event)) {
        for change in &self.pending {
            emit(Event { src: Source::Key(change.pos), pressed: change.pressed, time: change.time });
        }
        self.pending.clear();
    }
}
