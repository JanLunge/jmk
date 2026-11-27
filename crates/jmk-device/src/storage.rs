//! Saves the configuration to flash.
//!
//! Two slots are written alternately, so a power cut during a save leaves the
//! previous configuration intact. Each slot holds a header (magic, sequence
//! number, length, CRC-32) followed by the postcard-encoded configuration.

use embedded_storage::nor_flash::NorFlash;
use heapless::Vec;
use jmk_core::{Combo, ComboOptions};
use jmk_proto::{Doubles, HardwareConfig, LightingConfig, ScanConfig, MAX_COMBOS, MAX_KEYS};
use serde::{de::DeserializeOwned, Serialize};

use crate::config::Config;

const SLOT_SIZE: u32 = 16 * 1024;
/// Flash the storage region needs. Must start on an erase boundary.
pub const STORAGE_SIZE: u32 = 2 * SLOT_SIZE;
const HEADER: usize = 16;
const MAGIC: u32 = 0x314B_4D4A; // "JMK1"

pub struct Storage<F> {
    flash: F,
    /// Offset of the region within the flash driver's address space.
    base: u32,
    buf: &'static mut [u8; SLOT_SIZE as usize],
    /// Slot and sequence number of the newest valid save.
    current: Option<(u32, u32)>,
}

#[derive(Debug)]
pub struct StorageError;

impl<F: NorFlash> Storage<F> {
    pub fn new(flash: F, base: u32, buf: &'static mut [u8; SLOT_SIZE as usize]) -> Self {
        Self { flash, base, buf, current: None }
    }

    /// Load the newest valid save into `config`. Returns false if there is
    /// none; `config` then holds the defaults.
    pub fn load(&mut self, config: &mut Config) -> bool {
        let mut newest: Option<(u32, u32)> = None;
        for slot in 0..2 {
            if let Some(seq) = self.read_slot(slot) {
                if newest.is_none_or(|(_, s)| seq > s) {
                    newest = Some((slot, seq));
                }
            }
        }
        self.current = newest;
        let Some((slot, _)) = newest else { return false };
        // The buffer holds whichever slot was checked last, so read again.
        if self.read_slot(slot).is_none() {
            return false;
        }
        let len = u32::from_le_bytes(self.buf[8..12].try_into().unwrap()) as usize;
        let loaded = decode(&self.buf[HEADER..HEADER + len], config).is_some();
        if !loaded {
            // A save from an incompatible version may have been half applied.
            config.reset();
        }
        loaded
    }

