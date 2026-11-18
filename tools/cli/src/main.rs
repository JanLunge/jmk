//! Talks to a JMK keyboard over its configuration channel.
//! Linux only for now: it uses the kernel's hidraw device files directly.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use std::{env, thread};

use jmk_core::Action;
use jmk_proto::frame::{self, Assembler, REPORT_SIZE};
use jmk_proto::{DeviceMessage, DiodeDirection, Event, HardwareConfig, MatrixConfig, Pin, Request, Response};

/// The protocol's fixed-capacity vector.
use jmk_proto::Vec as PinVec;

const USAGE: &str = "\
usage: jmk <command>

  info                          firmware, chip and usable pins
  show                          current hardware configuration and layer 0
  matrix <rows> <cols> <col2row|row2col>
                                use a diode matrix, pins as 2,3,4
  direct <pins> [active-high]   one pin per switch
  reserve <pins|none>           pins the firmware must never touch
  keys <layer> <usages>         set actions from position 0, HID usages in hex: 04,05
  probe <pins> [seconds]        report connections between pins (default 10 s)
  watch [seconds]               report key positions as they change (default 10 s)
  save                          write the configuration to flash
  factory-reset                 erase the saved configuration
  reboot                        restart the firmware
  bootloader                    restart into the UF2 bootloader
  flash <file.uf2>              bootloader, write the image, wait for the keyboard
";

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result<()> {
    match args {
        ["info"] => {
            let Response::Info(info) = Keyboard::open()?.request(Request::GetInfo)? else { return unexpected() };
            let (major, minor, patch) = info.firmware;
            let pins: Vec<String> = (0..64).filter(|p| info.usable_pins >> p & 1 == 1).map(|p| p.to_string()).collect();
            println!("firmware {major}.{minor}.{patch}, protocol {}, {:?}", info.protocol, info.mcu);
            let uid: String = info.uid.iter().map(|b| format!("{b:02x}")).collect();
            println!("chip id {uid}");
            println!("up to {} keys on {} layers", info.max_keys, info.max_layers);
            println!("usable pins: {}", pins.join(","));
            Ok(())
        }
        ["show"] => {
            let mut keyboard = Keyboard::open()?;
            let hardware = keyboard.hardware()?;
            println!("{hardware:#?}");
            let Response::LayerCount(layers) = keyboard.request(Request::GetLayerCount)? else { return unexpected() };
            let Response::Actions(actions) = keyboard.request(Request::GetActions { layer: 0, pos: 0 })? else {
                return unexpected();
            };
            println!("{layers} layer(s), layer 0 starts with:");
            for (pos, action) in actions.iter().enumerate().take(hardware.matrix.key_count()) {
                println!("  {pos}: {action:?}");
            }
            Ok(())
        }
        ["matrix", rows, cols, diodes] => {
            let diodes = match *diodes {
                "col2row" => DiodeDirection::Col2Row,
                "row2col" => DiodeDirection::Row2Col,
                other => return Err(format!("unknown diode direction {other}")),
            };
            let matrix = MatrixConfig::Matrix { rows: pin_list(rows)?, cols: pin_list(cols)?, diodes };
            Keyboard::open()?.set_matrix(matrix)
        }
        ["direct", pins, rest @ ..] => {
            let active_high = match rest {
                [] => false,
                ["active-high"] => true,
                _ => return Err(USAGE.into()),
            };
            Keyboard::open()?.set_matrix(MatrixConfig::Direct { pins: pin_list(pins)?, active_high })
        }
        ["reserve", pins] => {
            let mut keyboard = Keyboard::open()?;
            let mut hardware = keyboard.hardware()?;
            hardware.reserved_pins = if *pins == "none" { Default::default() } else { pin_list(pins)? };
            keyboard.ok(Request::SetHardware(hardware))
        }
        ["keys", layer, usages] => {
            let layer = layer.parse().map_err(|_| format!("bad layer {layer}"))?;
            let usages: Vec<u8> = usages
                .split(',')
                .map(|u| u8::from_str_radix(u, 16).map_err(|_| format!("bad usage {u}")))
                .collect::<Result<_>>()?;
            let mut keyboard = Keyboard::open()?;
            for (chunk, usages) in usages.chunks(jmk_proto::ACTIONS_PER_CHUNK).enumerate() {
                let pos = (chunk * jmk_proto::ACTIONS_PER_CHUNK) as u16;
                let actions = usages.iter().map(|u| Action::key(*u)).collect();
                keyboard.ok(Request::SetActions { layer, pos, actions })?;
            }
            Ok(())
        }
        ["probe", pins, rest @ ..] => {
            let mut keyboard = Keyboard::open()?;
            keyboard.ok(Request::StartProbe(pin_list(pins)?))?;
            keyboard.print_events(seconds(rest)?);
            keyboard.ok(Request::StopProbe)
        }
        ["watch", rest @ ..] => {
            let mut keyboard = Keyboard::open()?;
            keyboard.ok(Request::WatchKeys(true))?;
            keyboard.print_events(seconds(rest)?);
            keyboard.ok(Request::WatchKeys(false))
        }
        ["save"] => Keyboard::open()?.ok(Request::Save),
        ["factory-reset"] => Keyboard::open()?.ok(Request::FactoryReset),
        ["reboot"] => Keyboard::open()?.ok(Request::Reboot { bootloader: false }),
        ["bootloader"] => Keyboard::open()?.ok(Request::Reboot { bootloader: true }),
        ["flash", image] => flash(image),
        _ => Err(USAGE.into()),
    }
}

