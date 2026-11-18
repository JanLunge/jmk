//! The device logic against simulated wiring, flash and split cable.

use jmk_core::keycode as kc;
use jmk_core::{Action, Basic, MacroStep};
use jmk_proto::{
    ColorOrder, Contact, DiodeDirection, Effect, EncoderConfig, Error, Event, HardwareConfig, KeyGeometry, Led,
    LightZone, LightingConfig, LinkConfig, MatrixConfig, Meta, Pin, Request, Response, SplitConfig, SplitStatus,
};
use jmk_sim::{HostEvent, VirtualKeyboard, Wire, Wiring};

/// Polls that are certain to contain one complete probe pass in these tests.
const PASS: u64 = 40;

struct Rig {
    kb: VirtualKeyboard,
}

impl Rig {
    fn new(wiring: Wiring) -> Self {
        Rig { kb: VirtualKeyboard::new(wiring, [1; 8]) }
    }

    fn wires(wires: &[Wire]) -> Self {
        Self::new(Wiring { switches: wires.to_vec(), encoders: vec![] })
    }

    fn ok(&mut self, request: Request) {
        assert_eq!(self.kb.request(request), Response::Ok);
    }

    fn refused(&mut self, request: Request, error: Error) {
        assert_eq!(self.kb.request(request), Response::Error(error));
    }

    fn run(&mut self, ms: u64) {
        self.kb.advance(ms);
    }

    /// Keyboard reports since the last call, as "mods:keys".
    fn out(&mut self) -> Vec<String> {
        self.kb.take_host_events().iter().map(show).collect()
    }

    fn events(&mut self) -> Vec<Event> {
        let mut assembler = jmk_proto::frame::Assembler::new();
        let mut events = vec![];
        for report in self.kb.take_reports() {
            if let Some(Ok(jmk_proto::DeviceMessage::Event(event))) = assembler.push(&report) {
                events.push(event);
            }
        }
        events
    }

    fn set_keys(&mut self, layer: u8, keys: &[u8]) {
        let actions = keys.iter().map(|k| Action::key(*k)).collect();
        self.ok(Request::SetActions { layer, pos: 0, actions });
    }

    fn first_actions(&mut self, layer: u8) -> Vec<Action> {
        match self.kb.request(Request::GetActions { layer, pos: 0 }) {
            Response::Actions(actions) => actions.to_vec(),
            other => panic!("expected actions, got {other:?}"),
        }
    }
}

fn show(event: &HostEvent) -> String {
    match event {
        HostEvent::Keyboard { mods, keys } => {
            let keys: Vec<String> = keys.iter().map(|k| format!("{k:02x}")).collect();
            format!("{mods:02x}:{}", keys.join(","))
        }
        HostEvent::Consumer(usage) => format!("consumer {usage:x}"),
    }
}

fn pins<const N: usize>(list: &[Pin]) -> jmk_proto::Vec<Pin, N> {
    list.iter().copied().collect()
}

fn direct(list: &[Pin]) -> HardwareConfig {
    hardware(MatrixConfig::Direct { pins: pins(list), active_high: false })
}

fn hardware(matrix: MatrixConfig) -> HardwareConfig {
    HardwareConfig { matrix, debounce_ms: 5, ..Default::default() }
}

fn matrix_2x2(diodes: DiodeDirection) -> HardwareConfig {
    hardware(MatrixConfig::Matrix { rows: pins(&[2, 3]), cols: pins(&[4, 5]), diodes })
}

/// 2x2 matrix, rows on 2 and 3, columns on 4 and 5, diodes column to row.
fn wired_2x2() -> Wiring {
    Wiring::matrix(&[2, 3], &[4, 5], true)
}

#[test]
fn unconfigured_device_touches_no_pins() {
    let mut rig = Rig::new(Wiring::direct(&[7]));
    rig.kb.set_key(0, true);
    rig.run(20);
    assert!(rig.kb.pins().touched().is_empty());
    assert!(rig.out().is_empty());
}