    pub fn save(&mut self, config: &Config) -> Result<(), StorageError> {
        let (slot, seq) = match self.current {
            Some((slot, seq)) => (1 - slot, seq.wrapping_add(1)),
            None => (0, 1),
        };
        let (header, payload) = self.buf.split_at_mut(HEADER);
        let len = encode(config, payload).ok_or(StorageError)?;
        header[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        header[4..8].copy_from_slice(&seq.to_le_bytes());
        header[8..12].copy_from_slice(&(len as u32).to_le_bytes());
        header[12..16].copy_from_slice(&crc32(&payload[..len]).to_le_bytes());

        let start = self.base + slot * SLOT_SIZE;
        self.flash.erase(start, start + SLOT_SIZE).map_err(|_| StorageError)?;
        let total = (HEADER + len).next_multiple_of(F::WRITE_SIZE.max(4));
        self.flash.write(start, &self.buf[..total]).map_err(|_| StorageError)?;
        self.current = Some((slot, seq));
        Ok(())
    }

    /// Memory to work in between saves. A save overwrites it.
    pub(crate) fn scratch(&mut self) -> &mut [u8] {
        &mut self.buf[..]
    }

    pub fn erase(&mut self) -> Result<(), StorageError> {
        self.current = None;
        self.flash.erase(self.base, self.base + STORAGE_SIZE).map_err(|_| StorageError)
    }

    /// Read a slot into the buffer. Returns its sequence number if valid.
    fn read_slot(&mut self, slot: u32) -> Option<u32> {
        let start = self.base + slot * SLOT_SIZE;
        self.flash.read(start, &mut self.buf[..HEADER]).ok()?;
        let word = |i: usize| u32::from_le_bytes(self.buf[i..i + 4].try_into().unwrap());
        let (magic, seq, len, crc) = (word(0), word(4), word(8) as usize, word(12));
        if magic != MAGIC || len > self.buf.len() - HEADER {
            return None;
        }
        self.flash.read(start + HEADER as u32, &mut self.buf[HEADER..HEADER + len]).ok()?;
        (crc32(&self.buf[HEADER..HEADER + len]) == crc).then_some(seq)
    }
}

fn encode(config: &Config, buf: &mut [u8]) -> Option<usize> {
    fn put<T: Serialize>(buf: &mut [u8], used: &mut usize, value: &T) -> Option<()> {
        *used += postcard::to_slice(value, &mut buf[*used..]).ok()?.len();
        Some(())
    }
    let mut used = 0;
    put(buf, &mut used, &config.hardware)?;
    put(buf, &mut used, &config.layer_count)?;
    put(buf, &mut used, &config.profiles)?;
    put(buf, &mut used, &config.combos)?;
    put(buf, &mut used, &config.meta)?;
    put(buf, &mut used, &config.layout)?;
    put(buf, &mut used, &config.macro_ends)?;
    put(buf, &mut used, &config.macro_steps)?;
    for layer in &config.actions[..config.layer_count as usize] {
        for action in layer {
            put(buf, &mut used, action)?;
        }
    }
    // Added later, so it comes last: older saves simply end before it.
    put(buf, &mut used, &config.lighting)?;
    put(buf, &mut used, &config.layer_colors)?;
    put(buf, &mut used, &config.revision)?;
    put(buf, &mut used, &config.base)?;
    put(buf, &mut used, &config.macro_names)?;
    put(buf, &mut used, &config.letter_rows)?;
    put(buf, &mut used, &config.more_wiring)?;
    put(buf, &mut used, &config.combo_options)?;
    put(buf, &mut used, &config.doubles)?;
    put(buf, &mut used, &config.scan)?;
    Some(used)
}

fn decode(mut bytes: &[u8], config: &mut Config) -> Option<()> {
    let hardware: HardwareConfig = take(&mut bytes)?;
    let layer_count: u8 = take(&mut bytes)?;
    if layer_count == 0 || layer_count as usize > config.actions.len() {
        return None;
    }
    let profiles = take(&mut bytes)?;
    let combos = take(&mut bytes)?;
    config.meta = take(&mut bytes)?;
    config.layout = take(&mut bytes)?;
    config.macro_ends = take(&mut bytes)?;
    config.macro_steps = take(&mut bytes)?;
    for layer in &mut config.actions[..layer_count as usize] {
        for action in layer.iter_mut().take(MAX_KEYS) {
            *action = take(&mut bytes)?;
        }
    }
    config.lighting = if bytes.is_empty() { LightingConfig::NONE } else { take(&mut bytes)? };
    config.layer_colors = if bytes.is_empty() { Default::default() } else { take(&mut bytes)? };
    config.revision = if bytes.is_empty() { 0 } else { take(&mut bytes)? };
    config.base = if bytes.is_empty() { None } else { take(&mut bytes)? };
    if bytes.is_empty() {
        config.macro_names.iter_mut().for_each(|name| name.clear());
    } else {
        config.macro_names = take(&mut bytes)?;
    }
    config.letter_rows = if bytes.is_empty() { Default::default() } else { take(&mut bytes)? };
    config.more_wiring = if bytes.is_empty() { Default::default() } else { take(&mut bytes)? };
    let combos: Vec<Combo, MAX_COMBOS> = combos;
    let (combo_options, doubles, scan) = newest(bytes, combos.len()).or_else(|| from_0_5_0(bytes))?;
    config.combo_options = combo_options;
    config.doubles = doubles;
    config.scan = scan;
    config.hardware = hardware;
    config.layer_count = layer_count;
    config.profiles = profiles;
    config.combos = combos;
    Some(())
}

type Newest = (Vec<ComboOptions, MAX_COMBOS>, Doubles, ScanConfig);

fn take<T: DeserializeOwned>(bytes: &mut &[u8]) -> Option<T> {
    let (value, rest) = postcard::take_from_bytes(bytes).ok()?;
    *bytes = rest;
    Some(value)
}

/// Whether the firmware would accept these scan settings over USB.
fn scan_fits(scan: &ScanConfig) -> bool {
    let ascending = scan.inverted.windows(2).all(|pair| pair[0] < pair[1]);
    let fits = scan.inverted.last().is_none_or(|pos| (*pos as usize) < MAX_KEYS);
    ascending && fits && scan.settle_us <= ScanConfig::MAX_SETTLE_US
}

/// What a save holds after the extra wiring: the combo options and doubles,
/// which firmware 0.4.0 wrote together, then the scan settings. A save may
/// end before either.
fn newest(mut bytes: &[u8], combos: usize) -> Option<Newest> {
    let mut found = Newest::default();
    if !bytes.is_empty() {
        found.0 = take(&mut bytes)?;
        found.1 = take(&mut bytes)?;
    }
    if !bytes.is_empty() {
        found.2 = take(&mut bytes)?;
    }
    (found.0.len() <= combos && scan_fits(&found.2)).then_some(found)
}

/// Firmware 0.5.0, which only went onto boards used for development, wrote
/// the scan settings where the combo options are and nothing after them.
/// Such a save does not read as the newest form, except with exactly one
/// inverted key on a board that has a combo: that one loses the inverted
/// key and has to be set again.
fn from_0_5_0(mut bytes: &[u8]) -> Option<Newest> {
    let scan: ScanConfig = take(&mut bytes)?;
    (bytes.is_empty() && scan_fits(&scan)).then(|| (Vec::new(), Vec::new(), scan))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::boxed::Box;

    use jmk_core::Action;

    use super::*;

    fn sample() -> Box<Config> {
        let mut config = Box::new(Config::new());
        config.meta.name.push_str("sample").unwrap();
        config.combos.push(Combo { keys: [1, 2].into_iter().collect(), action: Action::NONE, timeout_ms: 40 }).unwrap();
        config.scan = ScanConfig { inverted: [4, 9, 14].into_iter().collect(), settle_us: 150, drive_idle_high: true };
        config
    }

    /// The bytes firmware 0.5.0 saved: the scan settings right after the extra wiring.
    fn saved_by_0_5_0(config: &Config, buf: &mut [u8]) -> usize {
        let mut plain = sample();
        plain.scan = ScanConfig::default();
        // Without options, doubles and scan settings the save ends in 0, 0 and 0, 0, 0.
        let head = encode(&plain, buf).unwrap() - 5;
        head + postcard::to_slice(&config.scan, &mut buf[head..]).unwrap().len()
    }

    #[test]
    fn a_save_from_0_5_0_keeps_everything() {
        let mut buf = std::vec![0u8; SLOT_SIZE as usize];
        for scan in [sample().scan.clone(), ScanConfig::default()] {
            let mut config = sample();
            config.scan = scan;
            let len = saved_by_0_5_0(&config, &mut buf);
            let mut loaded = Box::new(Config::new());
            assert!(decode(&buf[..len], &mut loaded).is_some());
            assert_eq!(loaded.scan, config.scan);
            assert_eq!(loaded.meta, config.meta);
            assert_eq!(loaded.combos, config.combos);
            assert!(loaded.combo_options.is_empty() && loaded.doubles.is_empty());
        }
    }

    #[test]
    fn the_newest_save_and_one_from_0_4_0_read_as_before() {
        let mut buf = std::vec![0u8; SLOT_SIZE as usize];
        let mut config = sample();
        config.combo_options.push(ComboOptions { layers: 5, sequence: true }).unwrap();
        config.doubles.push(7).unwrap();
        let len = encode(&config, &mut buf).unwrap();
        let mut loaded = Box::new(Config::new());
        assert!(decode(&buf[..len], &mut loaded).is_some());
        assert_eq!((&loaded.combo_options, &loaded.doubles, &loaded.scan), (&config.combo_options, &config.doubles, &config.scan));

        // 0.4.0 stopped after the doubles.
        let scan = postcard::to_slice(&config.scan, &mut [0u8; 64]).unwrap().len();
        let mut loaded = Box::new(Config::new());
        assert!(decode(&buf[..len - scan], &mut loaded).is_some());
        assert_eq!((&loaded.combo_options, &loaded.doubles), (&config.combo_options, &config.doubles));
        assert_eq!(loaded.scan, ScanConfig::default());
    }
}