fn unexpected<T>() -> Result<T> {
    Err("unexpected reply from the keyboard".into())
}

fn pin_list<const N: usize>(text: &str) -> Result<PinVec<Pin, N>> {
    let mut pins = PinVec::new();
    for part in text.split(',') {
        let pin = part.parse().map_err(|_| format!("bad pin {part}"))?;
        pins.push(pin).map_err(|_| format!("more than {N} pins"))?;
    }
    Ok(pins)
}

fn seconds(rest: &[&str]) -> Result<Duration> {
    match rest {
        [] => Ok(Duration::from_secs(10)),
        [text] => text.parse().map(Duration::from_secs_f32).map_err(|_| format!("bad duration {text}")),
        _ => Err(USAGE.into()),
    }
}

struct Keyboard {
    device: File,
    messages: Receiver<DeviceMessage>,
}

impl Keyboard {
    fn open() -> Result<Self> {
        let path = find_config_channel().ok_or("no JMK keyboard found (or no permission for its hidraw device)")?;
        let device = OpenOptions::new().read(true).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut reader = device.try_clone().map_err(|e| e.to_string())?;
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            let mut assembler = Assembler::new();
            let mut report = [0u8; REPORT_SIZE];
            while let Ok(len) = reader.read(&mut report) {
                if let Some(Ok(message)) = assembler.push::<DeviceMessage>(&report[..len]) {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Self { device, messages })
    }

    fn request(&mut self, request: Request) -> Result<Response> {
        let mut failed = None;
        frame::encode(&request, |report| {
            // hidraw wants the report number first; this interface has none.
            let mut packet = [0u8; REPORT_SIZE + 1];
            packet[1..].copy_from_slice(report);
            failed = failed.take().or(self.device.write_all(&packet).err());
        })
        .map_err(|e| e.to_string())?;
        if let Some(error) = failed {
            return Err(format!("write failed: {error}"));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.messages.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(DeviceMessage::Response(response)) => return Ok(response),
                Ok(DeviceMessage::Event(event)) => print_event(&event),
                Err(RecvTimeoutError::Timeout) => return Err("the keyboard did not answer".into()),
                Err(RecvTimeoutError::Disconnected) => return Err("the keyboard disconnected".into()),
            }
        }
    }

    fn ok(&mut self, request: Request) -> Result<()> {
        match self.request(request)? {
            Response::Ok => Ok(()),
            Response::Error(error) => Err(format!("the keyboard refused: {error:?}")),
            _ => unexpected(),
        }
    }

    fn hardware(&mut self) -> Result<HardwareConfig> {
        match self.request(Request::GetHardware)? {
            Response::Hardware(hardware) => Ok(hardware),
            _ => unexpected(),
        }
    }

    fn set_matrix(&mut self, matrix: MatrixConfig) -> Result<()> {
        let hardware = HardwareConfig { matrix, ..self.hardware()? };
        self.ok(Request::SetHardware(hardware))
    }

    fn print_events(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while let Ok(message) = self.messages.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if let DeviceMessage::Event(event) = message {
                print_event(&event);
            }
        }
    }
}

fn print_event(event: &Event) {
    match event {
        Event::Key { pos, pressed } => println!("key {pos} {}", if *pressed { "down" } else { "up" }),
        Event::Probe(contacts) => println!("contacts: {contacts:?}"),
        Event::Resolved { pos, layer } => println!("key {pos} used layer {layer}"),
        Event::Layers(layers) => println!("active layers: {layers:#b}"),
    }
}

/// The hidraw device whose report descriptor starts with our vendor usage page.
fn find_config_channel() -> Option<PathBuf> {
    const VENDOR_PAGE: [u8; 3] = [0x06, 0x4A, 0xFF];
    let mut entries: std::vec::Vec<_> = fs::read_dir("/sys/class/hidraw").ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    entries.into_iter().find_map(|entry| {
        let descriptor = fs::read(entry.path().join("device/report_descriptor")).ok()?;
        descriptor.starts_with(&VENDOR_PAGE).then(|| PathBuf::from("/dev").join(entry.file_name()))
    })
}

fn flash(image: &str) -> Result<()> {
    const DRIVE: &str = "/dev/disk/by-label/RPI-RP2";
    let image = fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    if image.len() % 512 != 0 || !image.starts_with(b"UF2\n") {
        return Err("not a UF2 file".into());
    }
    if fs::metadata(DRIVE).is_err() {
        Keyboard::open()?.ok(Request::Reboot { bootloader: true })?;
    }
    let drive = wait_for("the bootloader drive", || OpenOptions::new().write(true).open(DRIVE).ok())?;
    // The RP2040 boot ROM takes UF2 blocks written to any sector of its
    // drive, so no filesystem access is needed. Stay clear of the first
    // sectors, which hold its FAT structures.
    use std::os::unix::fs::FileExt;
    drive.write_all_at(&image, 1000 * 512).map_err(|e| format!("write failed: {e}"))?;
    // The board resets as soon as the last block arrives, which can fail the sync.
    let _ = drive.sync_all();
    drop(drive);
    wait_for("the keyboard", find_config_channel_ready)?;
    println!("flashed");
    Ok(())
}

fn find_config_channel_ready() -> Option<()> {
    let path = find_config_channel()?;
    OpenOptions::new().read(true).write(true).open(path).ok().map(|_| ())
}

fn wait_for<T>(what: &str, mut attempt: impl FnMut() -> Option<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(found) = attempt() {
            return Ok(found);
        }
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        thread::sleep(Duration::from_millis(200));
    }
}