#[test]
fn direct_wiring_types_keys() {
    let mut rig = Rig::new(Wiring::direct(&[7, 9]));
    rig.ok(Request::SetHardware(direct(&[7, 9])));
    rig.set_keys(0, &[kc::A, kc::B]);
    rig.kb.set_key(1, true);
    rig.run(2);
    rig.kb.set_key(1, false);
    rig.run(10);
    assert_eq!(rig.out(), ["00:05", "00:"]);
    assert_eq!(rig.kb.pins().touched(), [7, 9]);
}

#[test]
fn active_high_direct_wiring() {
    let mut rig = Rig::wires(&[Wire::Supply(7)]);
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Direct { pins: pins(&[7]), active_high: true })));
    rig.set_keys(0, &[kc::A]);
    rig.run(5);
    assert!(rig.out().is_empty());
    rig.kb.set_key(0, true);
    rig.run(2);
    assert_eq!(rig.out(), ["00:04"]);
}

#[test]
fn diode_matrix_scans_in_the_configured_direction() {
    let mut rig = Rig::new(wired_2x2());
    rig.ok(Request::SetHardware(matrix_2x2(DiodeDirection::Col2Row)));
    rig.set_keys(0, &[kc::A, kc::B, kc::C, kc::D]);
    // Three keys of a square: with diodes the fourth must not ghost.
    for key in [0, 1, 2] {
        rig.kb.set_key(key, true);
    }
    rig.run(2);
    assert_eq!(rig.out(), ["00:04", "00:04,05", "00:04,05,06"]);

    // Scanning against the diodes sees nothing.
    rig.ok(Request::SetHardware(matrix_2x2(DiodeDirection::Row2Col)));
    assert_eq!(rig.out(), ["00:"]);
    rig.run(10);
    assert!(rig.out().is_empty());
}

#[test]
fn bounce_is_ignored() {
    let mut rig = Rig::new(Wiring::direct(&[7]));
    rig.ok(Request::SetHardware(direct(&[7])));
    rig.set_keys(0, &[kc::A]);
    for closed in [true, false, true, false, true] {
        rig.kb.set_key(0, closed);
        rig.run(1);
    }
    rig.run(10);
    assert_eq!(rig.out(), ["00:04"]);
}

#[test]
fn watch_keys_reports_positions() {
    let mut rig = Rig::new(wired_2x2());
    rig.ok(Request::SetHardware(matrix_2x2(DiodeDirection::Col2Row)));
    rig.ok(Request::WatchKeys(true));
    rig.kb.set_key(3, true);
    rig.run(2);
    assert_eq!(
        rig.events(),
        [Event::Key { pos: 3, pressed: true }, Event::Resolved { pos: 3, layer: 0 }, Event::Layers(0b01)]
    );
}

#[test]
fn layer_hold_on_a_diode_matrix_reports_the_layer_used() {
    // Three rows of two keys with the diodes from row to column.
    let (rows, cols) = ([18, 19, 20], [16, 17]);
    let mut rig = Rig::new(Wiring::matrix(&rows, &cols, false));
    rig.ok(Request::SetHardware(hardware(MatrixConfig::Matrix {
        rows: rows.into_iter().collect(),
        cols: cols.into_iter().collect(),
        diodes: DiodeDirection::Row2Col,
    })));
    rig.ok(Request::SetLayerCount(2));
    rig.set_keys(0, &[kc::A, kc::B, kc::C, kc::D, kc::E, kc::F]);
    rig.ok(Request::SetActions { layer: 0, pos: 4, actions: [Action::Basic(Basic::MomentaryLayer(1))].into_iter().collect() });
    rig.ok(Request::SetActions { layer: 1, pos: 0, actions: [Action::key(kc::N1)].into_iter().collect() });
    rig.ok(Request::WatchKeys(true));
    rig.run(2);
    assert_eq!(rig.events(), [Event::Layers(0b01)]);

    rig.kb.set_key(4, true);
    rig.run(10);
    assert_eq!(
        rig.events(),
        [Event::Key { pos: 4, pressed: true }, Event::Resolved { pos: 4, layer: 0 }, Event::Layers(0b11)]
    );
    // Same column as the layer key: layer 1 has its own key here.
    rig.kb.set_key(0, true);
    rig.run(10);
    assert_eq!(rig.events(), [Event::Key { pos: 0, pressed: true }, Event::Resolved { pos: 0, layer: 1 }]);
    // Other column: see-through on layer 1, so the base layer's key.
    rig.kb.set_key(1, true);
    rig.run(10);
    assert_eq!(rig.events(), [Event::Key { pos: 1, pressed: true }, Event::Resolved { pos: 1, layer: 0 }]);
    assert_eq!(rig.out(), ["00:1e", "00:1e,05"]);

    for key in [0, 1, 4] {
        rig.kb.set_key(key, false);
    }
    rig.run(10);
    assert_eq!(rig.events().last(), Some(&Event::Layers(0b01)));
    rig.kb.set_key(0, true);
    rig.run(10);
    assert_eq!(rig.out(), ["00:05", "00:", "00:04"]);
}

