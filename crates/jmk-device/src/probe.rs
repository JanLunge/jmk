//! Wiring detection for the setup wizard.
//!
//! Only the pins the user offered are touched, with weak pulls, and one pin at
//! a time driven low against pull-ups. No pin is ever driven high, so two
//! probed pins can never fight each other.

use heapless::Vec;
use jmk_proto::{Contact, Pin, MAX_CONTACTS};

use crate::pins::{Pins, Pull};

/// Report every connection currently present among `candidates`.
pub(crate) fn probe(candidates: &[Pin], pins: &mut impl Pins) -> Vec<Contact, MAX_CONTACTS> {
    let mut contacts: Vec<Contact, MAX_CONTACTS> = Vec::new();
    let mut fixed = 0u64;

    for (pull, wanted_level) in [(Pull::Down, true), (Pull::Up, false)] {
        for pin in candidates {
            pins.input(*pin, pull);
        }
        pins.settle();
        for pin in candidates {
            if fixed & (1 << pin) == 0 && pins.read(*pin) == wanted_level {
                fixed |= 1 << pin;
                let contact = if wanted_level { Contact::Supply(*pin) } else { Contact::Ground(*pin) };
                let _ = contacts.push(contact);
            }
        }
    }

    // All candidates now idle high through their pull-ups.
    let free = |pin: &&Pin| fixed & (1 << **pin) == 0;
    for driven in candidates.iter().filter(free) {
        pins.drive_low(*driven);
        pins.settle();
        for sensed in candidates.iter().filter(free) {
            if sensed != driven && !pins.read(*sensed) {
                let _ = contacts.push(Contact::Link { driven: *driven, sensed: *sensed });
            }
        }
        pins.input(*driven, Pull::Up);
        pins.settle();
    }
    contacts
}
