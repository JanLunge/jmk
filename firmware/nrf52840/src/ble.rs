//! Bluetooth LE: the keyboard as a HID-over-GATT peripheral.
//!
//! One host connects at a time. The keyboard advertises whenever it has no
//! connection; any host may pair then, and the newest pairing replaces the
//! stored bond.

use embassy_futures::join::join;
use embassy_futures::select::{select, select3, Either};
use embassy_time::{Duration, Timer};
use jmk_embassy::queue::HidQueues;
use jmk_embassy::usb::{report_map, MANUFACTURER, PID, PRODUCT, REPORT_MAP_SIZE, VID};
use trouble_host::prelude::*;

use crate::battery;
use crate::bonds::Bonds;

const CONNECTIONS_MAX: usize = 1;
/// Signalling, attribute protocol and security manager, with one to spare.
const L2CAP_CHANNELS_MAX: usize = 4;
/// The stored bond plus the one a new pairing adds before the old is dropped.
const BONDS_MAX: usize = crate::bonds::PROFILES + 1;

const KEYBOARD_REPORT_ID: u8 = 1;
const CONSUMER_REPORT_ID: u8 = 2;
const INPUT_REPORT: u8 = 1;
const OUTPUT_REPORT: u8 = 2;

static CONNECTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// True while a host is connected over Bluetooth.
pub fn connected() -> bool {
    CONNECTED.load(core::sync::atomic::Ordering::Relaxed)
}

static REPORT_MAP: [u8; REPORT_MAP_SIZE] = report_map(KEYBOARD_REPORT_ID, CONSUMER_REPORT_ID);

/// HID version 1.11, no country, remote wake and normally connectable.
const HID_INFORMATION: [u8; 4] = [0x11, 0x01, 0x00, 0x03];
/// Vendor ID from the USB Implementers Forum, then vendor, product and
/// version, as on USB.
const PNP_ID: [u8; 7] = {
    let (vid, pid) = (VID.to_le_bytes(), PID.to_le_bytes());
    [0x02, vid[0], vid[1], pid[0], pid[1], 0x00, 0x01]
};

#[gatt_server]
struct Server {
    hid: HidService,
    battery: BatteryService,
    /// Read by hosts through the stack, never by this code.
    _device_information: DeviceInformationService,
}

/// Report protocol only; the reports are the ones sent over USB. With
/// report IDs in the map, the values here leave the ID out.
#[gatt_service(uuid = service::HUMAN_INTERFACE_DEVICE)]
struct HidService {
    #[characteristic(uuid = characteristic::HID_INFORMATION, read, value = HID_INFORMATION)]
    information: [u8; 4],
    #[characteristic(uuid = characteristic::REPORT_MAP, read, value = REPORT_MAP, permissions(encrypted))]
    report_map: [u8; REPORT_MAP_SIZE],
    #[characteristic(uuid = characteristic::HID_CONTROL_POINT, write_without_response, permissions(encrypted))]
    control_point: u8,
    #[descriptor(uuid = descriptors::REPORT_REFERENCE, read, value = [KEYBOARD_REPORT_ID, INPUT_REPORT])]
    #[characteristic(uuid = characteristic::REPORT, read, notify, permissions(encrypted))]
    keyboard_input: [u8; 8],
    /// Lock-key LEDs.
    #[descriptor(uuid = descriptors::REPORT_REFERENCE, read, value = [KEYBOARD_REPORT_ID, OUTPUT_REPORT])]
    #[characteristic(uuid = characteristic::REPORT, read, write, write_without_response, permissions(encrypted))]
    keyboard_output: [u8; 1],
    #[descriptor(uuid = descriptors::REPORT_REFERENCE, read, value = [CONSUMER_REPORT_ID, INPUT_REPORT])]
    #[characteristic(uuid = characteristic::REPORT, read, notify, permissions(encrypted))]
    consumer_input: [u8; 2],
}

#[gatt_service(uuid = service::BATTERY)]
struct BatteryService {
    #[characteristic(uuid = characteristic::BATTERY_LEVEL, read, notify)]
    level: u8,
}

#[gatt_service(uuid = service::DEVICE_INFORMATION)]
struct DeviceInformationService {
    #[characteristic(uuid = characteristic::MANUFACTURER_NAME_STRING, read, value = MANUFACTURER)]
    manufacturer: &'static str,
    #[characteristic(uuid = characteristic::MODEL_NUMBER_STRING, read, value = PRODUCT)]
    model: &'static str,
    #[characteristic(uuid = characteristic::PNP_ID, read, value = PNP_ID)]
    pnp_id: [u8; 7],
}