#[test]
fn probe_finds_matrix_links_and_diode_direction() {
    let mut rig = Rig::new(wired_2x2());
    rig.ok(Request::StartProbe(pins(&[2, 3, 4, 5, 6])));
    rig.run(PASS);
    assert!(rig.events().is_empty(), "nothing pressed yet");

    rig.kb.set_key(1, true); // column 5, row 2
    rig.run(PASS);
    let link = Contact::Link { driven: 2, sensed: 5 };
    assert_eq!(rig.events(), [Event::Probe([link].into_iter().collect())]);

    rig.kb.set_key(1, false);
    rig.run(PASS);
    assert_eq!(rig.events(), [Event::Probe(Default::default())]);
    assert!(rig.out().is_empty(), "probing must not type");
}

#[test]
fn probe_finds_direct_and_diodeless_switches() {
    let mut rig = Rig::wires(&[Wire::Ground(7), Wire::Supply(8), Wire::Plain(10, 11)]);
    rig.ok(Request::StartProbe(pins(&[7, 8, 10, 11])));
    for key in 0..3 {
        rig.kb.set_key(key, true);
    }
    rig.run(PASS);
    let expected = [
        Contact::Supply(8),
        Contact::Ground(7),
        Contact::Link { driven: 10, sensed: 11 },
        Contact::Link { driven: 11, sensed: 10 },
    ];
    assert_eq!(rig.events(), [Event::Probe(expected.into_iter().collect())]);
}

#[test]
fn probe_catches_the_short_pulses_of_a_knob() {
    let mut rig = Rig::new(Wiring { encoders: vec![(10, 11)], ..Wiring::direct(&[7]) });
    rig.ok(Request::StartProbe((0..26).collect()));
    rig.run(PASS);
    for _ in 0..3 {
        rig.kb.turn_encoder(0, true);
    }
    rig.run(PASS);
    let mut grounded: Vec<Pin> = rig
        .events()
        .into_iter()
        .flat_map(|event| match event {
            Event::Probe(contacts) => contacts.to_vec(),
            _ => vec![],
        })
        .filter_map(|contact| match contact {
            Contact::Ground(pin) => Some(pin),
            _ => None,
        })
        .collect();
    grounded.sort();
    grounded.dedup();
    assert_eq!(grounded, [10, 11]);
}

#[test]
fn probing_many_pins_keeps_each_poll_short() {
    // Each poll may wait for levels to settle only a couple of times,
    // however many pins are probed; a long poll starves USB on real hardware.
    let mut rig = Rig::wires(&[]);
    rig.ok(Request::StartProbe((0..30).collect()));
    for _ in 0..100 {
        let before = rig.kb.pins().settles();
        rig.run(1);
        assert!(rig.kb.pins().settles() - before <= 2);
    }
}

