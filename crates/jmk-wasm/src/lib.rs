//! The configurator's bridge to the Rust side.
//!
//! One entry point, [`call`], takes a JSON command and returns JSON. That
//! keeps the WebAssembly interface to three functions and needs no binding
//! generator. The commands cover two things: turning protocol messages into
//! HID reports and back, and running virtual keyboards.

use std::cell::RefCell;
use std::collections::HashMap;

use jmk_proto::frame::{self, Assembler};
use jmk_proto::{DeviceMessage, Request};
use jmk_sim::{HostEvent, VirtualKeyboard, Wiring};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    /// Request -> list of 64-byte reports.
    Encode { request: Request },
    /// Feed one report from a device; yields a message once complete.
    /// `channel` keeps the partial messages of different devices apart.
    Decode { channel: u32, report: Vec<u8> },
    /// Forget a channel's partial message (device unplugged).
    Close { channel: u32 },
    Create { wiring: Wiring, uid: [u8; 8] },
    Destroy { id: u32 },
    /// Join two halves with a cable.
    Connect { a: u32, b: u32 },
    Cable { id: u32, plugged: bool },
    Key { id: u32, key: usize, pressed: bool },
    Encoder { id: u32, encoder: usize, clockwise: bool },
    Send { id: u32, report: Vec<u8> },
    /// Let time pass on several keyboards together, a millisecond at a time,
    /// so two halves keep talking. Returns what each one produced.
    Advance { ids: Vec<u32>, ms: u64 },
    /// Finish a simulated firmware update: leave the bootloader.
    Flash { id: u32 },
}

#[derive(Default)]
struct State {
    assemblers: HashMap<u32, Assembler>,
    keyboards: HashMap<u32, VirtualKeyboard>,
    next_id: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Run one command. Errors come back as `{"error": "..."}`.
pub fn run(command: &str) -> String {
    let result = serde_json::from_str::<Command>(command)
        .map_err(|e| e.to_string())
        .and_then(|command| STATE.with(|state| execute(&mut state.borrow_mut(), command)));
    match result {
        Ok(value) => value.to_string(),
        Err(message) => json!({ "error": message }).to_string(),
    }
}

fn keyboard(state: &mut State, id: u32) -> Result<&mut VirtualKeyboard, String> {
    state.keyboards.get_mut(&id).ok_or_else(|| format!("no virtual keyboard {id}"))
}

fn execute(state: &mut State, command: Command) -> Result<Value, String> {
    match command {
        Command::Encode { request } => {
            let mut reports: Vec<Vec<u8>> = vec![];
            frame::encode(&request, |report| reports.push(report.to_vec())).map_err(|e| e.to_string())?;
            Ok(json!({ "reports": reports }))
        }
        Command::Decode { channel, report } => {
            let assembler = state.assemblers.entry(channel).or_default();
            match assembler.push::<DeviceMessage>(&report) {
                None => Ok(Value::Null),
                Some(Ok(message)) => Ok(json!({ "message": message })),
                Some(Err(e)) => Err(e.to_string()),
            }
        }
        Command::Close { channel } => {
            state.assemblers.remove(&channel);
            Ok(Value::Null)
        }
        Command::Create { wiring, uid } => {
            state.next_id += 1;
            state.keyboards.insert(state.next_id, VirtualKeyboard::new(wiring, uid));
            Ok(json!({ "id": state.next_id }))
        }
        Command::Destroy { id } => {
            state.keyboards.remove(&id);
            Ok(Value::Null)
        }
        Command::Connect { a, b } => {
            let mut first = state.keyboards.remove(&a).ok_or("no such keyboard")?;
            let result = keyboard(state, b).map(|second| VirtualKeyboard::connect(&mut first, second));
            state.keyboards.insert(a, first);
            result.map(|()| Value::Null)
        }
        Command::Cable { id, plugged } => {
            keyboard(state, id)?.set_cable(plugged);
            Ok(Value::Null)
        }
        Command::Key { id, key, pressed } => {
            keyboard(state, id)?.set_key(key, pressed);
            Ok(Value::Null)
        }
        Command::Encoder { id, encoder, clockwise } => {
            keyboard(state, id)?.turn_encoder(encoder, clockwise);
            Ok(Value::Null)
        }
        Command::Send { id, report } => {
            keyboard(state, id)?.send_report(&report);
            Ok(Value::Null)
        }
        Command::Advance { ids, ms } => {
            for _ in 0..ms {
                for id in &ids {
                    keyboard(state, *id)?.advance(1);
                }
            }
            let mut out = serde_json::Map::new();
            for id in ids {
                let keyboard = keyboard(state, id)?;
                let reports: Vec<Vec<u8>> = keyboard.take_reports().iter().map(|r| r.to_vec()).collect();
                let host: Vec<HostEvent> = keyboard.take_host_events();
                out.insert(
                    id.to_string(),
                    json!({ "reports": reports, "host": host, "bootloader": keyboard.in_bootloader() }),
                );
            }
            Ok(Value::Object(out))
        }
        Command::Flash { id } => {
            keyboard(state, id)?.reboot();
            Ok(Value::Null)
        }
    }
}

// --- WebAssembly interface -------------------------------------------------
// The caller allocates a buffer, writes the command into it, and gets back a
// pointer and length packed into one 64-bit value. It frees both buffers.

#[no_mangle]
pub extern "C" fn jmk_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from [`jmk_alloc`] or [`jmk_call`].
#[no_mangle]
pub unsafe extern "C" fn jmk_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}

/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 written into a [`jmk_alloc`] buffer.
#[no_mangle]
pub unsafe extern "C" fn jmk_call(ptr: *const u8, len: usize) -> u64 {
    let command = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("");
    let mut reply = run(command).into_bytes();
    reply.shrink_to_fit();
    let packed = (reply.as_mut_ptr() as u64) << 32 | reply.len() as u64;
    std::mem::forget(reply);
    packed
}

#[cfg(test)]
mod tests {
    use super::run;
    use serde_json::{json, Value};

    fn call(command: Value) -> Value {
        serde_json::from_str(&run(&command.to_string())).unwrap()
    }

    #[test]
    fn a_virtual_keyboard_answers_through_the_bridge() {
        let id = call(json!({"op": "create", "uid": [1,2,3,4,5,6,7,8],
            "wiring": {"switches": [{"Ground": 7}], "encoders": []}}))["id"]
            .clone();
        let reports = call(json!({"op": "encode", "request": "GetInfo"}))["reports"].clone();
        call(json!({"op": "send", "id": id, "report": reports[0]}));
        let out = call(json!({"op": "advance", "ids": [id], "ms": 1}));
        let report = &out[id.to_string()]["reports"][0];
        let message = call(json!({"op": "decode", "channel": 1, "report": report}));
        assert_eq!(message["message"]["Response"]["Info"]["mcu"], "Simulated");
        assert_eq!(message["message"]["Response"]["Info"]["uid"], json!([1, 2, 3, 4, 5, 6, 7, 8]));
    }

    #[test]
    fn bad_commands_return_an_error() {
        assert!(call(json!({"op": "nope"}))["error"].is_string());
        assert!(call(json!({"op": "key", "id": 99, "key": 0, "pressed": true}))["error"].is_string());
    }
}
