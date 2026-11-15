use jmk_core::keycode::{self as kc, mods};
use jmk_core::{Action, Basic, Combo, Engine, HoldTapFlavor, HoldTapProfile, KeyboardReport, Keymap, MacroStep, Output};

struct Map {
    layers: Vec<Vec<Action>>,
    profile: HoldTapProfile,
    combos: Vec<Combo>,
    macros: Vec<Vec<MacroStep>>,
}

impl Keymap for Map {
    fn macro_steps(&self, index: u8) -> &[MacroStep] {
        self.macros.get(index as usize).map_or(&[], |m| m)
    }
    fn layer_count(&self) -> u8 {
        self.layers.len() as u8
    }
    fn action(&self, layer: u8, pos: u16) -> Action {
        self.layers[layer as usize].get(pos as usize).copied().unwrap_or_default()
    }
    fn hold_tap_profile(&self, _: u8) -> HoldTapProfile {
        self.profile
    }
    fn combos(&self) -> &[Combo] {
        &self.combos
    }
}

/// Records output as readable strings: "mods:keys" per keyboard report.
#[derive(Default)]
struct Log(Vec<String>);

impl Output for Log {
    fn keyboard(&mut self, report: &KeyboardReport) {
        let keys: Vec<String> = report.keys.iter().map(|k| format!("{k:02x}")).collect();
        self.0.push(format!("{:02x}:{}", report.mods, keys.join(",")));
    }
    fn consumer(&mut self, usage: u16) {
        self.0.push(format!("consumer {usage:x}"));
    }
    fn custom(&mut self, id: u16, pressed: bool) {
        self.0.push(format!("custom {id} {pressed}"));
    }
}

struct Rig {
    map: Map,
    engine: Engine,
    log: Log,
}

impl Rig {
    fn new(layers: Vec<Vec<Action>>) -> Self {
        let map = Map { layers, profile: HoldTapProfile::default(), combos: vec![], macros: vec![] };
        Rig { map, engine: Engine::new(), log: Log::default() }
    }
    fn flavor(mut self, flavor: HoldTapFlavor) -> Self {
        self.map.profile.flavor = flavor;
        self
    }
    fn press(&mut self, pos: u16, t: u64) {
        self.engine.key_event(&self.map, &mut self.log, pos, true, t);
    }
    fn release(&mut self, pos: u16, t: u64) {
        self.engine.key_event(&self.map, &mut self.log, pos, false, t);
    }
    fn tick(&mut self, t: u64) {
        self.engine.tick(&self.map, &mut self.log, t);
    }
    fn out(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log.0)
    }
}

fn key(k: u8) -> Action {
    Action::key(k)
}
fn basic(b: Basic) -> Action {
    Action::Basic(b)
}
/// Left shift when held, `tap` when tapped.
fn shift_or(tap: u8) -> Action {
    Action::HoldTap { hold: Basic::Mods(mods::LSHIFT), tap: Basic::Key(tap), profile: 0 }
}

#[test]
fn plain_keys_and_rollover() {
    let mut rig = Rig::new(vec![vec![key(kc::A), key(kc::B)]]);
    rig.press(0, 0);
    rig.press(1, 10);
    rig.release(0, 20);
    rig.release(1, 30);
    assert_eq!(rig.out(), ["00:04", "00:04,05", "00:05", "00:"]);
}

#[test]
fn modifier_usage_goes_into_mod_byte() {
    let mut rig = Rig::new(vec![vec![key(kc::LEFT_SHIFT), basic(Basic::ModKey { mods: mods::LCTRL, key: kc::C })]]);
    rig.press(0, 0);
    rig.press(1, 1);
    rig.release(1, 2);
    rig.release(0, 3);
    assert_eq!(rig.out(), ["02:", "03:06", "02:", "00:"]);
}