#[test]
fn reserved_and_unusable_pins_are_never_touched() {
    let mut rig = Rig::wires(&[]);
    let mut hw = hardware(MatrixConfig::None);
    hw.reserved_pins = pins(&[16, 17]);
    rig.ok(Request::SetHardware(hw.clone()));

    rig.refused(Request::StartProbe(pins(&[2, 3, 17])), Error::ReservedPin(17));
    rig.refused(Request::StartProbe(pins(&[2, 30])), Error::UnusablePin(30));

    hw.matrix = MatrixConfig::Direct { pins: pins(&[4, 16]), active_high: false };
    rig.refused(Request::SetHardware(hw.clone()), Error::ReservedPin(16));
    hw.matrix = MatrixConfig::Direct { pins: pins(&[4, 4]), active_high: false };
    rig.refused(Request::SetHardware(hw.clone()), Error::DuplicatePin(4));
    hw.matrix = MatrixConfig::Direct { pins: pins(&[4]), active_high: false };
    hw.encoders = [EncoderConfig { a: 4, b: 5, steps_per_detent: 4 }].into_iter().collect();
    rig.refused(Request::SetHardware(hw), Error::DuplicatePin(4));

    rig.run(10);
    assert!(rig.kb.pins().touched().is_empty());
}

#[test]
fn stopping_a_probe_resumes_typing() {
    let mut rig = Rig::new(Wiring::direct(&[7]));
    rig.ok(Request::SetHardware(direct(&[7])));
    rig.set_keys(0, &[kc::A]);
    rig.ok(Request::StartProbe(pins(&[7, 8])));
    rig.kb.set_key(0, true);
    rig.run(5);
    assert!(rig.out().is_empty());
    rig.ok(Request::StopProbe);
    rig.run(2);
    assert_eq!(rig.out(), ["00:04"]);
}

#[test]
fn everything_survives_reboot_after_save_only() {
    let mut rig = Rig::new(Wiring::direct(&[7]));
    let hw = direct(&[7]);
    let hold_tap = Action::HoldTap { hold: Basic::MomentaryLayer(1), tap: Basic::Key(kc::SPACE), profile: 1 };
    let meta = Meta { name: "Test pad".try_into().unwrap(), controller: "pico".try_into().unwrap() };
    let shape = KeyGeometry { x: 0, y: 150, w: 100, h: 200, r: -150 };
    let steps = [MacroStep::Tap(kc::A), MacroStep::DelayMs(300)];
    let configure = |rig: &mut Rig| {
        rig.ok(Request::SetHardware(hw.clone()));
        rig.ok(Request::SetLayerCount(2));
        rig.ok(Request::SetActions { layer: 1, pos: 0, actions: [hold_tap].into_iter().collect() });
        rig.ok(Request::SetMeta(meta.clone()));
        rig.ok(Request::SetLayout { start: 0, keys: [shape].into_iter().collect() });
        rig.ok(Request::SetMacro { index: 3, start: 0, steps: steps.into_iter().collect() });
    };

    configure(&mut rig);
    rig.kb.reboot();
    assert_eq!(rig.kb.request(Request::GetHardware), Response::Hardware(hardware(MatrixConfig::None)));

    configure(&mut rig);
    rig.ok(Request::Save);
    rig.kb.reboot();
    assert_eq!(rig.kb.request(Request::GetHardware), Response::Hardware(hw));
    assert_eq!(rig.kb.request(Request::GetLayerCount), Response::LayerCount(2));
    assert_eq!(rig.first_actions(1)[..2], [hold_tap, Action::Transparent]);
    assert_eq!(rig.kb.request(Request::GetMeta), Response::Meta(meta));
    assert_eq!(
        rig.kb.request(Request::GetLayout { start: 0 }),
        Response::Layout { total: 1, keys: [shape].into_iter().collect() }
    );
    assert_eq!(
        rig.kb.request(Request::GetMacro { index: 3, start: 0 }),
        Response::Macro { total: 2, steps: steps.into_iter().collect() }
    );
}

#[test]
fn power_cut_during_save_keeps_previous_configuration() {
    let mut rig = Rig::wires(&[]);
    rig.set_keys(0, &[kc::A]);
    rig.ok(Request::Save);
    rig.set_keys(0, &[kc::B]);
    rig.ok(Request::Save);

    rig.set_keys(0, &[kc::C]);
    rig.kb.flash().fail_after_writes(Some(0));
    rig.refused(Request::Save, Error::Storage);
    rig.kb.flash().fail_after_writes(None);

    rig.kb.reboot();
    assert_eq!(rig.first_actions(0)[0], Action::key(kc::B));

    // The next save must still work and win.
    rig.set_keys(0, &[kc::D]);
    rig.ok(Request::Save);
    rig.kb.reboot();
    assert_eq!(rig.first_actions(0)[0], Action::key(kc::D));
}

