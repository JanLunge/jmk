//! Usage: jmk-uf2 <family> <firmware.elf> <firmware.uf2>
//!
//! Writes every loadable segment of a 32-bit little-endian ELF file as UF2
//! blocks of 256 bytes, which is what the UF2 bootloaders we target expect.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::{env, fs};

const PAGE: u32 = 256;

fn family_id(name: &str) -> Option<u32> {
    match name {
        "rp2040" => Some(0xE48B_FF56),
        "nrf52840" => Some(0xADA5_2840),
        _ => None,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let [_, family, input, output] = args.as_slice() else {
        eprintln!("usage: jmk-uf2 <rp2040|nrf52840> <firmware.elf> <firmware.uf2>");
        return ExitCode::FAILURE;
    };
    let Some(family) = family_id(family) else {
        eprintln!("unknown family {family}");
        return ExitCode::FAILURE;
    };
    let result = fs::read(input)
        .map_err(|e| format!("{input}: {e}"))
        .and_then(|elf| pages(&elf).ok_or_else(|| format!("{input}: not a 32-bit little-endian ELF file")))
        .and_then(|pages| fs::write(output, uf2(&pages, family)).map_err(|e| format!("{output}: {e}")));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Flash contents by page address, taken from the PT_LOAD segments.
fn pages(elf: &[u8]) -> Option<BTreeMap<u32, [u8; PAGE as usize]>> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(elf.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(elf.get(at..at + 4)?.try_into().ok()?));
    // Magic, 32-bit class, little-endian data.
    if elf.get(..6)? != b"\x7FELF\x01\x01" {
        return None;
    }
    let (table, entry_size, entries) = (u32_at(28)? as usize, u16_at(42)? as usize, u16_at(44)? as usize);
    let mut pages = BTreeMap::new();
    for header in (0..entries).map(|i| table + i * entry_size) {
        const PT_LOAD: u32 = 1;
        let (kind, offset, address, size) = (u32_at(header)?, u32_at(header + 4)?, u32_at(header + 12)?, u32_at(header + 16)?);
        if kind != PT_LOAD {
            continue;
        }
        let data = elf.get(offset as usize..(offset + size) as usize)?;
        for (i, byte) in data.iter().enumerate() {
            let at = address + i as u32;
            pages.entry(at - at % PAGE).or_insert([0; PAGE as usize])[(at % PAGE) as usize] = *byte;
        }
    }
    Some(pages)
}

fn uf2(pages: &BTreeMap<u32, [u8; PAGE as usize]>, family: u32) -> Vec<u8> {
    const FAMILY_ID_PRESENT: u32 = 0x2000;
    let mut out = Vec::with_capacity(pages.len() * 512);
    for (number, (address, data)) in pages.iter().enumerate() {
        let header = [0x0A32_4655, 0x9E5D_5157, FAMILY_ID_PRESENT, *address, PAGE, number as u32, pages.len() as u32, family];
        out.extend(header.iter().flat_map(|word| word.to_le_bytes()));
        out.extend_from_slice(data);
        out.resize(out.len() + 476 - data.len(), 0);
        out.extend_from_slice(&0x0AB1_6F30u32.to_le_bytes());
    }
    out
}
