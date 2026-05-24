//! Resolve struct field offsets, module globals, Source 2 interfaces and button
//! addresses at runtime with compile-time literal fallbacks.
//!
//! Declare modules with the attribute macros below. When the `runtime` feature is
//! enabled and [`init`] has been called with a [`Process`] implementation, the
//! generated accessors return live values from the target process. Without the
//! feature (or before `init`), they return the declared literal.
//!
//! The crate supports `#![no_std]` + `alloc`.
//!
//! # Using With MinHook
//!
//! `dynoffsets` does not provide a hook engine. It resolves runtime addresses;
//! a library such as MinHook installs the detour.
//!
//! The usual flow is:
//!
//! 1. Resolve a live interface pointer with [`interfaces`].
//! 2. Read the vtable slot for the method you want to hook.
//! 3. Pass that function entry address to MinHook.
//!
//! [`schema`] and [`globals`] usually resolve data addresses rather than hook
//! targets, so [`interfaces`] is the most common fit for MinHook-based setups.
//!
//! ```rust,ignore
//! use core::{ffi::c_void, mem, ptr};
//!
//! use dynoffsets::interfaces;
//! use minhook_sys::{MH_CreateHook, MH_EnableHook, MH_Initialize, MH_OK};
//!
//! #[interfaces("engine2.dll")]
//! mod engine2 {
//!     pub const Source2EngineToClient001: usize = 0;
//! }
//!
//! type TargetFn = unsafe extern "system" fn(this: *mut c_void, arg: i32) -> i32;
//!
//! static mut ORIGINAL_TARGET: Option<TargetFn> = None;
//!
//! unsafe extern "system" fn hk_target(this: *mut c_void, arg: i32) -> i32 {
//!     let original = ORIGINAL_TARGET.expect("hook not installed");
//!     original(this, arg)
//! }
//!
//! unsafe fn vfunc(instance: usize, index: usize) -> *mut c_void {
//!     let vtable = *(instance as *const *const usize);
//!     *vtable.add(index) as *mut c_void
//! }
//!
//! unsafe fn install_hook() {
//!     let iface = engine2::Source2EngineToClient001();
//!     assert_ne!(iface, 0, "interface was not resolved");
//!
//!     let target = vfunc(iface, 42);
//!
//!     assert_eq!(MH_Initialize(), MH_OK);
//!
//!     let mut original = ptr::null_mut();
//!     assert_eq!(
//!         MH_CreateHook(target, hk_target as *mut c_void, &mut original),
//!         MH_OK,
//!     );
//!
//!     ORIGINAL_TARGET = Some(mem::transmute(original));
//!
//!     assert_eq!(MH_EnableHook(target), MH_OK);
//! }
//! ```
//!
//! The same pattern works for exported functions when your backend can resolve
//! the function entry address.

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;

/// `#[schema]` / `#[schema("client.dll")]` / `#[schema(false)]` / `#[schema(..., r#static)]` / `#[schema(hashed)]`
///
/// Turns each `pub const NAME: usize = LIT;` into `pub fn NAME() -> usize`.
/// Returns live value when runtime discovery succeeds, else the literal.
///
/// `hashed` emits
/// `lookup_or_fallback_h(fnv1a("dll"), "dll".len(), fnv1a("Class"), "Class".len(), fnv1a("field"), "field".len(), lit)`
/// so no schema name strings appear in the caller's `.rdata`. The length pairs
/// fold to `u16` immediates in `.text`.
///
/// With `r#static`, emits `AtomicUsize` per offset + `__dynoffsets_register()`.
/// Call the register fn after `init`, then `populate()` to fill the cells. (hashed ignored in static mode)
pub use dynoffsets_macros::schema;

/// `#[globals]` / `#[globals(r#static)]`
///
/// Turns `pub const NAME: usize = LIT;` into `pub fn NAME() -> usize`
/// returning the live global or the literal fallback.
///
/// `r#static` mode: `AtomicUsize` cells + register fn; populate after init.
pub use dynoffsets_macros::globals;

/// `#[interfaces]` / `#[interfaces("dll")]` / `#[interfaces(false)]` / `#[interfaces(..., r#static)]`
///
/// Rewrites interface consts to fns returning the live pointer or literal.
///
/// `r#static`: per-item `AtomicUsize` + register fn; fill via `populate()`.
pub use dynoffsets_macros::interfaces;

/// `#[buttons]` / `#[buttons(r#static)]`
///
/// Rewrites button consts to fns returning live button state addr or literal.
///
/// `r#static`: `AtomicUsize` per button + register fn; populate after init.
pub use dynoffsets_macros::buttons;

#[path = "static.rs"]
mod r#static;
mod sync;

pub use r#static::{populate, PopulateStats, Slot};