#[test]
fn factory_reset_erases_everything() {
    let mut rig = Rig::new(Wiring::direct(&[7]));
    rig.ok(Request::SetHardware(direct(&[7])));
    rig.ok(Request::Save);
    rig.ok(Request::FactoryReset);
    rig.kb.reboot();
    assert_eq!(rig.kb.request(Request::GetHardware), Response::Hardware(hardware(MatrixConfig::None)));
}

#[test]
fn macros_can_be_edited_independently() {
    let mut rig = Rig::wires(&[]);
    let taps = |keys: &[u8]| keys.iter().map(|k| MacroStep::Tap(*k)).collect::<jmk_proto::Vec<_, 32>>();
    let read = |rig: &mut Rig, index| match rig.kb.request(Request::GetMacro { index, start: 0 }) {
        Response::Macro { steps, .. } => steps,
        other => panic!("expected macro, got {other:?}"),
    };
    rig.ok(Request::SetMacro { index: 0, start: 0, steps: taps(&[1, 2, 3]) });
    rig.ok(Request::SetMacro { index: 2, start: 0, steps: taps(&[7, 8]) });
    rig.ok(Request::SetMacro { index: 1, start: 0, steps: taps(&[4, 5, 6]) });
    // Shrinking and growing one macro leaves its neighbours alone.
    rig.ok(Request::SetMacro { index: 0, start: 0, steps: taps(&[9]) });
    rig.ok(Request::SetMacro { index: 1, start: 3, steps: taps(&[10]) });
    assert_eq!(read(&mut rig, 0), taps(&[9]));
    assert_eq!(read(&mut rig, 1), taps(&[4, 5, 6, 10]));
    assert_eq!(read(&mut rig, 2), taps(&[7, 8]));
    rig.refused(Request::SetMacro { index: 0, start: 5, steps: taps(&[1]) }, Error::OutOfRange);
    rig.refused(Request::SetMacro { index: 16, start: 0, steps: taps(&[1]) }, Error::OutOfRange);

    // The pool is shared and bounded.
    let chunk = taps(&[1; 32]);
    for part in 0..15 {
        rig.ok(Request::SetMacro { index: 5, start: part * 32, steps: chunk.clone() });
    }
    rig.refused(Request::SetMacro { index: 5, start: 480, steps: chunk }, Error::Full);
}

#[test]
fn macro_key_types_text() {
    let mut rig = Rig::new(Wiring::direct(&[7]));
    rig.ok(Request::SetHardware(direct(&[7])));
    rig.ok(Request::SetActions { layer: 0, pos: 0, actions: [Action::Basic(Basic::Macro(0))].into_iter().collect() });
    let steps = [MacroStep::Tap(kc::A), MacroStep::Tap(kc::B)];
    rig.ok(Request::SetMacro { index: 0, start: 0, steps: steps.into_iter().collect() });
    rig.kb.set_key(0, true);
    rig.run(30);
    assert_eq!(rig.out(), ["00:04", "00:", "00:05", "00:"]);
}

#[test]
fn encoder_clicks_tap_their_positions() {
    let mut rig = Rig::new(Wiring { encoders: vec![(10, 11)], ..Wiring::direct(&[7]) });
    let mut hw = direct(&[7]);
    hw.encoders = [EncoderConfig { a: 10, b: 11, steps_per_detent: 4 }].into_iter().collect();
    rig.ok(Request::SetHardware(hw));
    // Position 0 is the key, 1 counter-clockwise, 2 clockwise.
    rig.set_keys(0, &[kc::A, kc::J, kc::K]);
    rig.kb.turn_encoder(0, true);
    rig.kb.turn_encoder(0, true);
    rig.kb.turn_encoder(0, false);
    assert_eq!(rig.out(), ["00:0e", "00:", "00:0e", "00:", "00:0d", "00:"]);
}

