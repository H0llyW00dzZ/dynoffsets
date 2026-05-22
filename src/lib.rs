//! Resolve struct field offsets, module globals, Source 2 interfaces and button
//! addresses at runtime with compile-time literal fallbacks.
//!
//! Declare modules with the attribute macros below. When the `runtime` feature is
//! enabled and [`init`] has been called with a [`Process`] implementation, the
//! generated accessors return live values from the target process. Without the
//! feature (or before `init`), they return the declared literal.
//!
//! The crate supports `#![no_std]` + `alloc`.

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;

pub use dynoffsets_macros::{buttons, globals, interfaces, schema};

mod sync;

#[cfg(all(test, feature = "runtime"))]
mod mock;
#[cfg(all(test, feature = "runtime"))]
mod tests;

#[cfg(feature = "runtime")]
mod buttons;
#[cfg(feature = "runtime")]
mod interfaces;
#[cfg(feature = "runtime")]
mod mem;
#[cfg(feature = "runtime")]
mod offsets;
#[cfg(feature = "runtime")]
mod sigscan;
#[cfg(feature = "runtime")]
mod walker;

#[cfg(feature = "runtime")]
pub use buttons::{discover_buttons, RuntimeButtons};
#[cfg(feature = "runtime")]
pub use interfaces::{discover_interfaces, discover_interfaces_in, RuntimeInterfaces};
#[cfg(feature = "runtime")]
pub use offsets::{discover_globals, RuntimeGlobals};

/// Stub version of [`RuntimeGlobals`] used when the `runtime` feature is disabled.
#[cfg(not(feature = "runtime"))]
#[derive(Debug, Default, Clone)]
pub struct RuntimeGlobals {
    pub dw_csgo_input: Option<usize>,
    pub dw_entity_list: Option<usize>,
    pub dw_game_entity_system: Option<usize>,
    pub dw_game_entity_system_highest_entity_index: Option<usize>,
    pub dw_game_rules: Option<usize>,
    pub dw_global_vars: Option<usize>,
    pub dw_glow_manager: Option<usize>,
    pub dw_local_player_controller: Option<usize>,
    pub dw_local_player_pawn: Option<usize>,
    pub dw_planted_c4: Option<usize>,
    pub dw_prediction: Option<usize>,
    pub dw_sensitivity: Option<usize>,
    pub dw_view_angles: Option<usize>,
    pub dw_view_matrix: Option<usize>,
    pub dw_view_render: Option<usize>,
    pub dw_weapon_c4: Option<usize>,

    pub dw_build_number: Option<usize>,
    pub dw_network_game_client: Option<usize>,
    pub dw_network_game_client_is_background_map: Option<usize>,
    pub dw_network_game_client_local_player: Option<usize>,
    pub dw_network_game_client_max_clients: Option<usize>,
    pub dw_network_game_client_sign_on_state: Option<usize>,
    pub dw_window_height: Option<usize>,
    pub dw_window_width: Option<usize>,

    pub dw_input_system: Option<usize>,
    pub dw_game_types: Option<usize>,
    pub dw_sound_system: Option<usize>,
}

/// Stub when `runtime` is disabled.
#[cfg(not(feature = "runtime"))]
#[derive(Debug, Default, Clone)]
pub struct RuntimeInterfaces;

#[cfg(not(feature = "runtime"))]
impl RuntimeInterfaces {
    #[inline]
    pub fn get(&self, _module: &str, _name: &str) -> Option<usize> {
        None
    }
}

/// Stub when `runtime` is disabled.
#[cfg(not(feature = "runtime"))]
#[derive(Debug, Default, Clone)]
pub struct RuntimeButtons;

#[cfg(not(feature = "runtime"))]
impl RuntimeButtons {
    #[inline]
    pub fn get(&self, _name: &str) -> Option<usize> {
        None
    }
}

