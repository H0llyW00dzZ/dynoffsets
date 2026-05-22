//! Live button address discovery from client.dll's KeyButton list.
//!
//! Used by the `#[buttons]` macro to turn button name constants into
//! functions that return the runtime `&state` pointer.

use alloc::string::{String, ToString};

use hashbrown::HashMap;

use crate::mem;
use crate::sigscan::find_pattern_rip32;

/// Button name → address of its `state: u32` field, walked from the live
/// `g_pButtonList` linked list in client.dll.
#[derive(Debug, Default, Clone)]
pub struct RuntimeButtons {
    pub map: HashMap<String, usize>,
}

impl RuntimeButtons {
    #[inline]
    pub fn get(&self, name: &str) -> Option<usize> {
        self.map.get(name).copied()
    }
}

const BTN_PATTERN: &[Option<u8>] = &[
    Some(0x48),
    Some(0x8B),
    Some(0x15),
    None,
    None,
    None,
    None,
    Some(0x48),
    Some(0x85),
    Some(0xD2),
    Some(0x74),
    None,
    Some(0x48),
    Some(0x8B),
    Some(0x02),
    Some(0x48),
    Some(0x85),
    Some(0xC0),
];
const BTN_DISP: usize = 3;

const NAME_OFF: usize = 0x08;
const STATE_OFF: usize = 0x30;
const NEXT_OFF: usize = 0x88;
const MAX_CHAIN: usize = 1024;
const MAX_NAME: usize = 32;

/// Walk the live `KeyButton` linked list in `client.dll` and collect
/// button name → `&state` mappings.
///
/// The returned addresses point at the `u32` state fields (read them to get
/// the current pressed bitmask). Used by the `#[buttons]` macro.
pub fn discover_buttons() -> RuntimeButtons {
    let mut out = RuntimeButtons::default();

    let cell = match find_pattern_rip32("client.dll", BTN_PATTERN, BTN_DISP) {
        Some(a) => a,
        None => return out,
    };
    let mut node = match mem::read_usize(cell) {
        Some(v) if v != 0 => v,
        _ => return out,
    };

    for _ in 0..MAX_CHAIN {
        let Some(np) = mem::read_usize_off(node, NAME_OFF) else {
            break;
        };
        if np == 0 {
            break;
        }
        let Some(name) = mem::read_cstring(np, MAX_NAME) else {
            break;
        };

        if let Some(state) = node.checked_add(STATE_OFF) {
            out.map.insert(name.to_string(), state);
        }

        match mem::read_usize_off(node, NEXT_OFF) {
            Some(0) | None => break,
            Some(n) => node = n,
        }
    }
    out
}