/// Two halves of three direct-wired keys each, joined by a cable.
fn split_pair() -> (Rig, Rig) {
    let link = LinkConfig::Uart { tx: 0, rx: 1 };
    let mut left = Rig::new(Wiring::direct(&[2, 3, 4]));
    let mut right = Rig::new(Wiring { encoders: vec![(10, 11)], ..Wiring::direct(&[2, 3, 4]) });
    VirtualKeyboard::connect(&mut left.kb, &mut right.kb);
    left.ok(Request::SetHardware(HardwareConfig { split: SplitConfig::Central(link), ..direct(&[2, 3, 4]) }));
    right.ok(Request::SetHardware(HardwareConfig {
        split: SplitConfig::Peripheral(link),
        encoders: [EncoderConfig { a: 10, b: 11, steps_per_detent: 4 }].into_iter().collect(),
        ..direct(&[2, 3, 4])
    }));
    // Left keys are positions 0..3, right keys 3..6, right encoder 6 and 7.
    left.set_keys(0, &[kc::A, kc::B, kc::C, kc::D, kc::E, kc::F, kc::J, kc::K]);
    (left, right)
}

fn run_both(left: &mut Rig, right: &mut Rig, ms: u64) {
    for _ in 0..ms {
        right.run(1);
        left.run(1);
    }
}

#[test]
fn split_halves_type_through_the_central_half() {
    let (mut left, mut right) = split_pair();
    run_both(&mut left, &mut right, 10);
    let status = SplitStatus { connected: true, remote_positions: 5 };
    assert_eq!(left.kb.request(Request::GetSplitStatus), Response::SplitStatus(status));
    assert_eq!(
        right.kb.request(Request::GetSplitStatus),
        Response::SplitStatus(SplitStatus { connected: true, remote_positions: 0 })
    );

    left.kb.set_key(0, true);
    right.kb.set_key(2, true);
    run_both(&mut left, &mut right, 5);
    right.kb.set_key(2, false);
    run_both(&mut left, &mut right, 10);
    assert_eq!(left.out(), ["00:04", "00:04,09", "00:04"]);
    assert!(right.out().is_empty(), "the peripheral half never types itself");

    right.kb.turn_encoder(0, true);
    run_both(&mut left, &mut right, 5);
    assert_eq!(left.out(), ["00:04,0e", "00:04"]);
}

#[test]
fn unplugging_the_cable_releases_the_other_halfs_keys() {
    let (mut left, mut right) = split_pair();
    right.kb.set_key(0, true);
    run_both(&mut left, &mut right, 10);
    assert_eq!(left.out(), ["00:07"]);

    left.kb.set_cable(false);
    run_both(&mut left, &mut right, 400);
    assert_eq!(left.out(), ["00:"]);
    assert_eq!(left.kb.request(Request::GetSplitStatus), Response::SplitStatus(SplitStatus { connected: false, remote_positions: 5 }));

    // Plugged back in, the key that is still held comes back by itself.
    left.kb.set_cable(true);
    run_both(&mut left, &mut right, 100);
    assert_eq!(left.out(), ["00:07"]);
}

#[test]
fn garbage_on_the_cable_is_ignored() {
    let (mut left, mut right) = split_pair();
    run_both(&mut left, &mut right, 10);
    // Noise, a frame with a wrong checksum claiming key 0 is down, an
    // impossible length, and a frame cut short.
    left.kb.inject_link_bytes(&[0xFF, 0x00, 0x4A, 0x03, 0x01, 0x03, 0x01, 0x00, 0x4A, 0xF0, 0x4A, 0x03, 0x01]);
    run_both(&mut left, &mut right, 10);
    assert!(left.out().is_empty());
    // The link recovers and real key presses still arrive.
    right.kb.set_key(1, true);
    run_both(&mut left, &mut right, 120);
    assert_eq!(left.out(), ["00:08"]);
}

#[test]
fn split_link_is_refused_where_the_chip_cannot_do_it() {
    let mut rig = Rig::wires(&[]);
    let hw = HardwareConfig { split: SplitConfig::Central(LinkConfig::Ble), ..hardware(MatrixConfig::None) };
    rig.refused(Request::SetHardware(hw), Error::Unsupported);
}

