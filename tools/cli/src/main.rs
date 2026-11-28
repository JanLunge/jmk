//! Talks to a JMK keyboard over its configuration channel.
//! Linux only for now: it uses the kernel's hidraw device files directly.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use std::{env, thread};

use jmk_core::Action;
use jmk_proto::frame::{self, Assembler, REPORT_SIZE};
use jmk_proto::{DeviceMessage, DiodeDirection, Event, HardwareConfig, Info, MatrixConfig, Pin, Request, Response};

/// The protocol's fixed-capacity vector.
use jmk_proto::Vec as PinVec;

const USAGE: &str = "\
usage: jmk [--device <chip id>] <command>

  devices                       connected keyboards and their chip ids
  info                          firmware, chip and usable pins
  show                          current hardware configuration and layer 0
  matrix <rows> <cols> <col2row|row2col|both>
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

With more than one keyboard connected, --device says which one, by its chip
id from `jmk devices`.

With no keyboard connected, flash writes to a Pico that waits in its
bootloader (a new board, or BOOTSEL held while plugging in).
";

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (device, args) = match args.as_slice() {
        ["--device", device, rest @ ..] => (Some(*device), rest),
        args => (None, args),
    };
    match run(device, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(device: Option<&str>, args: &[&str]) -> Result<()> {
    let open = || Keyboard::open(&select(device)?.hidraw);
    match args {
        ["devices"] => {
            devices();
            Ok(())
        }
        ["info"] => {
            let info = open()?.info()?;
            let (major, minor, patch) = info.firmware;
            let pins: Vec<String> = (0..64).filter(|p| info.usable_pins >> p & 1 == 1).map(|p| p.to_string()).collect();
            println!("firmware {major}.{minor}.{patch}, protocol {}, {:?}", info.protocol, info.mcu);
            println!("chip id {}", chip_id(&info));
            println!("up to {} keys on {} layers", info.max_keys, info.max_layers);
            println!("usable pins: {}", pins.join(","));
            Ok(())
        }
        ["show"] => {
            let mut keyboard = open()?;
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
                "both" => DiodeDirection::Both,
                other => return Err(format!("unknown diode direction {other}")),
            };
            let matrix = MatrixConfig::Matrix { rows: pin_list(rows)?, cols: pin_list(cols)?, diodes };
            open()?.set_matrix(matrix)
        }
        ["direct", pins, rest @ ..] => {
            let active_high = match rest {
                [] => false,
                ["active-high"] => true,
                _ => return Err(USAGE.into()),
            };
            open()?.set_matrix(MatrixConfig::Direct { pins: pin_list(pins)?, active_high })
        }
        ["reserve", pins] => {
            let mut keyboard = open()?;
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
            let mut keyboard = open()?;
            for (chunk, usages) in usages.chunks(jmk_proto::ACTIONS_PER_CHUNK).enumerate() {
                let pos = (chunk * jmk_proto::ACTIONS_PER_CHUNK) as u16;
                let actions = usages.iter().map(|u| Action::key(*u)).collect();
                keyboard.ok(Request::SetActions { layer, pos, actions })?;
            }
            Ok(())
        }
        ["probe", pins, rest @ ..] => {
            let mut keyboard = open()?;
            keyboard.ok(Request::StartProbe(pin_list(pins)?))?;
            keyboard.print_events(seconds(rest)?);
            keyboard.ok(Request::StopProbe)
        }
        ["watch", rest @ ..] => {
            let mut keyboard = open()?;
            keyboard.ok(Request::WatchKeys(true))?;
            keyboard.print_events(seconds(rest)?);
            keyboard.ok(Request::WatchKeys(false))
        }
        ["save"] => open()?.ok(Request::Save),
        ["factory-reset"] => open()?.ok(Request::FactoryReset),
        ["reboot"] => open()?.ok(Request::Reboot { bootloader: false }),
        ["bootloader"] => open()?.ok(Request::Reboot { bootloader: true }),
        ["flash", image] => flash(device, image),
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
    fn open(path: &Path) -> Result<Self> {
        let device = OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
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

    fn info(&mut self) -> Result<Info> {
        match self.request(Request::GetInfo)? {
            Response::Info(info) => Ok(info),
            _ => unexpected(),
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
        Event::Peer(bytes) => println!("message from the other half, {} bytes", bytes.len()),
    }
}

fn chip_id(info: &Info) -> String {
    info.uid.iter().map(|b| format!("{b:02x}")).collect()
}

/// A keyboard's configuration channel.
struct Found {
    /// The USB port it is plugged into, such as `1-5.2`: unlike the hidraw
    /// device, it stays the same when the keyboard restarts.
    port: String,
    hidraw: PathBuf,
}

/// Every connected keyboard, by its configuration channel: the hidraw
/// device whose report descriptor starts with our vendor usage page.
fn keyboards() -> Vec<Found> {
    const VENDOR_PAGE: [u8; 3] = [0x06, 0x4A, 0xFF];
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else { return Vec::new() };
    let mut found: Vec<Found> = entries
        .flatten()
        .filter_map(|entry| {
            let descriptor = fs::read(entry.path().join("device/report_descriptor")).ok()?;
            if !descriptor.starts_with(&VENDOR_PAGE) {
                return None;
            }
            let port = usb_port(&fs::canonicalize(entry.path()).ok()?)?;
            Some(Found { port, hidraw: PathBuf::from("/dev").join(entry.file_name()) })
        })
        .collect();
    found.sort_by(|a, b| a.port.cmp(&b.port));
    found
}

/// The keyboard whose chip id `device` names; without one, the only
/// keyboard connected.
fn select(device: Option<&str>) -> Result<Found> {
    let mut found = keyboards();
    match device {
        Some(id) => found
            .into_iter()
            .find(|keyboard| Keyboard::open(&keyboard.hidraw).and_then(|mut k| k.info()).is_ok_and(|info| chip_id(&info) == id))
            .ok_or_else(|| format!("no keyboard with chip id {id} (see `jmk devices`)")),
        None if found.len() == 1 => Ok(found.remove(0)),
        None if found.is_empty() => Err("no JMK keyboard found (or no permission for its hidraw device)".into()),
        None => Err("more than one keyboard is connected; choose one with --device (see `jmk devices`)".into()),
    }
}

fn devices() {
    for keyboard in keyboards() {
        match Keyboard::open(&keyboard.hidraw).and_then(|mut k| k.info()) {
            Ok(info) => {
                let (major, minor, patch) = info.firmware;
                println!("{}  {:?}, firmware {major}.{minor}.{patch}", chip_id(&info), info.mcu);
            }
            Err(error) => println!("{}: {error}", keyboard.hidraw.display()),
        }
    }
}

/// The USB port in a device's sysfs path: the name of the USB device that
/// the interface it hangs off belongs to, such as `1-5.2` in
/// `.../usb1/1-5/1-5.2/1-5.2:1.2/...`.
fn usb_port(sys: &Path) -> Option<String> {
    let parts: Vec<&str> = sys.iter().filter_map(|part| part.to_str()).collect();
    let bus = parts.iter().position(|part| part.starts_with("usb"))?;
    let interface = parts[bus + 1..].iter().find(|part| part.contains(':'))?;
    interface.split(':').next().map(str::to_string)
}

/// USB disks with their port. Where a disk has partitions, its first one,
/// which is where a bootloader keeps its filesystem.
fn usb_disks() -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir("/sys/block") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let sys = fs::canonicalize(entry.path()).ok()?;
            let port = usb_port(&sys)?;
            let mut partitions: Vec<_> = fs::read_dir(&sys)
                .ok()?
                .flatten()
                .filter(|part| part.path().join("partition").exists())
                .map(|part| part.file_name())
                .collect();
            partitions.sort();
            let name = partitions.into_iter().next().unwrap_or(entry.file_name());
            Some((port, PathBuf::from("/dev").join(name)))
        })
        .collect()
}