#[test]
fn momentary_layer_and_transparent() {
    let mut rig = Rig::new(vec![
        vec![basic(Basic::MomentaryLayer(1)), key(kc::A), key(kc::B)],
        vec![Action::Transparent, key(kc::N1), Action::Transparent],
    ]);
    rig.press(0, 0);
    rig.press(1, 1);
    rig.press(2, 2);
    assert_eq!(rig.out(), ["00:1e", "00:1e,05"]);
    // Releasing the layer key first must still release the layer-1 key.
    rig.release(0, 3);
    rig.release(1, 4);
    rig.release(2, 5);
    assert_eq!(rig.out(), ["00:05", "00:"]);
}

#[test]
fn toggle_and_to_layer() {
    let mut rig = Rig::new(vec![
        vec![basic(Basic::ToggleLayer(1)), key(kc::A), basic(Basic::ToLayer(2))],
        vec![Action::Transparent, key(kc::B), Action::Transparent],
        vec![basic(Basic::ToLayer(0)), key(kc::C), Action::Transparent],
    ]);
    let tap = |rig: &mut Rig, pos| {
        rig.press(pos, 0);
        rig.release(pos, 0);
    };
    tap(&mut rig, 0);
    tap(&mut rig, 1);
    tap(&mut rig, 0);
    tap(&mut rig, 1);
    assert_eq!(rig.out(), ["00:05", "00:", "00:04", "00:"]);
    tap(&mut rig, 2);
    assert_eq!(rig.engine.active_layers(), 0b101);
    tap(&mut rig, 1);
    tap(&mut rig, 0);
    assert_eq!(rig.engine.active_layers(), 0b001);
    assert_eq!(rig.out(), ["00:06", "00:"]);
}

#[test]
fn hold_tap_taps_when_released_in_time() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A)]]);
    rig.press(0, 0);
    assert!(rig.out().is_empty());
    assert_eq!(rig.engine.next_deadline(&rig.map), Some(200));
    rig.release(0, 100);
    assert_eq!(rig.out(), ["00:04", "00:"]);
    assert_eq!(rig.engine.next_deadline(&rig.map), None);
}

#[test]
fn hold_tap_holds_after_tapping_term() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A), key(kc::B)]]);
    rig.press(0, 0);
    rig.tick(199);
    assert!(rig.out().is_empty());
    rig.tick(200);
    assert_eq!(rig.out(), ["02:"]);
    rig.press(1, 250);
    rig.release(1, 260);
    rig.release(0, 300);
    assert_eq!(rig.out(), ["02:05", "02:", "00:"]);
}

#[test]
fn hold_tap_release_after_term_is_hold_even_without_tick() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A)]]);
    rig.press(0, 0);
    rig.release(0, 300);
    assert_eq!(rig.out(), ["02:", "00:"]);
}

#[test]
fn balanced_holds_on_nested_tap_but_not_on_roll() {
    let layers = vec![vec![shift_or(kc::A), key(kc::B)]];

    // Nested: other key pressed and released inside -> Shift+B.
    let mut rig = Rig::new(layers.clone());
    rig.press(0, 0);
    rig.press(1, 50);
    assert!(rig.out().is_empty());
    rig.release(1, 100);
    assert_eq!(rig.out(), ["02:", "02:05", "02:"]);
    rig.release(0, 120);
    assert_eq!(rig.out(), ["00:"]);

    // Roll: hold-tap released first -> "ab".
    let mut rig = Rig::new(layers);
    rig.press(0, 0);
    rig.press(1, 50);
    rig.release(0, 80);
    rig.release(1, 100);
    assert_eq!(rig.out(), ["00:04", "00:04,05", "00:05", "00:"]);
}

#[test]
fn hold_preferred_holds_on_other_press() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A), key(kc::B)]]).flavor(HoldTapFlavor::HoldPreferred);
    rig.press(0, 0);
    rig.press(1, 50);
    assert_eq!(rig.out(), ["02:", "02:05"]);
}