/// Memory access trait for the runtime dumper.
///
/// Implementors provide raw reads and module lookups. Override the typed
/// `read_*` methods for zero-copy if your backend already has them.
pub trait Process: Send + Sync + 'static {
    fn read_bytes(&self, addr: usize, buf: &mut [u8]) -> Option<()>;
    fn module_base(&self, module: &str) -> Option<usize>;

    fn get_interface(&self, _module: &str, _name: &str) -> Option<usize> {
        None
    }
    fn get_proc_address(&self, _module: &str, _proc_name: &str) -> Option<usize> {
        None
    }

    fn read_usize(&self, addr: usize) -> Option<usize> {
        let mut buf = [0u8; core::mem::size_of::<usize>()];
        self.read_bytes(addr, &mut buf)?;
        Some(usize::from_le_bytes(buf))
    }
    fn read_u32(&self, addr: usize) -> Option<u32> {
        let mut buf = [0u8; 4];
        self.read_bytes(addr, &mut buf)?;
        Some(u32::from_le_bytes(buf))
    }
    fn read_i16(&self, addr: usize) -> Option<i16> {
        let mut buf = [0u8; 2];
        self.read_bytes(addr, &mut buf)?;
        Some(i16::from_le_bytes(buf))
    }
    fn read_cstring(&self, addr: usize, max_len: usize) -> Option<String> {
        let mut buf = vec![0u8; max_len];
        self.read_bytes(addr, &mut buf)?;
        let end = buf.iter().position(|&b| b == 0).unwrap_or(max_len);
        core::str::from_utf8(&buf[..end]).ok().map(Into::into)
    }
}

static PROCESS: sync::OnceCell<Box<dyn Process>> = sync::OnceCell::new();

/// Register the process backend used by all runtime accessors.
///
/// Must be called once at startup, before any `#[schema]` or `#[globals]`
/// accessor runs. First call wins; later calls are ignored.
pub fn init<P: Process>(p: P) {
    let _ = PROCESS.set(Box::new(p));
}

#[cfg(feature = "runtime")]
pub(crate) fn process() -> Option<&'static dyn Process> {
    PROCESS.get().map(|b| &**b)
}

/// Returns the lazily-discovered global pointers (pattern scan).
///
/// `None` until [`init`] has been called or if the `runtime` feature is disabled.
pub fn get_runtime_globals() -> Option<&'static RuntimeGlobals> {
    #[cfg(feature = "runtime")]
    {
        static G: sync::OnceCell<RuntimeGlobals> = sync::OnceCell::new();
        process()?;
        Some(G.get_or_init(offsets::discover_globals))
    }
    #[cfg(not(feature = "runtime"))]
    {
        None
    }
}

/// Returns the discovered Source 2 interfaces (via CreateInterface chains).
pub fn get_runtime_interfaces() -> Option<&'static RuntimeInterfaces> {
    #[cfg(feature = "runtime")]
    {
        static I: sync::OnceCell<RuntimeInterfaces> = sync::OnceCell::new();
        process()?;
        Some(I.get_or_init(interfaces::discover_interfaces))
    }
    #[cfg(not(feature = "runtime"))]
    {
        None
    }
}

/// Returns the discovered button state addresses from client.dll's KeyButton list.
pub fn get_runtime_buttons() -> Option<&'static RuntimeButtons> {
    #[cfg(feature = "runtime")]
    {
        static B: sync::OnceCell<RuntimeButtons> = sync::OnceCell::new();
        process()?;
        Some(B.get_or_init(buttons::discover_buttons))
    }
    #[cfg(not(feature = "runtime"))]
    {
        None
    }
}

/// Internal helper used by the `#[schema]` macro.
///
/// Returns the runtime offset if the schema walker found it, otherwise `fallback`.
#[inline]
pub fn lookup_or_fallback(module: &str, class: &str, field: &str, fallback: usize) -> usize {
    #[cfg(feature = "runtime")]
    if let Some(off) = walker::lookup_offset(module, class, field) {
        return off as usize;
    }
    let _ = (module, class, field);
    fallback
}
