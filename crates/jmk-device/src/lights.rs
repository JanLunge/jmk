//! Works out what colour every light shows.

use jmk_core::Millis;
use jmk_proto::{ColorOrder, Effect, Led, LightZone, LightingConfig, MAX_LEDS};

use crate::pins::Pins;

/// Time between frames.
const FRAME_MS: Millis = 20;
/// An unchanged picture is sent again this often, for a chain that got its
/// power after the controller did.
const REPEAT_MS: Millis = 1000;
/// How much a pressed key's glow fades per frame, out of 255.
const FADE: u8 = 10;
/// Brightness of the one light shown by [`Lights::identify`].
const IDENTIFY: u8 = 90;

pub(crate) struct Lights {
    frame: [[u8; 3]; MAX_LEDS],
    /// Glow left on each light from a key press.
    heat: [u8; MAX_LEDS],
    next_frame: Millis,
    last_sent: Option<Millis>,
    identify: Option<u8>,
}

impl Lights {
    pub const fn new() -> Self {
        Self { frame: [[0; 3]; MAX_LEDS], heat: [0; MAX_LEDS], next_frame: 0, last_sent: None, identify: None }
    }

    /// Show only this light, or go back to the effects.
    pub fn identify(&mut self, led: Option<u8>) {
        self.identify = led;
        self.changed();
    }

    /// Send the next picture even if it looks the same.
    pub fn changed(&mut self) {
        self.last_sent = None;
    }

    pub fn key_pressed(&mut self, config: &LightingConfig, pos: u16) {
        for (heat, led) in self.heat.iter_mut().zip(&config.leds) {
            if led.key == Led::UNDERGLOW || led.key as u16 == pos {
                *heat = 255;
            }
        }
    }

    /// `clock` is the time the effects move by: the same on both halves of
    /// a split, so they move as one.
    pub fn tick(&mut self, config: &LightingConfig, now: Millis, clock: Millis, pins: &mut impl Pins) {
        if now < self.next_frame || config.leds.is_empty() {
            return;
        }
        self.next_frame = now + FRAME_MS;
        let count = config.leds.len();
        let mut different = false;
        for (index, led) in config.leds.iter().enumerate() {
            let zone = if led.key == Led::UNDERGLOW { &config.underglow } else { &config.keys };
            let rgb = match self.identify {
                Some(shown) if shown as usize == index => [IDENTIFY; 3],
                Some(_) => [0; 3],
                None => scale(color(zone, led, self.heat[index], clock), config.limit),
            };
            let wire = match config.order {
                ColorOrder::Grb => [rgb[1], rgb[0], rgb[2]],
                ColorOrder::Rgb => rgb,
            };
            different |= self.frame[index] != wire;
            self.frame[index] = wire;
            self.heat[index] = self.heat[index].saturating_sub(FADE);
        }
        if different || self.last_sent.is_none_or(|at| now >= at + REPEAT_MS) {
            self.last_sent = Some(now);
            pins.show_lights(&self.frame[..count]);
        }
    }
}

fn color(zone: &LightZone, led: &Led, heat: u8, now: Millis) -> [u8; 3] {
    // A full cycle takes about 26 s at speed 1 and 0.4 s at speed 255.
    let phase = (now * (zone.speed as Millis + 1) / 100) as u8;
    match zone.effect {
        Effect::Off => [0; 3],
        Effect::Solid => hsv(zone.hue, zone.sat, zone.val),
        Effect::Breathing => {
            // Up and down in a triangle, squared so the dark end lasts.
            let wave = if phase < 128 { phase * 2 } else { (255 - phase) * 2 } as u16;
            let level = 8 + wave * wave / 264;
            hsv(zone.hue, zone.sat, (zone.val as u16 * level.min(255) / 255) as u8)
        }
        Effect::Rainbow => hsv(zone.hue.wrapping_add(led.x).wrapping_sub(phase), zone.sat.max(1), zone.val),
        Effect::Reactive => hsv(zone.hue, zone.sat, (zone.val as u16 * heat as u16 / 255) as u8),
    }
}

fn scale(rgb: [u8; 3], limit: u8) -> [u8; 3] {
    rgb.map(|part| (part as u16 * limit as u16 / 255) as u8)
}

/// Hue, saturation and brightness (each 0..=255) as red, green and blue.
pub(crate) fn hsv(hue: u8, sat: u8, val: u8) -> [u8; 3] {
    if sat == 0 {
        return [val; 3];
    }
    // Six regions of 43 hue steps each.
    let region = hue / 43;
    let rest = (hue - region * 43) as u16 * 6;
    let (sat, val) = (sat as u16, val as u16);
    let p = (val * (255 - sat) / 255) as u8;
    let q = (val * (255 - sat * rest / 255) / 255) as u8;
    let t = (val * (255 - sat * (255 - rest) / 255) / 255) as u8;
    let val = val as u8;
    match region {
        0 => [val, t, p],
        1 => [q, val, p],
        2 => [p, val, t],
        3 => [p, q, val],
        4 => [t, p, val],
        _ => [val, p, q],
    }
}