#[test]
fn tap_preferred_ignores_other_keys() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A), key(kc::B)]]).flavor(HoldTapFlavor::TapPreferred);
    rig.press(0, 0);
    rig.press(1, 50);
    rig.release(1, 100);
    assert!(rig.out().is_empty());
    rig.release(0, 150);
    assert_eq!(rig.out(), ["00:04", "00:04,05", "00:04", "00:"]);
}

#[test]
fn key_held_before_hold_tap_releases_immediately() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A), key(kc::B)]]);
    rig.press(1, 0);
    rig.press(0, 10);
    rig.release(1, 20);
    assert_eq!(rig.out(), ["00:05", "00:"]);
    rig.release(0, 30);
    assert_eq!(rig.out(), ["00:04", "00:"]);
}

#[test]
fn layer_tap_applies_layer_to_queued_keys() {
    let layer_tap = Action::HoldTap { hold: Basic::MomentaryLayer(1), tap: Basic::Key(kc::SPACE), profile: 0 };
    let mut rig = Rig::new(vec![vec![layer_tap, key(kc::A)], vec![Action::Transparent, key(kc::N1)]]);
    rig.press(0, 0);
    rig.press(1, 50);
    rig.release(1, 100);
    rig.release(0, 150);
    assert_eq!(rig.out(), ["00:1e", "00:"]);
}

#[test]
fn two_home_row_mods_stack() {
    let ctrl_or = Action::HoldTap { hold: Basic::Mods(mods::LCTRL), tap: Basic::Key(kc::D), profile: 0 };
    let mut rig = Rig::new(vec![vec![shift_or(kc::A), ctrl_or, key(kc::B)]]);
    rig.press(0, 0);
    rig.press(1, 20);
    rig.press(2, 40);
    rig.release(2, 60);
    assert_eq!(rig.out(), ["02:", "03:", "03:05", "03:"]);
}

#[test]
fn require_prior_idle_forces_tap_while_typing() {
    let mut rig = Rig::new(vec![vec![shift_or(kc::A), key(kc::B)]]);
    rig.map.profile.require_prior_idle_ms = 150;
    rig.map.profile.flavor = HoldTapFlavor::HoldPreferred;
    rig.press(1, 0);
    rig.release(1, 30);
    rig.press(0, 60);
    assert_eq!(rig.out(), ["00:05", "00:", "00:04"]);
}

#[test]
fn one_shot_mod_applies_to_next_key_only() {
    let mut rig = Rig::new(vec![vec![basic(Basic::OneShotMods(mods::LSHIFT)), key(kc::A)]]);
    rig.press(0, 0);
    rig.release(0, 10);
    assert_eq!(rig.out(), ["02:"]);
    rig.press(1, 20);
    rig.release(1, 30);
    rig.press(1, 40);
    assert_eq!(rig.out(), ["02:04", "00:04", "00:", "00:04"]);
}

#[test]
fn consumer_and_custom_actions() {
    let mut rig = Rig::new(vec![vec![basic(Basic::Consumer(0xE9)), basic(Basic::Custom(7))]]);
    rig.press(0, 0);
    rig.release(0, 1);
    rig.press(1, 2);
    rig.release(1, 3);
    assert_eq!(rig.out(), ["consumer e9", "consumer 0", "custom 7 true", "custom 7 false"]);
}

fn combo(keys: &[u16], action: Action) -> Combo {
    Combo { keys: keys.iter().copied().collect(), action, timeout_ms: 50 }
}

#[test]
fn combo_fires_and_releases_once() {
    let mut rig = Rig::new(vec![vec![key(kc::J), key(kc::K), key(kc::A)]]);
    rig.map.combos = vec![combo(&[0, 1], key(kc::ESCAPE))];
    rig.press(0, 0);
    assert!(rig.out().is_empty());
    rig.press(1, 20);
    assert_eq!(rig.out(), ["00:29"]);
    rig.release(0, 40);
    rig.release(1, 50);
    assert_eq!(rig.out(), ["00:"]);
}