/// Run Bluetooth forever. Returns only if the stack cannot be set up; the
/// keyboard then keeps working over USB.
///
/// `address` must be a static random address. Reports queued in `reports`
/// go to the connected host.
pub async fn run<C: Controller>(controller: C, address: [u8; 6], mut bonds: Bonds<'_>, reports: &HidQueues) {
    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX, 1, BONDS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(Address::random(address))
        .build();
    for bond in bonds.profiles().iter().flatten() {
        // Cannot fail: the table has room for every profile.
        let _ = stack.add_bond_information(bond.clone());
    }
    let Ok(server) = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: PRODUCT,
        appearance: &appearance::human_interface_device::KEYBOARD,
    })) else {
        return;
    };
    let mut runner = stack.runner();
    let mut peripheral = stack.peripheral();

    let host = async {
        loop {
            // Ends on a controller error; start over rather than stop.
            let _ = runner.run().await;
            Timer::after(Duration::from_secs(1)).await;
        }
    };
    let keyboard = async {
        loop {
            let Ok(connection) = advertise(&mut peripheral, &server).await else {
                Timer::after(Duration::from_secs(1)).await;
                continue;
            };
            // Without this a pairing leaves no keys to reconnect with.
            let _ = connection.raw().set_bondable(true);
            // Whatever was typed while no host listened must not arrive now.
            reports.clear();
            CONNECTED.store(true, core::sync::atomic::Ordering::Relaxed);
            select3(
                handle_events(&stack, &connection, &mut bonds),
                send_reports(&server, &connection, reports),
                report_battery(&server, &connection),
            )
            .await;
            CONNECTED.store(false, core::sync::atomic::Ordering::Relaxed);
        }
    };
    join(host, keyboard).await;
}

async fn advertise<'values, 'server, C: Controller>(
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut data = [0; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::IncompleteServiceUuids16(&[service::HUMAN_INTERFACE_DEVICE.to_le_bytes(), service::BATTERY.to_le_bytes()]),
            AdStructure::Unknown {
                ty: 0x19, // Appearance
                data: &appearance::human_interface_device::KEYBOARD.to_le_bytes(),
            },
            AdStructure::CompleteLocalName(PRODUCT.as_bytes()),
        ],
        &mut data,
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected { adv_data: &data[..len], scan_data: &[] },
        )
        .await?;
    Ok(advertiser.accept().await?.with_attribute_server(server)?)
}

/// Answer the host until it disconnects, and keep the bond of a pairing.
async fn handle_events<C: Controller>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    connection: &GattConnection<'_, '_, DefaultPacketPool>,
    bonds: &mut Bonds<'_>,
) {
    loop {
        match connection.next().await {
            GattConnectionEvent::Disconnected { .. } => return,
            GattConnectionEvent::PairingComplete { bond: Some(bond), .. } => {
                // One profile for now: the new host takes the place of the
                // previous one, in the stack and in flash.
                if let Some(previous) = &bonds.profiles()[0] {
                    if !previous.identity.match_identity(&bond.identity) {
                        let _ = stack.remove_bond_information(previous.identity);
                    }
                }
                // If flash fails the bond lasts until the next restart and
                // the host has to pair again.
                let _ = bonds.set(0, bond);
            }
            GattConnectionEvent::Gatt { event } => {
                // Reads and writes are served from the attribute table.
                // Nothing acts on a written value yet (lock-key LEDs,
                // suspend).
                if let Ok(reply) = event.accept() {
                    reply.send().await;
                }
            }
            _ => {}
        }
    }
}

/// Notify the host of queued reports. Fails quietly while the host has not
/// subscribed; the next report carries the complete state again.
async fn send_reports(server: &Server<'_>, connection: &GattConnection<'_, '_, DefaultPacketPool>, reports: &HidQueues) {
    loop {
        match select(reports.keyboard.receive(), reports.consumer.receive()).await {
            Either::First(report) => {
                let _ = server.hid.keyboard_input.notify(connection, &report, true).await;
            }
            Either::Second(usage) => {
                let _ = server.hid.consumer_input.notify(connection, &usage.to_le_bytes(), true).await;
            }
        }
    }
}

async fn report_battery(server: &Server<'_>, connection: &GattConnection<'_, '_, DefaultPacketPool>) {
    let mut reported = None;
    loop {
        let level = battery::level_percent();
        if reported != Some(level) {
            let _ = server.battery.level.notify(connection, &level, true).await;
            reported = Some(level);
        }
        Timer::after(Duration::from_secs(60)).await;
    }
}