/// The port of a board that waits in its bootloader with no firmware to
/// ask: a new one, or one whose BOOTSEL was held. Only when there is
/// exactly one such drive, so the image cannot go to the wrong board.
fn waiting_bootloader() -> Option<String> {
    // What the boot ROMs of the RP2040 and the RP2350 call their drive.
    const DRIVES: [&str; 2] = ["/dev/disk/by-label/RPI-RP2", "/dev/disk/by-label/RP2350"];
    let mut waiting = DRIVES.iter().filter_map(|drive| fs::canonicalize(drive).ok());
    let drive = waiting.next()?;
    if waiting.next().is_some() {
        return None;
    }
    usb_disks().into_iter().find(|(_, disk)| *disk == drive).map(|(port, _)| port)
}

fn flash(device: Option<&str>, image: &str) -> Result<()> {
    let image = fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    if image.len() % 512 != 0 || !image.starts_with(b"UF2\n") {
        return Err("not a UF2 file".into());
    }
    let port = match waiting_bootloader() {
        Some(port) if device.is_none() && keyboards().is_empty() => port,
        _ => {
            let Found { port, hidraw } = select(device)?;
            Keyboard::open(&hidraw)?.ok(Request::Reboot { bootloader: true })?;
            port
        }
    };
    // The bootloader's disk appears on the port the keyboard was on, so no
    // other disk can be taken for it.
    let (disk, file) = wait_for("the bootloader drive", || {
        let (_, disk) = usb_disks().into_iter().find(|(at, _)| *at == port)?;
        let file = OpenOptions::new().write(true).open(&disk).ok()?;
        Some((disk, file))
    })?;
    println!("writing to {}", disk.display());
    // A UF2 bootloader takes UF2 blocks written to any sector of its
    // drive, so no filesystem access is needed. Stay clear of the first
    // sectors, which hold its FAT structures.
    use std::os::unix::fs::FileExt;
    file.write_all_at(&image, 1000 * 512).map_err(|e| format!("write failed: {e}"))?;
    // The board resets as soon as the last block arrives, which can fail the sync.
    let _ = file.sync_all();
    drop(file);
    wait_for("the keyboard", || {
        let keyboard = keyboards().into_iter().find(|keyboard| keyboard.port == port)?;
        OpenOptions::new().read(true).write(true).open(keyboard.hidraw).ok()
    })?;
    println!("flashed");
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_is_the_usb_device_of_the_interface() {
        let hidraw = "/sys/devices/pci0000:00/0000:00:02.1/0000:10:00.0/usb1/1-5/1-5.2/1-5.2:1.2/0003:1209:0001.036B/hidraw/hidraw58";
        assert_eq!(usb_port(Path::new(hidraw)).as_deref(), Some("1-5.2"));
        let disk = "/sys/devices/pci0000:00/0000:00:14.0/usb3/3-2/3-2:1.0/host0/target0:0:0/0:0:0:0/block/sda";
        assert_eq!(usb_port(Path::new(disk)).as_deref(), Some("3-2"));
    }

    #[test]
    fn a_device_not_on_usb_has_no_port() {
        assert_eq!(usb_port(Path::new("/sys/devices/pci0000:00/0000:00:01.2/0000:01:00.0/nvme/nvme0/nvme0n1")), None);
    }
}
