//! What goes over USB and into flash keeps its place.
//!
//! A message is sent as numbers, not names: the 5th kind of request is sent
//! as 4, and the 3rd part of a struct comes third. Firmware and app of
//! different versions only understand each other while nothing that was
//! released changes its number. `wire.txt` lists every kind and every part in
//! that order, and this test compares it with the code:
//!
//! - an enum may get new kinds at its end, and only there;
//! - a struct stays as it is. One that needs more gets a new request, like
//!   `MoreWiring` beside `HardwareConfig`.
//!
//! Two branches that both add to the end of the same enum do not agree on
//! the numbers. Whichever is merged second moves its additions behind the
//! first one's, and takes the next protocol version.
//!
//! After adding kinds, bring the list up to date and commit it:
//! `JMK_WIRE_OUT=crates/jmk-sim/tests/wire.txt cargo test -p jmk-sim --test wire`
//! The diff of `wire.txt` should then show added words at line ends and new
//! lines, nothing else.

use jmk_core::{Action, Basic, Combo, ComboOptions, HoldTapFlavor, HoldTapProfile, MacroStep};
use jmk_proto::{
    ColorOrder, Contact, DeviceMessage, DiodeDirection, Effect, EncoderConfig, Error, Event, HardwareConfig, Info, KeyGeometry, Led, LightZone,
    LightingConfig, LinkConfig, MatrixConfig, Mcu, Meta, Request, Response, SplitConfig, SplitStatus, SplitSync,
};
use serde::de::{value::Error as Noted, Deserialize, Deserializer, Error as _, Visitor};

/// Reads nothing; notes the names a type asks for.
struct Names<'a>(&'a mut Shape);

#[derive(Debug, PartialEq)]
enum Shape {
    Unknown,
    Enum(&'static [&'static str]),
    Struct(&'static [&'static str]),
}

impl<'de> Deserializer<'de> for Names<'_> {
    type Error = Noted;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Noted> {
        Err(Noted::custom("not an enum or a struct"))
    }

    fn deserialize_enum<V: Visitor<'de>>(self, _: &'static str, variants: &'static [&'static str], _: V) -> Result<V::Value, Noted> {
        *self.0 = Shape::Enum(variants);
        Err(Noted::custom("noted"))
    }

    fn deserialize_struct<V: Visitor<'de>>(self, _: &'static str, fields: &'static [&'static str], _: V) -> Result<V::Value, Noted> {
        *self.0 = Shape::Struct(fields);
        Err(Noted::custom("noted"))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map identifier ignored_any
    }
}

fn shape<T: for<'de> Deserialize<'de>>() -> Shape {
    let mut shape = Shape::Unknown;
    let _ = T::deserialize(Names(&mut shape));
    shape
}

/// Every enum and struct that is sent or saved. A new one is added here.
fn types() -> Vec<(&'static str, Shape)> {
    macro_rules! list {
        ($($ty:ident),* $(,)?) => { vec![$((stringify!($ty), shape::<$ty>())),*] };
    }
    list![
        // Messages.
        Request, Response, Error, Event, DeviceMessage,
        // What they carry.
        Info, Mcu, HardwareConfig, MatrixConfig, DiodeDirection, EncoderConfig, SplitConfig, LinkConfig,
        SplitStatus, SplitSync, Contact, LightingConfig, LightZone, Led, ColorOrder, Effect, KeyGeometry, Meta,
        // The keymap.
        Action, Basic, HoldTapProfile, HoldTapFlavor, Combo, ComboOptions, MacroStep,
    ]
}

fn written() -> String {
    types()
        .into_iter()
        .map(|(name, shape)| match shape {
            Shape::Enum(names) => format!("enum {name}: {}\n", names.join(" ")),
            Shape::Struct(names) => format!("struct {name}: {}\n", names.join(" ")),
            Shape::Unknown => panic!("{name} is neither an enum nor a struct"),
        })
        .collect()
}

#[test]
fn nothing_released_changed_its_place() {
    let now = written();
    if let Ok(path) = std::env::var("JMK_WIRE_OUT") {
        std::fs::write(path, &now).unwrap();
        return;
    }
    let now: Vec<&str> = now.lines().collect();
    let mut problems = Vec::new();
    for line in include_str!("wire.txt").lines() {
        let (kind, listed) = line.split_once(": ").expect("a line of wire.txt has no names");
        let Some(current) = now.iter().find_map(|each| each.strip_prefix(kind).and_then(|rest| rest.strip_prefix(": "))) else {
            problems.push(format!("{kind} is gone, or no longer listed in this test"));
            continue;
        };
        let (listed, current): (Vec<&str>, Vec<&str>) = (listed.split(' ').collect(), current.split(' ').collect());
        if kind.starts_with("struct") {
            if listed != current {
                problems.push(format!("{kind} changed: it was {listed:?} and is {current:?}"));
            }
        } else if !current.starts_with(&listed) {
            let at = listed.iter().zip(&current).position(|(a, b)| a != b).unwrap_or(current.len());
            problems.push(format!(
                "{kind}: number {at} was {} and is now {}; new kinds go at the end",
                listed[at],
                current.get(at).unwrap_or(&"missing")
            ));
        }
    }
    assert!(problems.is_empty(), "the protocol changed where it must not:\n{}\nSee the top of this file.", problems.join("\n"));
}
