//! Combo detection. Sits in front of the main engine and either passes key
//! events through (possibly delayed, with their original timestamps) or
//! replaces a set of presses with one combo event.

use heapless::Vec;

use crate::action::{Combo, MAX_COMBO_KEYS};
use crate::{Event, Millis, Source};

const MAX_ACTIVE: usize = 4;

struct Active {
    index: u8,
    keys_down: Vec<u16, MAX_COMBO_KEYS>,
    released: bool,
}

#[derive(Default)]
pub(crate) struct ComboStage {
    /// Presses held back because they may still become a combo.
    pending: Vec<(u16, Millis), MAX_COMBO_KEYS>,
    active: Vec<Active, MAX_ACTIVE>,
}

impl ComboStage {
    pub fn key_event(
        &mut self,
        combos: &[Combo],
        pos: u16,
        pressed: bool,
        now: Millis,
        emit: &mut impl FnMut(Event),
    ) {
        if pressed {
            self.press(combos, pos, now, emit);
        } else {
            self.release(pos, now, emit);
        }
    }

    pub fn tick(&mut self, combos: &[Combo], now: Millis, emit: &mut impl FnMut(Event)) {
        let Some(deadline) = self.deadline(combos) else { return };
        if now < deadline {
            return;
        }
        match self.complete(combos) {
            Some(index) => self.fire(index, deadline, emit),
            None => self.flush(emit),
        }
    }

    /// When the pending presses must be resolved at the latest.
    pub fn deadline(&self, combos: &[Combo]) -> Option<Millis> {
        let first = self.pending.first()?.1;
        let timeout = self.candidates(combos, None).map(|(_, c)| c.timeout_ms).max()?;
        Some(first + timeout as Millis)
    }

    /// Combos that contain every pending key (plus `extra`).
    fn candidates<'a>(
        &'a self,
        combos: &'a [Combo],
        extra: Option<u16>,
    ) -> impl Iterator<Item = (usize, &'a Combo)> + 'a {
        combos.iter().enumerate().filter(move |(_, c)| {
            self.pending.iter().all(|(p, _)| c.keys.contains(p))
                && extra.is_none_or(|p| c.keys.contains(&p))
        })
    }

    /// The combo whose keys are exactly the pending ones, if any.
    fn complete(&self, combos: &[Combo]) -> Option<u8> {
        self.candidates(combos, None)
            .find(|(_, c)| c.keys.len() == self.pending.len())
            .map(|(i, _)| i as u8)
    }

    fn press(&mut self, combos: &[Combo], pos: u16, now: Millis, emit: &mut impl FnMut(Event)) {
        let first = self.pending.first().map_or(now, |p| p.1);
        let fits = !self.pending.is_full()
            && self
                .candidates(combos, Some(pos))
                .any(|(_, c)| now - first <= c.timeout_ms as Millis);

        if !fits {
            if self.pending.is_empty() {
                emit(Event { src: Source::Key(pos), pressed: true, time: now });
                return;
            }
            // The held-back keys cannot form a combo with this one. A complete
            // shorter combo still fires; otherwise they become normal presses.
            match self.complete(combos) {
                Some(index) => self.fire(index, now, emit),
                None => self.flush(emit),
            }
            return self.press(combos, pos, now, emit);
        }

        let _ = self.pending.push((pos, now));
        let longer_possible = self.candidates(combos, None).any(|(_, c)| c.keys.len() > self.pending.len());
        if !longer_possible {
            if let Some(index) = self.complete(combos) {
                self.fire(index, now, emit);
            }
        }
    }

    fn release(&mut self, pos: u16, now: Millis, emit: &mut impl FnMut(Event)) {
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
        if self.pending.iter().any(|p| p.0 == pos) {
            self.flush(emit);
        }
        emit(Event { src: Source::Key(pos), pressed: false, time: now });
    }

    fn fire(&mut self, index: u8, now: Millis, emit: &mut impl FnMut(Event)) {
        let keys_down = self.pending.iter().map(|p| p.0).collect();
        self.pending.clear();
        if self.active.push(Active { index, keys_down, released: false }).is_err() {
            return;
        }
        emit(Event { src: Source::Combo(index), pressed: true, time: now });
    }

    fn flush(&mut self, emit: &mut impl FnMut(Event)) {
        for (pos, time) in self.pending.iter() {
            emit(Event { src: Source::Key(*pos), pressed: true, time: *time });
        }
        self.pending.clear();
    }
}