// Hidden re-exports invoked by macro-generated `__dynoffsets_register()`
// functions. Not part of the stable API; do not call directly.
#[doc(hidden)]
pub use r#static::register_buttons as __register_buttons_static;
#[doc(hidden)]
pub use r#static::register_globals as __register_globals_static;
#[doc(hidden)]
pub use r#static::register_interfaces as __register_interfaces_static;
#[doc(hidden)]
pub use r#static::register_schema as __register_schema_static;

/// Hidden re-export used by macro-generated accessors so they can cache their
/// resolved offset in a `OnceCell<usize>` without depending on
/// `std::sync::OnceLock` (which would break `default-features = false` builds).
#[doc(hidden)]
pub use sync::OnceCell as __AccessorCell;

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

#[cfg(not(feature = "runtime"))]
impl RuntimeGlobals {
    /// Stub `get` for the no-runtime build: always returns `None`.
    #[inline]
    pub fn get(&self, _name: &str) -> Option<usize> {
        None
    }
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
/// Implementors provide raw reads and module lookups for every runtime path,
/// including pattern scanning through `pe-sigscan`'s reader-backed APIs.
/// Override the typed `read_*` methods for zero-copy if your backend already
/// has them.
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

/// Returns `true` after [`init`] has been called.
///
/// Lightweight (one atomic load). Used by the macro-generated accessors to
/// avoid caching the literal fallback before a process backend is installed.
#[inline]
pub fn is_initialized() -> bool {
    PROCESS.get().is_some()
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

/// FNV-1a 32-bit hash (const-eval).
///
/// Use with `#[schema(hashed)]` (or `#[schema("client.dll", hashed)]`) so the
/// macro emits only `u32` literals via `fnv1a("Name")`; the input strings
/// exist only at compile time and do not appear in the final `.rdata`.
pub const fn fnv1a(s: &str) -> u32 {
    fnv1a_bytes(s.as_bytes())
}

/// Internal byte slice variant.
pub(crate) const fn fnv1a_bytes(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let prime: u32 = 0x01000193;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(prime);
        i += 1;
    }
    hash
}

#[inline]
const fn str_len_u16(s: &str) -> u16 {
    let len = s.len();
    if len > u16::MAX as usize {
        u16::MAX
    } else {
        len as u16
    }
}

/// Internal helper used by the `#[schema]` macro.
///
/// Returns the runtime offset if the schema walker found it, otherwise `fallback`.
#[inline]
pub fn lookup_or_fallback(module: &str, class: &str, field: &str, fallback: usize) -> usize {
    let mh = fnv1a(module);
    let ch = fnv1a(class);
    let fh = fnv1a(field);
    lookup_or_fallback_h(
        mh,
        str_len_u16(module),
        ch,
        str_len_u16(class),
        fh,
        str_len_u16(field),
        fallback,
    )
}

/// Internal helper used by the cached `#[schema]` accessor.
///
/// Returns `Some(off)` only when the schema walker successfully resolved the
/// field. The macro uses this to gate the per-accessor cache write so a miss
/// (e.g. schema system not yet populated) does not latch the literal fallback.
#[inline]
pub fn try_lookup_offset(module: &str, class: &str, field: &str) -> Option<usize> {
    let mh = fnv1a(module);
    let ch = fnv1a(class);
    let fh = fnv1a(field);
    try_lookup_offset_h(
        mh,
        str_len_u16(module),
        ch,
        str_len_u16(class),
        fh,
        str_len_u16(field),
    )
}

/// Hash-keyed variant of [`lookup_or_fallback`].
///
/// Intended for `#[schema(hashed)]` emission so no `&str` literals reach the
/// caller's `.rdata`. The three hashes are produced at compile time by `fnv1a`;
/// the three lengths are emitted by the macro as `u16` immediates next to each
/// hash and are checked alongside it to discriminate hash collisions.
#[inline]
pub fn lookup_or_fallback_h(
    dll_hash: u32,
    dll_len: u16,
    class_hash: u32,
    class_len: u16,
    field_hash: u32,
    field_len: u16,
    fallback: usize,
) -> usize {
    try_lookup_offset_h(
        dll_hash, dll_len, class_hash, class_len, field_hash, field_len,
    )
    .unwrap_or(fallback)
}

/// Hash-keyed variant of [`try_lookup_offset`].
#[inline]
pub fn try_lookup_offset_h(
    dll_hash: u32,
    dll_len: u16,
    class_hash: u32,
    class_len: u16,
    field_hash: u32,
    field_len: u16,
) -> Option<usize> {
    #[cfg(feature = "runtime")]
    {
        walker::lookup_offset_h(
            dll_hash, dll_len, class_hash, class_len, field_hash, field_len,
        )
        .map(|off| off as usize)
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (
            dll_hash, dll_len, class_hash, class_len, field_hash, field_len,
        );
        None
    }
}
