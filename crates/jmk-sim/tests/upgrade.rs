//! Settings saved by every earlier firmware still load.
//!
//! `saves/` holds what each firmware version wrote to flash for the same
//! settings (see `set_up`), produced by that version's own code. A new
//! version that changes what is saved adds its file here: run
//! `JMK_SAVE_OUT=crates/jmk-sim/tests/saves/<version>.bin cargo test -p jmk-sim --test upgrade write_save`
//! before the change is released, and list the file in `SAVES`.

use jmk_core::{Action, Basic, Combo, ComboOptions, HoldTapProfile, MacroStep};
use jmk_proto::{DiodeDirection, HardwareConfig, KeyGeometry, MatrixConfig, Meta, Request, Response, ScanConfig};
use jmk_sim::{VirtualKeyboard, Wiring};

/// What a save holds beyond the settings of the first firmware.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Knows {
    Basics,
    /// Layer colours, macro names, where the letters are.
    Names,
    ComboOptions,
}

const SAVES: [(&str, &[u8], Knows, bool); 5] = [
    ("the first firmware", include_bytes!("saves/protocol-1.bin"), Knows::Basics, false),
    ("0.2.0", include_bytes!("saves/0.2.0.bin"), Knows::Basics, false),
    ("0.3.0 (POG 2.0.0)", include_bytes!("saves/0.3.0.bin"), Knows::Names, false),
    ("0.4.0 (POG 2.1.0)", include_bytes!("saves/0.4.0.bin"), Knows::ComboOptions, false),
    // Only on boards used for development; it had scan settings but no combo options.
    ("0.5.0", include_bytes!("saves/0.5.0.bin"), Knows::Names, true),
];

fn board() -> VirtualKeyboard {
    VirtualKeyboard::new(Wiring::matrix(&[2, 3], &[4, 5], true), [7; 8])
}

fn ok(kb: &mut VirtualKeyboard, request: Request) {
    let what = format!("{request:?}");
    assert_eq!(kb.request(request), Response::Ok, "{what}");
}

/// The settings every save in `saves/` was made from, as far as its firmware
/// knew them.
fn set_up(kb: &mut VirtualKeyboard) {
    ok(kb, Request::SetHardware(HardwareConfig {
        matrix: MatrixConfig::Matrix {
            rows: [2, 3].into_iter().collect(),
            cols: [4, 5].into_iter().collect(),
            diodes: DiodeDirection::Col2Row,
        },
        reserved_pins: [25].into_iter().collect(),
        debounce_ms: 7,
        ..Default::default()
    }));
    ok(kb, Request::SetLayerCount(2));
    let key = |k| Action::Basic(Basic::Key(k));
    let hold_tap = Action::HoldTap { hold: Basic::Mods(2), tap: Basic::Key(6), profile: 1 };
    ok(kb, Request::SetActions { layer: 0, pos: 0, actions: [key(4), key(5), hold_tap, Action::Basic(Basic::MomentaryLayer(1))].into_iter().collect() });
    ok(kb, Request::SetActions { layer: 1, pos: 0, actions: [Action::Basic(Basic::Consumer(0xE9)), Action::Transparent, Action::Basic(Basic::Macro(0)), key(40)].into_iter().collect() });
    ok(kb, Request::SetHoldTapProfile { index: 1, profile: HoldTapProfile { tapping_term_ms: 170, ..Default::default() } });
    ok(kb, Request::SetCombos([Combo { keys: [0, 1].into_iter().collect(), action: key(41), timeout_ms: 45 }].into_iter().collect()));
    ok(kb, Request::SetMacro { index: 0, start: 0, steps: [MacroStep::Tap(11), MacroStep::DelayMs(20), MacroStep::Press(225), MacroStep::Tap(12), MacroStep::Release(225)].into_iter().collect() });
    ok(kb, Request::SetMeta(Meta { name: "Old Faithful".try_into().unwrap(), controller: "pico".try_into().unwrap() }));
    let at = |x, y| KeyGeometry { x, y, w: 100, h: 100, r: 0 };
    ok(kb, Request::SetLayout { start: 0, keys: [at(0, 0), at(100, 0), at(0, 100), KeyGeometry { x: 100, y: 100, w: 150, h: 100, r: 15 }].into_iter().collect() });
    {
        ok(kb, Request::SetLayerColors([3, 5].into_iter().collect()));
        ok(kb, Request::SetMacroName { index: 0, name: "Hello".try_into().unwrap() });
        ok(kb, Request::SetLetterRows([[0, 1].into_iter().collect(), [2, 3].into_iter().collect(), Default::default()]));
    }
    ok(kb, Request::SetComboOptions([ComboOptions { layers: 0b10, sequence: true }].into_iter().collect()));
}

fn scan() -> ScanConfig {
    ScanConfig { inverted: [1, 3].into_iter().collect(), settle_us: 120, drive_idle_high: true }
}

#[test]
fn saves_of_every_earlier_firmware_load_whole() {
    // What the newest firmware holds after the same settings were made on it.
    let mut fresh = board();
    set_up(&mut fresh);
    for (version, save, knows, scans) in SAVES {
        let mut kb = board();
        kb.flash().load(save);
        kb.reboot();
        let mut same = |request: Request| assert_eq!(kb.request(request.clone()), fresh.request(request.clone()), "{request:?} after a save from {version}");
        same(Request::GetHardware);
        same(Request::GetLayerCount);
        same(Request::GetActions { layer: 0, pos: 0 });
        same(Request::GetActions { layer: 1, pos: 0 });
        same(Request::GetHoldTapProfiles);
        same(Request::GetCombos);
        same(Request::GetMacro { index: 0, start: 0 });
        same(Request::GetMeta);
        same(Request::GetLayout { start: 0 });
        if knows >= Knows::Names {
            same(Request::GetLayerColors);
            same(Request::GetMacroName { index: 0 });
            same(Request::GetLetterRows);
        }
        let options = if knows >= Knows::ComboOptions { fresh.request(Request::GetComboOptions) } else { Response::ComboOptions(Default::default()) };
        assert_eq!(kb.request(Request::GetComboOptions), options, "combo options after a save from {version}");
        let expected = if scans { scan() } else { ScanConfig::default() };
        assert_eq!(kb.request(Request::GetScan), Response::Scan(expected), "scan settings after a save from {version}");
        assert_eq!(kb.request(Request::GetDoubles), Response::Doubles(Default::default()), "doubles after a save from {version}");

        // Saved again by this firmware, it still reads the same.
        assert_eq!(kb.request(Request::Save), Response::Ok);
        kb.reboot();
        assert_eq!(kb.request(Request::GetMeta), fresh.request(Request::GetMeta), "after {version} was saved again");
        assert_eq!(kb.request(Request::GetScan), Response::Scan(if scans { scan() } else { ScanConfig::default() }));
    }
}

/// Writes this firmware's save for `saves/`; does nothing unless asked.
#[test]
fn write_save() {
    let Ok(path) = std::env::var("JMK_SAVE_OUT") else { return };
    let mut kb = board();
    set_up(&mut kb);
    ok(&mut kb, Request::SetScan(scan()));
    ok(&mut kb, Request::Save);
    let bytes = kb.flash().bytes();
    let len = 16 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    std::fs::write(path, &bytes[..len]).unwrap();
}