#[test]
fn a_held_pin_is_seen_as_grounded_by_the_board_at_the_other_end_of_a_wire() {
    let mut a = VirtualKeyboard::new(Wiring::direct(&[7]), [1; 8]);
    let mut b = VirtualKeyboard::new(Wiring::direct(&[7]), [2; 8]);
    VirtualKeyboard::connect_wired(&mut a, &mut b, &[(0, 1), (1, 0)]);
    let grounded = |kb: &mut VirtualKeyboard| {
        kb.advance(40);
        let mut assembler = jmk_proto::frame::Assembler::new();
        let mut last = None;
        for report in kb.take_reports() {
            if let Some(Ok(jmk_proto::DeviceMessage::Event(Event::Probe(contacts)))) = assembler.push(&report) {
                last = Some(contacts.to_vec());
            }
        }
        last
    };
    assert_eq!(b.request(Request::StartProbe((0..10).collect())), Response::Ok);
    assert_eq!(grounded(&mut b), None);

    assert_eq!(a.request(Request::HoldLow(Some(0))), Response::Ok);
    assert_eq!(grounded(&mut b), Some(vec![Contact::Ground(1)]));
    // Holding another pin lets go of the first.
    assert_eq!(a.request(Request::HoldLow(Some(1))), Response::Ok);
    assert_eq!(grounded(&mut b), Some(vec![Contact::Ground(0)]));
    assert_eq!(a.request(Request::HoldLow(Some(5))), Response::Ok);
    assert_eq!(grounded(&mut b), Some(vec![]));

    // A held pin stops the keys; letting go brings them back.
    a.request(Request::SetHardware(direct(&[7])));
    a.request(Request::SetActions { layer: 0, pos: 0, actions: [Action::key(kc::A)].into_iter().collect() });
    assert_eq!(a.request(Request::HoldLow(Some(0))), Response::Ok);
    a.set_key(0, true);
    a.advance(10);
    assert!(a.take_host_events().is_empty());
    assert_eq!(a.request(Request::HoldLow(None)), Response::Ok);
    a.advance(10);
    assert_eq!(a.take_host_events().len(), 1);
    // Reserved pins are never driven.
    let mut hw = direct(&[7]);
    hw.reserved_pins = [3].into_iter().collect();
    a.request(Request::SetHardware(hw));
    assert_eq!(a.request(Request::HoldLow(Some(3))), Response::Error(Error::ReservedPin(3)));
}

fn lighting(keys: Effect, underglow: Effect) -> LightingConfig {
    let zone = |effect| LightZone { effect, hue: 0, sat: 255, val: 255, speed: 100 };
    let leds = [Led { key: 0, x: 0, y: 0 }, Led { key: 1, x: 128, y: 0 }, Led { key: Led::UNDERGLOW, x: 255, y: 0 }];
    LightingConfig {
        pin: Some(9),
        order: ColorOrder::Grb,
        limit: 255,
        leds: leds.into_iter().collect(),
        keys: zone(keys),
        underglow: zone(underglow),
    }
}

#[test]
fn lights_show_each_zones_colour_in_the_chains_order() {
    let mut rig = Rig::wires(&[Wire::Ground(2), Wire::Ground(3)]);
    rig.ok(Request::SetHardware(direct(&[2, 3])));
    let mut config = lighting(Effect::Solid, Effect::Off);
    config.limit = 51;
    rig.ok(Request::SetLighting(config.clone()));
    rig.run(30);
    // Red at a fifth of full brightness, sent green first.
    assert_eq!(rig.kb.pins().lights(), [[0, 51, 0], [0, 51, 0], [0, 0, 0]]);

    config.order = ColorOrder::Rgb;
    config.underglow = LightZone { effect: Effect::Solid, hue: 0, sat: 0, val: 255, speed: 0 };
    rig.ok(Request::SetLighting(config));
    rig.run(30);
    assert_eq!(rig.kb.pins().lights(), [[51, 0, 0], [51, 0, 0], [51, 51, 51]]);
}

