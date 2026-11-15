//! Bluetooth bonds in flash: for each host profile, the keys that let a
//! paired host reconnect.
//!
//! Like the configuration, the table is written to two flash pages in turn,
//! so a power cut during a save leaves the previous table intact. A page
//! holds a header (magic, sequence number, CRC-32 of the slots) followed by
//! one fixed-size slot per profile.

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use nrf_mpsl::FlashError;
use trouble_host::prelude::{AddrKind, BdAddr, SecurityLevel};
use trouble_host::{Address, BondInformation, Identity, IdentityResolvingKey, LongTermKey};

use crate::flash::{Flash, BONDS_OFFSET, BONDS_SIZE};

/// Hosts the keyboard can stay bonded to. Raising this makes room in the
/// table; choosing between profiles is not implemented.
pub const PROFILES: usize = 1;

const PAGE_SIZE: u32 = BONDS_SIZE / 2;
const MAGIC: u32 = 0x424B_4D4A; // "JMKB"
const HEADER: usize = 12;
const SLOT: usize = 44;
const TABLE: usize = HEADER + PROFILES * SLOT;

const _: () = assert!(TABLE <= PAGE_SIZE as usize && TABLE % 4 == 0);

pub struct Bonds<'a> {
    flash: &'a Flash,
    profiles: [Option<BondInformation>; PROFILES],
    /// Page and sequence number of the newest valid table.
    current: Option<(u32, u32)>,
}

impl<'a> Bonds<'a> {
    /// Read the newest valid table; without one, no profile is bonded.
    pub fn load(mut flash: &'a Flash) -> Self {
        let mut newest: Option<(u32, u32, [u8; TABLE])> = None;
        for page in 0..2 {
            let mut table = [0u8; TABLE];
            if flash.read(BONDS_OFFSET + page * PAGE_SIZE, &mut table).is_err() {
                continue;
            }
            let word = |i: usize| u32::from_le_bytes(table[i..i + 4].try_into().unwrap());
            let (magic, seq, crc) = (word(0), word(4), word(8));
            let valid = magic == MAGIC && crc == crc32(&table[HEADER..]);
            if valid && newest.as_ref().is_none_or(|(_, newest_seq, _)| seq > *newest_seq) {
                newest = Some((page, seq, table));
            }
        }
        let mut profiles = [const { None }; PROFILES];
        let Some((page, seq, table)) = newest else {
            return Self { flash, profiles, current: None };
        };
        for (profile, slot) in profiles.iter_mut().zip(table[HEADER..].chunks_exact(SLOT)) {
            *profile = decode(slot.try_into().unwrap());
        }
        Self { flash, profiles, current: Some((page, seq)) }
    }

    pub fn profiles(&self) -> &[Option<BondInformation>; PROFILES] {
        &self.profiles
    }

    /// Store a profile's bond and write the table to flash. This blocks
    /// for as long as erasing one page takes (about 0.1 s, longer while a
    /// connection is active).
    pub fn set(&mut self, profile: usize, bond: BondInformation) -> Result<(), FlashError> {
        self.profiles[profile] = Some(bond);
        let (page, seq) = match self.current {
            Some((page, seq)) => (1 - page, seq.wrapping_add(1)),
            None => (0, 1),
        };
        let mut table = [0u8; TABLE];
        for (profile, slot) in self.profiles.iter().zip(table[HEADER..].chunks_exact_mut(SLOT)) {
            if let Some(bond) = profile {
                encode(bond, slot.try_into().unwrap());
            }
        }
        let crc = crc32(&table[HEADER..]);
        table[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        table[4..8].copy_from_slice(&seq.to_le_bytes());
        table[8..12].copy_from_slice(&crc.to_le_bytes());

        let start = BONDS_OFFSET + page * PAGE_SIZE;
        let mut flash = self.flash;
        flash.erase(start, start + PAGE_SIZE)?;
        flash.write(start, &table)?;
        self.current = Some((page, seq));
        Ok(())
    }
}

// Slot layout: used (1), address kind, address (6), has IRK, security level,
// two unused bytes, IRK (16), LTK (16).

fn encode(bond: &BondInformation, slot: &mut [u8; SLOT]) {
    slot[0] = 1;
    slot[1] = bond.identity.addr.kind.into_inner();
    slot[2..8].copy_from_slice(&bond.identity.addr.addr.into_inner());
    if let Some(irk) = bond.identity.irk {
        slot[8] = 1;
        slot[12..28].copy_from_slice(&irk.to_le_bytes());
    }
    slot[9] = match bond.security_level {
        SecurityLevel::NoEncryption => 0,
        SecurityLevel::Encrypted => 1,
        SecurityLevel::EncryptedAuthenticated => 2,
    };
    slot[28..44].copy_from_slice(&bond.ltk.to_le_bytes());
}

fn decode(slot: &[u8; SLOT]) -> Option<BondInformation> {
    if slot[0] != 1 {
        return None;
    }
    let addr = Address::new(AddrKind::new(slot[1]), BdAddr::new(slot[2..8].try_into().unwrap()));
    let irk = match slot[8] {
        0 => None,
        _ => Some(IdentityResolvingKey::from_le_bytes(slot[12..28].try_into().unwrap())?),
    };
    let security_level = match slot[9] {
        1 => SecurityLevel::Encrypted,
        2 => SecurityLevel::EncryptedAuthenticated,
        _ => return None,
    };
    let ltk = LongTermKey::from_le_bytes(slot[28..44].try_into().unwrap());
    Some(BondInformation::new(Identity { addr, irk }, ltk, security_level, true))
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