#[test]
fn combo_key_alone_passes_through() {
    let mut rig = Rig::new(vec![vec![key(kc::J), key(kc::K), key(kc::A)]]);
    rig.map.combos = vec![combo(&[0, 1], key(kc::ESCAPE))];

    // Timeout.
    rig.press(0, 0);
    assert_eq!(rig.engine.next_deadline(&rig.map), Some(50));
    rig.tick(50);
    assert_eq!(rig.out(), ["00:0d"]);
    rig.release(0, 60);

    // Quick tap.
    rig.press(1, 100);
    rig.release(1, 110);
    assert_eq!(rig.out(), ["00:", "00:0e", "00:"]);

    // Interrupted by a key outside the combo: order is preserved.
    rig.press(0, 200);
    rig.press(2, 210);
    assert_eq!(rig.out(), ["00:0d", "00:0d,04"]);
}

#[test]
fn second_combo_key_too_late_types_both() {
    let mut rig = Rig::new(vec![vec![key(kc::J), key(kc::K)]]);
    rig.map.combos = vec![combo(&[0, 1], key(kc::ESCAPE))];
    rig.press(0, 0);
    rig.press(1, 80);
    rig.tick(130);
    assert_eq!(rig.out(), ["00:0d", "00:0d,0e"]);
}

#[test]
fn overlapping_combos_prefer_the_longer_one() {
    let mut rig = Rig::new(vec![vec![key(kc::J), key(kc::K), key(kc::A)]]);
    rig.map.combos = vec![combo(&[0, 1], key(kc::ESCAPE)), combo(&[0, 1, 2], key(kc::TAB))];

    rig.press(0, 0);
    rig.press(1, 10);
    assert!(rig.out().is_empty());
    rig.press(2, 20);
    assert_eq!(rig.out(), ["00:2b"]);
    for pos in 0..3 {
        rig.release(pos, 30);
    }
    assert_eq!(rig.out(), ["00:"]);

    // Only two keys: the short combo fires once the long one times out.
    rig.press(0, 100);
    rig.press(1, 110);
    rig.tick(150);
    assert_eq!(rig.out(), ["00:29"]);
}

#[test]
fn combo_can_be_a_hold_tap() {
    let mut rig = Rig::new(vec![vec![key(kc::J), key(kc::K), key(kc::B)]]);
    rig.map.combos = vec![combo(&[0, 1], shift_or(kc::A))];
    rig.press(0, 0);
    rig.press(1, 10);
    rig.tick(210);
    rig.press(2, 220);
    assert_eq!(rig.out(), ["02:", "02:05"]);
}

#[test]
fn boot_report_layout() {
    let mut report = KeyboardReport { mods: 0x02, ..Default::default() };
    for key in 4..12 {
        report.keys.push(key).unwrap();
    }
    assert_eq!(report.boot(), [0x02, 0, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn macro_types_its_steps_in_order() {
    let mut rig = Rig::new(vec![vec![basic(Basic::Macro(0)), key(kc::Z)]]);
    rig.map.macros = vec![vec![
        MacroStep::Press(kc::LEFT_SHIFT),
        MacroStep::Tap(kc::A),
        MacroStep::Release(kc::LEFT_SHIFT),
        MacroStep::DelayMs(50),
        MacroStep::Tap(kc::B),
    ]];
    rig.press(0, 0);
    rig.release(0, 1);
    // Run the clock the way the firmware does: tick whenever a deadline passed.
    for t in 0..100 {
        if rig.engine.next_deadline(&rig.map).is_some_and(|d| t >= d) {
            rig.tick(t);
        }
        if t == 30 {
            assert_eq!(rig.out(), ["02:", "02:04", "02:", "00:"], "pauses at the delay");
        }
    }
    assert_eq!(rig.out(), ["00:05", "00:"]);
    assert_eq!(rig.engine.next_deadline(&rig.map), None);

    // Normal typing still works while a macro is not running, and a key
    // pressed during one is merged into the same report.
    rig.press(0, 200);
    rig.press(1, 200);
    rig.tick(200);
    assert_eq!(rig.out(), ["00:1d", "02:1d"]);
}