#[test]
fn a_pressed_key_lights_its_own_light_and_the_underglow() {
    let mut rig = Rig::wires(&[Wire::Ground(2), Wire::Ground(3)]);
    rig.ok(Request::SetHardware(direct(&[2, 3])));
    rig.ok(Request::SetLighting(lighting(Effect::Reactive, Effect::Reactive)));
    rig.run(30);
    assert_eq!(rig.kb.pins().lights(), [[0, 0, 0]; 3]);
    rig.kb.set_key(1, true);
    rig.run(25);
    let lit = rig.kb.pins().lights();
    assert_eq!(lit[0], [0, 0, 0]);
    assert!(lit[1][1] > 150 && lit[2][1] > 150, "{lit:?}");
    rig.run(1000);
    assert_eq!(rig.kb.pins().lights(), [[0, 0, 0]; 3]);
}

#[test]
fn one_light_can_be_shown_alone_to_find_it() {
    let mut rig = Rig::wires(&[Wire::Ground(2)]);
    rig.ok(Request::SetLighting(lighting(Effect::Solid, Effect::Solid)));
    rig.ok(Request::IdentifyLed(Some(2)));
    rig.run(30);
    let lit = rig.kb.pins().lights();
    assert_eq!((lit[0], lit[1]), ([0; 3], [0; 3]));
    assert!(lit[2].iter().all(|part| *part > 0));
    rig.refused(Request::IdentifyLed(Some(3)), Error::OutOfRange);
    rig.ok(Request::IdentifyLed(None));
    rig.run(30);
    assert_eq!(rig.kb.pins().lights()[0], [0, 255, 0]);
}

#[test]
fn the_light_pin_cannot_double_as_a_key_pin() {
    let mut rig = Rig::wires(&[Wire::Ground(2)]);
    rig.ok(Request::SetHardware(direct(&[2, 3])));
    let mut config = lighting(Effect::Solid, Effect::Off);
    config.pin = Some(3);
    rig.refused(Request::SetLighting(config.clone()), Error::DuplicatePin(3));
    config.pin = Some(9);
    rig.ok(Request::SetLighting(config));
    rig.refused(Request::SetHardware(direct(&[2, 9])), Error::DuplicatePin(9));
    rig.refused(Request::StartProbe(pins(&[2, 9])), Error::ReservedPin(9));
    rig.refused(Request::HoldLow(Some(9)), Error::ReservedPin(9));
    // The data pin belongs to the light driver; scanning leaves it alone.
    rig.run(50);
    assert!(!rig.kb.pins().touched().contains(&9));
}

#[test]
fn lighting_is_saved_and_a_save_from_before_lighting_still_loads() {
    let mut rig = Rig::wires(&[Wire::Ground(2)]);
    rig.ok(Request::SetHardware(direct(&[2])));
    rig.ok(Request::Save);
    rig.kb.reboot();
    assert_eq!(rig.kb.request(Request::GetLighting), Response::Lighting(LightingConfig::NONE));

    let config = lighting(Effect::Rainbow, Effect::Breathing);
    rig.ok(Request::SetLighting(config.clone()));
    rig.ok(Request::Save);
    rig.kb.reboot();
    assert_eq!(rig.kb.request(Request::GetLighting), Response::Lighting(config));
    assert_eq!(rig.kb.request(Request::GetHardware), Response::Hardware(direct(&[2])));
    // The chain is driven again after the restart without being asked.
    rig.run(30);
    assert_eq!(rig.kb.pins().lights().len(), 3);
}

#[test]
fn layer_colors_are_saved() {
    let mut rig = Rig::wires(&[Wire::Ground(2)]);
    rig.ok(Request::SetHardware(direct(&[2])));
    assert_eq!(rig.kb.request(Request::GetLayerColors), Response::LayerColors([0; 8].into_iter().collect()));
    rig.ok(Request::SetLayerColors([0, 3, 7].into_iter().collect()));
    rig.ok(Request::Save);
    rig.kb.reboot();
    assert_eq!(rig.kb.request(Request::GetLayerColors), Response::LayerColors([0, 3, 7, 0, 0, 0, 0, 0].into_iter().collect()));
}
