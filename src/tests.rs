//! Comprehensive unit tests targeting full line coverage.
//!
//! All runtime-state tests serialize on `mock::TEST_LOCK` because the registered
//! process and the various lazy statics are crate-globals.

#![allow(dead_code)]
#![cfg(all(test, feature = "runtime"))]

use alloc::string::String;

use crate::mem;
use crate::mock::{populate, setup};
use crate::sigscan;
use crate::sync::{Mutex as SpinMutex, OnceCell};
use crate::walker;
use crate::{
    buttons, get_runtime_buttons, get_runtime_globals, get_runtime_interfaces, init, interfaces,
    lookup_or_fallback, offsets, Process, RuntimeButtons, RuntimeInterfaces,
};

/// Allocate a 7-byte fake instruction in the heap whose rel32 (at +3) points
/// at `inst_addr + 7 + offset`. Returns `(inst_addr, target)`. Buffer is leaked.
fn rel32_instr(offset: i32) -> (usize, usize) {
    let v: alloc::vec::Vec<u8> = alloc::vec![0u8; 7];
    let mut b = v.into_boxed_slice();
    let inst_addr = b.as_ptr() as usize;
    let next_ip = inst_addr + 7;
    b[3..7].copy_from_slice(&offset.to_le_bytes());
    let _ = alloc::boxed::Box::leak(b);
    let target = (next_ip as isize).wrapping_add(offset as isize) as usize;
    (inst_addr, target)
}

/// Leak a heap buffer with a u32 little-endian at `imm_off`; return the buffer
/// base address. Used to exercise `find_pattern_u32`'s real-path unaligned read.
fn imm32_at(imm_off: usize, val: u32) -> usize {
    let mut v: alloc::vec::Vec<u8> = alloc::vec![0u8; imm_off + 4];
    v[imm_off..imm_off + 4].copy_from_slice(&val.to_le_bytes());
    let b = v.into_boxed_slice();
    let addr = b.as_ptr() as usize;
    let _ = alloc::boxed::Box::leak(b);
    addr
}

/// Same shape as [`imm32_at`] but for a single byte.
fn imm8_at(imm_off: usize, val: u8) -> usize {
    let mut v: alloc::vec::Vec<u8> = alloc::vec![0u8; imm_off + 1];
    v[imm_off] = val;
    let b = v.into_boxed_slice();
    let addr = b.as_ptr() as usize;
    let _ = alloc::boxed::Box::leak(b);
    addr
}

#[test]
fn lookup_or_fallback_with_no_match_returns_fallback() {
    let _g = setup();
    let v = lookup_or_fallback("client.dll", "C_NoSuch", "m_field", 0x99);
    assert_eq!(v, 0x99);
}

struct DummyProc;
impl Process for DummyProc {
    fn read_bytes(&self, _: usize, _: &mut [u8]) -> Option<()> {
        None
    }
    fn module_base(&self, _: &str) -> Option<usize> {
        None
    }
}

#[test]
fn init_is_idempotent() {
    let _g = setup();
    init(DummyProc);
    populate(|s| s.add_module("client.dll", 0x1234));
    assert_eq!(crate::process().unwrap().module_base("client.dll"), Some(0x1234));
}

#[test]
fn get_runtime_globals_returns_some_when_process_installed() {
    let _g = setup();
    assert!(get_runtime_globals().is_some());
}

#[test]
fn get_runtime_interfaces_returns_some_when_process_installed() {
    let _g = setup();
    assert!(get_runtime_interfaces().is_some());
}

#[test]
fn get_runtime_buttons_returns_some_when_process_installed() {
    let _g = setup();
    assert!(get_runtime_buttons().is_some());
}

// ---------------------------------------------------------------------------
// Process trait default implementations
// ---------------------------------------------------------------------------

struct AlwaysNoneProc;
impl Process for AlwaysNoneProc {
    fn read_bytes(&self, _: usize, _: &mut [u8]) -> Option<()> {
        None
    }
    fn module_base(&self, _: &str) -> Option<usize> {
        None
    }
}

#[test]
fn process_defaults_all_none_when_read_fails() {
    let p = AlwaysNoneProc;
    assert_eq!(p.get_interface("a", "b"), None);
    assert_eq!(p.get_proc_address("a", "b"), None);
    assert_eq!(p.read_usize(0), None);
    assert_eq!(p.read_u32(0), None);
    assert_eq!(p.read_i16(0), None);
    assert_eq!(p.read_cstring(0, 16), None);
}

struct SequentialReadProc;
impl Process for SequentialReadProc {
    fn read_bytes(&self, _addr: usize, buf: &mut [u8]) -> Option<()> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (i + 1) as u8;
        }
        Some(())
    }
    fn module_base(&self, _: &str) -> Option<usize> {
        None
    }
}

#[test]
fn process_default_read_usize_decodes_little_endian() {
    let p = SequentialReadProc;
    let want = usize::from_le_bytes([1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(p.read_usize(0), Some(want));
}

#[test]
fn process_default_read_u32_decodes_little_endian() {
    let p = SequentialReadProc;
    assert_eq!(p.read_u32(0), Some(u32::from_le_bytes([1, 2, 3, 4])));
}

#[test]
fn process_default_read_i16_decodes_little_endian() {
    let p = SequentialReadProc;
    assert_eq!(p.read_i16(0), Some(i16::from_le_bytes([1, 2])));
}

struct StaticBytesProc(&'static [u8]);
impl Process for StaticBytesProc {
    fn read_bytes(&self, _addr: usize, buf: &mut [u8]) -> Option<()> {
        if buf.len() > self.0.len() {
            return None;
        }
        buf.copy_from_slice(&self.0[..buf.len()]);
        Some(())
    }
    fn module_base(&self, _: &str) -> Option<usize> {
        None
    }
}

#[test]
fn process_default_read_cstring_finds_nul() {
    let p = StaticBytesProc(b"hello\0xxxxx");
    assert_eq!(p.read_cstring(0, 11), Some(String::from("hello")));
}

#[test]
fn process_default_read_cstring_uses_max_len_when_no_nul() {
    let p = StaticBytesProc(b"abcde");
    assert_eq!(p.read_cstring(0, 5), Some(String::from("abcde")));
}

#[test]
fn process_default_read_cstring_rejects_non_utf8() {
    static BAD: &[u8] = &[0xFF, 0xFE, 0x00];
    let p = StaticBytesProc(BAD);
    assert_eq!(p.read_cstring(0, 3), None);
}

// ---------------------------------------------------------------------------
// sync::OnceCell shim
// ---------------------------------------------------------------------------

#[test]
fn once_cell_get_returns_none_before_init() {
    let c: OnceCell<u32> = OnceCell::new();
    assert!(c.get().is_none());
}

#[test]
fn once_cell_set_first_wins_second_loses() {
    let c: OnceCell<u32> = OnceCell::new();
    assert_eq!(c.set(7), Ok(()));
    assert_eq!(c.set(8), Err(8));
    assert_eq!(c.get(), Some(&7));
}

#[test]
fn once_cell_get_or_init_initialises_once() {
    let c: OnceCell<u32> = OnceCell::new();
    let v = c.get_or_init(|| 42);
    assert_eq!(*v, 42);
    let v2 = c.get_or_init(|| 99);
    assert_eq!(*v2, 42);
}

#[test]
fn once_cell_default_is_uninitialised() {
    let c: OnceCell<u32> = OnceCell::default();
    assert!(c.get().is_none());
}

#[test]
fn once_cell_set_after_already_completed_returns_err() {
    let c: OnceCell<u32> = OnceCell::new();
    c.get_or_init(|| 1);
    assert_eq!(c.set(5), Err(5));
}

#[test]
fn spin_mutex_lock_works() {
    let m: SpinMutex<u32> = SpinMutex::new(11);
    let g = m.lock();
    assert_eq!(*g, 11);
}

// ---------------------------------------------------------------------------
// mem.rs forwarders
// ---------------------------------------------------------------------------

#[test]
fn mem_read_usize_off_overflow_returns_none() {
    let _g = setup();
    assert!(mem::read_usize_off(usize::MAX - 3, 100).is_none());
}

#[test]
fn mem_read_u32_off_overflow_returns_none() {
    let _g = setup();
    assert!(mem::read_u32_off(usize::MAX - 3, 100).is_none());
}

#[test]
fn mem_read_i16_off_overflow_returns_none() {
    let _g = setup();
    assert!(mem::read_i16_off(usize::MAX - 3, 100).is_none());
}

#[test]
fn mem_read_usize_off_happy_path() {
    let _g = setup();
    populate(|s| s.write_usize(0x1008, 0xDEAD_BEEFusize));
    assert_eq!(mem::read_usize_off(0x1000, 8), Some(0xDEAD_BEEFusize));
}

#[test]
fn mem_read_u32_off_happy_path() {
    let _g = setup();
    populate(|s| s.write_u32(0x2008, 0xCAFE_BABE));
    assert_eq!(mem::read_u32_off(0x2000, 8), Some(0xCAFE_BABE));
}

#[test]
fn mem_read_i16_off_happy_path() {
    let _g = setup();
    populate(|s| s.write_i16(0x3004, -42));
    assert_eq!(mem::read_i16_off(0x3000, 4), Some(-42));
}

#[test]
fn mem_read_ptr_returns_none_for_null() {
    let _g = setup();
    populate(|s| s.write_usize(0x4000, 0));
    assert!(mem::read_ptr(0x4000, 0).is_none());
}

#[test]
fn mem_read_ptr_returns_some_for_non_null() {
    let _g = setup();
    populate(|s| s.write_usize(0x5000, 0x9999));
    assert_eq!(mem::read_ptr(0x5000, 0), Some(0x9999));
}

#[test]
fn mem_read_cstring_via_global_process() {
    let _g = setup();
    populate(|s| s.write_cstr(0x6000, "hi"));
    assert_eq!(mem::read_cstring(0x6000, 8), Some(String::from("hi")));
}

#[test]
fn mem_read_usize_returns_none_when_denied() {
    let _g = setup();
    populate(|s| s.deny_read(0x7000));
    assert!(mem::read_usize(0x7000).is_none());
}

#[test]
fn mem_read_u32_returns_none_when_denied() {
    let _g = setup();
    populate(|s| s.deny_read(0x7100));
    assert!(mem::read_u32(0x7100).is_none());
}

#[test]
fn mem_read_i16_returns_none_when_denied() {
    let _g = setup();
    populate(|s| s.deny_read(0x7200));
    assert!(mem::read_i16(0x7200).is_none());
}

// ---------------------------------------------------------------------------
// sigscan.rs
// ---------------------------------------------------------------------------

#[test]
fn sigscan_resolve_rel32_at_disp_off_too_large() {
    assert!(sigscan::resolve_rel32_at(0x1000, 8, 5).is_none());
}

#[test]
fn sigscan_resolve_rel32_at_disp_off_overflow() {
    assert!(sigscan::resolve_rel32_at(0x1000, usize::MAX, 0).is_none());
}

#[test]
fn sigscan_resolve_rel32_at_happy_path() {
    let (inst, target) = rel32_instr(100);
    assert_eq!(sigscan::resolve_rel32_at(inst, 3, 7), Some(target));
}

#[test]
fn sigscan_resolve_rip32_at_overflow_in_instr_len() {
    assert!(sigscan::resolve_rip32_at(0x1000, usize::MAX).is_none());
}

#[test]
fn sigscan_resolve_rip32_at_happy_path() {
    let (inst, target) = rel32_instr(-200);
    assert_eq!(sigscan::resolve_rip32_at(inst, 3), Some(target));
}

#[test]
fn sigscan_find_pattern_override_some() {
    let _g = setup();
    let pat = &[Some(0x90u8), None][..];
    sigscan::set_pattern("any.dll", pat, Some(0xABCD));
    assert_eq!(sigscan::find_pattern("any.dll", pat), Some(0xABCD));
}

#[test]
fn sigscan_find_pattern_override_none() {
    let _g = setup();
    let pat = &[Some(0x90u8)][..];
    sigscan::set_pattern("any.dll", pat, None);
    assert!(sigscan::find_pattern("any.dll", pat).is_none());
}

#[test]
fn sigscan_find_pattern_no_override_no_module() {
    let _g = setup();
    assert!(sigscan::find_pattern("absent.dll", &[Some(0x90)]).is_none());
}

#[test]
fn sigscan_find_pattern_rip32_via_rip32_override() {
    let _g = setup();
    let pat = &[Some(0xAAu8)][..];
    sigscan::set_pattern_rip32("m.dll", pat, 3, Some(0xDEAD));
    assert_eq!(sigscan::find_pattern_rip32("m.dll", pat, 3), Some(0xDEAD));
}

#[test]
fn sigscan_find_pattern_rip32_via_raw_override_then_resolve() {
    let _g = setup();
    let (inst, target) = rel32_instr(64);
    let pat = &[Some(0xBBu8)][..];
    sigscan::set_pattern("m2.dll", pat, Some(inst));
    assert_eq!(sigscan::find_pattern_rip32("m2.dll", pat, 3), Some(target));
}

#[test]
fn sigscan_find_pattern_rip32_propagates_none() {
    let _g = setup();
    let pat = &[Some(0xAAu8)][..];
    sigscan::set_pattern("m.dll", pat, None);
    assert!(sigscan::find_pattern_rip32("m.dll", pat, 3).is_none());
}

#[test]
fn sigscan_find_pattern_u32_via_override_some_and_none() {
    let _g = setup();
    let pat = &[Some(0xCCu8)][..];
    sigscan::set_pattern_u32("m.dll", pat, 2, Some(0xDEAD_BEEF));
    assert_eq!(sigscan::find_pattern_u32("m.dll", pat, 2), Some(0xDEAD_BEEF));

    let pat2 = &[Some(0xDDu8)][..];
    sigscan::set_pattern_u32("m.dll", pat2, 2, None);
    assert!(sigscan::find_pattern_u32("m.dll", pat2, 2).is_none());
}

#[test]
fn sigscan_find_pattern_u32_reads_unaligned_from_match_site() {
    let _g = setup();
    let addr = imm32_at(3, 0xCAFE_BABE);
    let pat = &[Some(0xEEu8)][..];
    sigscan::set_pattern("m.dll", pat, Some(addr));
    assert_eq!(sigscan::find_pattern_u32("m.dll", pat, 3), Some(0xCAFE_BABE));
}

#[test]
fn sigscan_find_pattern_u32_propagates_none_when_pattern_misses() {
    let _g = setup();
    let pat = &[Some(0xEFu8)][..];
    sigscan::set_pattern("m.dll", pat, None);
    assert!(sigscan::find_pattern_u32("m.dll", pat, 3).is_none());
}

#[test]
fn sigscan_find_pattern_u8_via_override_some_and_none() {
    let _g = setup();
    let pat = &[Some(0xC0u8)][..];
    sigscan::set_pattern_u8("m.dll", pat, 1, Some(0x42));
    assert_eq!(sigscan::find_pattern_u8("m.dll", pat, 1), Some(0x42));

    let pat2 = &[Some(0xC1u8)][..];
    sigscan::set_pattern_u8("m.dll", pat2, 1, None);
    assert!(sigscan::find_pattern_u8("m.dll", pat2, 1).is_none());
}

#[test]
fn sigscan_find_pattern_u8_reads_byte_from_match_site() {
    let _g = setup();
    let addr = imm8_at(2, 0xAB);
    let pat = &[Some(0xC2u8)][..];
    sigscan::set_pattern("m.dll", pat, Some(addr));
    assert_eq!(sigscan::find_pattern_u8("m.dll", pat, 2), Some(0xAB));
}

#[test]
fn sigscan_find_pattern_u8_propagates_none_when_pattern_misses() {
    let _g = setup();
    let pat = &[Some(0xC3u8)][..];
    sigscan::set_pattern("m.dll", pat, None);
    assert!(sigscan::find_pattern_u8("m.dll", pat, 1).is_none());
}

#[test]
fn runtime_buttons_get_hit_and_miss() {
    let mut b = RuntimeButtons::default();
    b.map.insert(String::from("in_attack"), 0x1234);
    assert_eq!(b.get("in_attack"), Some(0x1234));
    assert_eq!(b.get("absent"), None);
}

#[test]
fn runtime_interfaces_get_three_branches() {
    let mut outer = hashbrown::HashMap::new();
    let mut inner = hashbrown::HashMap::new();
    inner.insert(String::from("Source2Client002"), 0xAA);
    outer.insert(String::from("client.dll"), inner);
    let r = RuntimeInterfaces { map: outer };
    assert_eq!(r.get("client.dll", "Source2Client002"), Some(0xAA));
    assert_eq!(r.get("client.dll", "Missing"), None);
    assert_eq!(r.get("nope.dll", "Source2Client002"), None);
}

#[test]
fn discover_globals_all_none_when_no_matches() {
    let _g = setup();
    let r = offsets::discover_globals();
    assert!(r.dw_csgo_input.is_none());
    assert!(r.dw_view_angles.is_none());
    assert!(r.dw_entity_list.is_none());
    assert!(r.dw_view_matrix.is_none());
    assert!(r.dw_build_number.is_none());
    assert!(r.dw_input_system.is_none());
    assert!(r.dw_game_types.is_none());
    assert!(r.dw_sound_system.is_none());
    // New u32/u8-immediate (struct-field offset) fields.
    assert!(r.dw_game_entity_system_highest_entity_index.is_none());
    assert!(r.dw_local_player_pawn.is_none());
    assert!(r.dw_sensitivity_sensitivity.is_none());
    assert!(r.dw_network_game_client_client_tick_count.is_none());
    assert!(r.dw_network_game_client_delta_tick.is_none());
    assert!(r.dw_network_game_client_is_background_map.is_none());
    assert!(r.dw_network_game_client_local_player.is_none());
    assert!(r.dw_network_game_client_max_clients.is_none());
    assert!(r.dw_network_game_client_server_tick_count.is_none());
    assert!(r.dw_network_game_client_sign_on_state.is_none());
    assert!(r.dw_sound_system_engine_view_data.is_none());
}

#[test]
fn discover_globals_resolves_one_known_pattern() {
    let _g = setup();
    let pat: &[Option<u8>] =
        &[Some(0x48), Some(0x89), Some(0x05), None, None, None, None, Some(0x33), Some(0xC0)];
    sigscan::set_pattern_rip32("inputsystem.dll", pat, 3, Some(0xFEED_FACE_usize));
    let r = offsets::discover_globals();
    assert_eq!(r.dw_input_system, Some(0xFEED_FACE_usize));
}

#[test]
fn discover_globals_resolves_view_angles_as_csgo_input_plus_imm() {
    let _g = setup();
    let csgo_pat: &[Option<u8>] = &[
        Some(0x48),
        Some(0x89),
        Some(0x05),
        None,
        None,
        None,
        None,
        Some(0x0F),
        Some(0x57),
        Some(0xC0),
        Some(0x0F),
        Some(0x11),
        Some(0x05),
    ];
    let secondary: &[Option<u8>] = &[
        Some(0xF2),
        Some(0x42),
        Some(0x0F),
        Some(0x10),
        Some(0x84),
        Some(0x28),
        None,
        None,
        None,
        None,
    ];
    sigscan::set_pattern_rip32("client.dll", csgo_pat, 3, Some(0x1000));
    sigscan::set_pattern_u32("client.dll", secondary, 6, Some(0x40));
    let r = offsets::discover_globals();
    assert_eq!(r.dw_view_angles, Some(0x1040));
}

#[test]
fn discover_globals_view_angles_none_when_csgo_input_missing() {
    let _g = setup();
    let secondary: &[Option<u8>] = &[
        Some(0xF2),
        Some(0x42),
        Some(0x0F),
        Some(0x10),
        Some(0x84),
        Some(0x28),
        None,
        None,
        None,
        None,
    ];
    sigscan::set_pattern_u32("client.dll", secondary, 6, Some(0x40));
    let r = offsets::discover_globals();
    assert!(r.dw_view_angles.is_none());
}

#[test]
fn discover_globals_view_angles_none_when_secondary_imm_missing() {
    let _g = setup();
    let csgo_pat: &[Option<u8>] = &[
        Some(0x48),
        Some(0x89),
        Some(0x05),
        None,
        None,
        None,
        None,
        Some(0x0F),
        Some(0x57),
        Some(0xC0),
        Some(0x0F),
        Some(0x11),
        Some(0x05),
    ];
    sigscan::set_pattern_rip32("client.dll", csgo_pat, 3, Some(0x1000));
    let r = offsets::discover_globals();
    assert!(r.dw_view_angles.is_none());
}

#[test]
fn discover_globals_resolves_local_player_pawn_as_prediction_plus_imm() {
    let _g = setup();
    let pred_pat: &[Option<u8>] = &[
        Some(0x48),
        Some(0x8D),
        Some(0x05),
        None,
        None,
        None,
        None,
        Some(0xC3),
        Some(0xCC),
        Some(0xCC),
        Some(0xCC),
        Some(0xCC),
        Some(0xCC),
        Some(0xCC),
        Some(0xCC),
        Some(0xCC),
        Some(0x40),
        Some(0x53),
        Some(0x56),
        Some(0x41),
        Some(0x54),
    ];
    let secondary: &[Option<u8>] = &[
        Some(0x4C),
        Some(0x39),
        Some(0xB6),
        None,
        None,
        None,
        None,
        Some(0x74),
        None,
        Some(0x44),
        Some(0x88),
        Some(0xBE),
    ];
    sigscan::set_pattern_rip32("client.dll", pred_pat, 3, Some(0x2000));
    sigscan::set_pattern_u32("client.dll", secondary, 3, Some(0x80));
    let r = offsets::discover_globals();
    assert_eq!(r.dw_local_player_pawn, Some(0x2080));
}

#[test]
fn discover_globals_local_player_pawn_none_when_prediction_missing() {
    let _g = setup();
    let secondary: &[Option<u8>] = &[
        Some(0x4C),
        Some(0x39),
        Some(0xB6),
        None,
        None,
        None,
        None,
        Some(0x74),
        None,
        Some(0x44),
        Some(0x88),
        Some(0xBE),
    ];
    sigscan::set_pattern_u32("client.dll", secondary, 3, Some(0x80));
    let r = offsets::discover_globals();
    assert!(r.dw_local_player_pawn.is_none());
}

#[test]
fn discover_globals_resolves_u32_imm_highest_entity_index() {
    let _g = setup();
    let pat: &[Option<u8>] =
        &[Some(0xFF), Some(0x81), None, None, None, None, Some(0x48), Some(0x85), Some(0xD2)];
    sigscan::set_pattern_u32("client.dll", pat, 2, Some(0x2090));
    let r = offsets::discover_globals();
    assert_eq!(r.dw_game_entity_system_highest_entity_index, Some(0x2090));
}

#[test]
fn discover_globals_resolves_u32_imm_network_game_client_sign_on_state() {
    let _g = setup();
    let pat: &[Option<u8>] = &[
        Some(0x44),
        Some(0x8B),
        Some(0x81),
        None,
        None,
        None,
        None,
        Some(0x48),
        Some(0x8D),
        Some(0x0D),
    ];
    sigscan::set_pattern_u32("engine2.dll", pat, 3, Some(0x230));
    let r = offsets::discover_globals();
    assert_eq!(r.dw_network_game_client_sign_on_state, Some(0x230));
}

#[test]
fn discover_globals_resolves_all_remaining_struct_offset_fields() {
    let _g = setup();
    // Each `.map(|v| v as usize)` widening closure runs only when its
    // pattern resolves to Some; cover every remaining one in one shot.
    sigscan::set_pattern_u8(
        "client.dll",
        &[
            Some(0x48),
            Some(0x8D),
            Some(0x7E),
            None,
            Some(0x48),
            Some(0x0F),
            Some(0xBA),
            Some(0xE0),
            None,
            Some(0x72),
            None,
            Some(0x85),
            Some(0xD2),
            Some(0x49),
            Some(0x0F),
            Some(0x4F),
            Some(0xFF),
        ],
        3,
        Some(0x10),
    );
    sigscan::set_pattern_u32(
        "engine2.dll",
        &[
            Some(0x8B),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0x8B),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0x83),
            Some(0xB9),
        ],
        2,
        Some(0x100),
    );
    sigscan::set_pattern_u32(
        "engine2.dll",
        &[
            Some(0x4C),
            Some(0x8D),
            Some(0xB7),
            None,
            None,
            None,
            None,
            Some(0x4C),
            Some(0x89),
            Some(0x7C),
            Some(0x24),
        ],
        3,
        Some(0x110),
    );
    sigscan::set_pattern_u32(
        "engine2.dll",
        &[
            Some(0x0F),
            Some(0xB6),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0x0F),
            Some(0xB6),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0x40),
            Some(0x53),
        ],
        3,
        Some(0x2C141F),
    );
    sigscan::set_pattern_u32(
        "engine2.dll",
        &[
            Some(0x42),
            Some(0x8B),
            Some(0x94),
            Some(0xD3),
            None,
            None,
            None,
            None,
            Some(0x5B),
            Some(0x49),
            Some(0xFF),
            Some(0xE3),
            Some(0x32),
            Some(0xC0),
            Some(0x5B),
            Some(0xC3),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0x40),
            Some(0x53),
        ],
        4,
        Some(0xF8),
    );
    sigscan::set_pattern_u32(
        "engine2.dll",
        &[
            Some(0x8B),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(0x8B),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(0x8B),
            Some(0x81),
        ],
        2,
        Some(0x240),
    );
    // dw_network_game_client_server_tick_count uses a distinct shorter pattern.
    sigscan::set_pattern_u32(
        "engine2.dll",
        &[
            Some(0x8B),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0xC3),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0xCC),
            Some(0x83),
            Some(0xB9),
        ],
        2,
        Some(0x108),
    );
    let r = offsets::discover_globals();
    assert_eq!(r.dw_sensitivity_sensitivity, Some(0x10));
    assert_eq!(r.dw_network_game_client_client_tick_count, Some(0x100));
    assert_eq!(r.dw_network_game_client_server_tick_count, Some(0x108));
    assert_eq!(r.dw_network_game_client_delta_tick, Some(0x110));
    assert_eq!(r.dw_network_game_client_is_background_map, Some(0x2C141F));
    assert_eq!(r.dw_network_game_client_local_player, Some(0xF8));
    assert_eq!(r.dw_network_game_client_max_clients, Some(0x240));
}

#[test]
fn discover_globals_resolves_u8_imm_sound_system_engine_view_data() {
    let _g = setup();
    let pat: &[Option<u8>] = &[
        Some(0x0F),
        Some(0x11),
        Some(0x47),
        None,
        Some(0x0F),
        Some(0x10),
        Some(0x4E),
        None,
        Some(0x0F),
        Some(0x11),
        Some(0x8F),
    ];
    sigscan::set_pattern_u8("soundsystem.dll", pat, 3, Some(0x60));
    let r = offsets::discover_globals();
    assert_eq!(r.dw_sound_system_engine_view_data, Some(0x60));
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

#[test]
fn discover_buttons_no_pattern_match_returns_empty() {
    let _g = setup();
    let r = buttons::discover_buttons();
    assert!(r.map.is_empty());
}

#[test]
fn discover_buttons_zero_head_returns_empty() {
    let _g = setup();
    let cell = 0x10_0000usize;
    sigscan::set_pattern_rip32("client.dll", BTN_PATTERN, 3, Some(cell));
    populate(|s| s.write_usize(cell, 0));
    let r = buttons::discover_buttons();
    assert!(r.map.is_empty());
}

#[test]
fn discover_buttons_walks_two_node_chain() {
    let _g = setup();
    let cell = 0x20_0000usize;
    let node1 = 0x30_0000usize;
    let node2 = 0x30_1000usize;
    let name1 = 0x40_0000usize;
    let name2 = 0x40_0100usize;

    sigscan::set_pattern_rip32("client.dll", BTN_PATTERN, 3, Some(cell));
    populate(|s| {
        s.write_usize(cell, node1);
        s.write_usize(node1 + 0x08, name1);
        s.write_cstr(name1, "in_attack");
        s.write_usize(node1 + 0x88, node2);
        s.write_usize(node2 + 0x08, name2);
        s.write_cstr(name2, "in_jump");
        s.write_usize(node2 + 0x88, 0);
    });

    let r = buttons::discover_buttons();
    assert_eq!(r.get("in_attack"), Some(node1 + 0x30));
    assert_eq!(r.get("in_jump"), Some(node2 + 0x30));
}

#[test]
fn discover_buttons_stops_on_null_name_ptr() {
    let _g = setup();
    let cell = 0x21_0000usize;
    let node = 0x32_0000usize;
    sigscan::set_pattern_rip32("client.dll", BTN_PATTERN, 3, Some(cell));
    populate(|s| {
        s.write_usize(cell, node);
        s.write_usize(node + 0x08, 0);
    });
    let r = buttons::discover_buttons();
    assert!(r.map.is_empty());
}

#[test]
fn discover_buttons_stops_on_unreadable_name_field() {
    let _g = setup();
    let cell = 0x22_0000usize;
    let node = 0x33_0000usize;
    sigscan::set_pattern_rip32("client.dll", BTN_PATTERN, 3, Some(cell));
    populate(|s| {
        s.write_usize(cell, node);
        // The name_ptr field itself is denied → read_usize_off returns None.
        s.deny_read(node + 0x08);
    });
    let r = buttons::discover_buttons();
    assert!(r.map.is_empty());
}

#[test]
fn discover_buttons_stops_on_unreadable_name_string() {
    let _g = setup();
    let cell = 0x23_0000usize;
    let node = 0x34_0000usize;
    let name_addr = 0x44_0000usize;
    sigscan::set_pattern_rip32("client.dll", BTN_PATTERN, 3, Some(cell));
    populate(|s| {
        s.write_usize(cell, node);
        s.write_usize(node + 0x08, name_addr);
        s.deny_read(name_addr);
    });
    let r = buttons::discover_buttons();
    assert!(r.map.is_empty());
}

#[test]
fn discover_interfaces_returns_empty_without_exports() {
    let _g = setup();
    let r = interfaces::discover_interfaces();
    assert!(r.map.is_empty());
}

#[test]
fn discover_interfaces_in_skips_modules_without_create_interface() {
    let _g = setup();
    let r = interfaces::discover_interfaces_in(&["nope.dll"]);
    assert!(r.map.is_empty());
}

#[test]
fn discover_interfaces_in_skips_module_with_zero_head() {
    let _g = setup();
    let (inst, cell) = rel32_instr(0x100);
    populate(|s| {
        s.add_export("client.dll", "CreateInterface", inst);
        s.write_usize(cell, 0);
    });
    let r = interfaces::discover_interfaces_in(&["client.dll"]);
    assert!(r.map.is_empty());
}

#[test]
fn discover_interfaces_in_walks_chain_with_two_entries() {
    let _g = setup();
    let (ci_inst, cell) = rel32_instr(0x200);
    let (create1_inst, inst1) = rel32_instr(0x400);
    let (create2_inst, inst2) = rel32_instr(0x800);

    let reg1 = 0x81_0000usize;
    let reg2 = 0x82_0000usize;
    let name1 = 0x83_0000usize;
    let name2 = 0x84_0000usize;

    populate(|s| {
        s.add_export("server.dll", "CreateInterface", ci_inst);
        s.write_usize(cell, reg1);

        s.write_usize(reg1 + 0x00, create1_inst);
        s.write_usize(reg1 + 0x08, name1);
        s.write_usize(reg1 + 0x10, reg2);
        s.write_cstr(name1, "Source2Server001");

        s.write_usize(reg2 + 0x00, create2_inst);
        s.write_usize(reg2 + 0x08, name2);
        s.write_usize(reg2 + 0x10, 0);
        s.write_cstr(name2, "Source2Server002");
    });

    let r = interfaces::discover_interfaces_in(&["server.dll"]);
    assert_eq!(r.get("server.dll", "Source2Server001"), Some(inst1));
    assert_eq!(r.get("server.dll", "Source2Server002"), Some(inst2));
}

#[test]
fn discover_interfaces_in_skips_entries_with_zero_create_or_name() {
    let _g = setup();
    let (ci_inst, cell) = rel32_instr(0x200);
    let (create2_inst, inst2) = rel32_instr(0x800);

    let reg1 = 0xA1_0000usize;
    let reg2 = 0xA2_0000usize;
    let name2 = 0xA3_0000usize;

    populate(|s| {
        s.add_export("foo.dll", "CreateInterface", ci_inst);
        s.write_usize(cell, reg1);
        // reg1: create_fn=0, name=0 → skipped; chain continues to reg2
        s.write_usize(reg1 + 0x10, reg2);

        s.write_usize(reg2 + 0x00, create2_inst);
        s.write_usize(reg2 + 0x08, name2);
        s.write_usize(reg2 + 0x10, 0);
        s.write_cstr(name2, "Real");
    });

    let r = interfaces::discover_interfaces_in(&["foo.dll"]);
    assert_eq!(r.get("foo.dll", "Real"), Some(inst2));
}

#[test]
fn discover_interfaces_in_stops_when_node_unreadable() {
    let _g = setup();
    let (ci_inst, cell) = rel32_instr(0x200);
    let bad_node = 0xCC_CC00usize;
    populate(|s| {
        s.add_export("bar.dll", "CreateInterface", ci_inst);
        s.write_usize(cell, bad_node);
        s.deny_read(bad_node);
    });
    let r = interfaces::discover_interfaces_in(&["bar.dll"]);
    assert!(r.map.is_empty());
}

struct SchemaLayout {
    ss_base: usize,
    fields_base: usize,
}

fn populate_schema_layout() -> SchemaLayout {
    let ss_base = 0x10_00_0000usize;
    let scope_array = 0x10_10_0000usize;
    let type_scope = 0x10_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let node = 0x10_40_0000usize;
    let binding = 0x10_50_0000usize;
    let class_name = 0x10_60_0000usize;
    let fields_base = 0x10_70_0000usize;
    let field_name = 0x10_80_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);

        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 1);
        s.write_u32(class_bindings + 0x10, 0);
        s.write_usize(class_bindings + 0x20, 0);

        s.write_usize(class_bindings + 0x60 + 0x10, node);

        s.write_usize(node + 0x08, 0);
        s.write_usize(node + 0x10, binding);

        s.write_usize(binding + 0x08, class_name);
        s.write_i16(binding + 0x24, 1);
        s.write_usize(binding + 0x30, fields_base);
        s.write_cstr(class_name, "C_BaseEntity");

        s.write_usize(fields_base + 0x00, field_name);
        s.write_u32(fields_base + 0x10, 0x42);
        s.write_cstr(field_name, "m_iHealth");
    });

    walker::_test_set_schema_system(ss_base);
    SchemaLayout { ss_base, fields_base }
}

#[test]
fn walker_happy_path_resolves_field_offset() {
    let _g = setup();
    populate_schema_layout();
    assert_eq!(walker::lookup_offset("client.dll", "C_BaseEntity", "m_iHealth"), Some(0x42));
}

#[test]
fn walker_cache_hits_on_second_call() {
    let _g = setup();
    populate_schema_layout();
    let a = walker::lookup_offset("client.dll", "C_BaseEntity", "m_iHealth");
    // Wipe layout - if cache hit, we still get the same answer.
    populate(|s| s.reset());
    let b = walker::lookup_offset("client.dll", "C_BaseEntity", "m_iHealth");
    assert_eq!(a, b);
}

#[test]
fn walker_class_not_found_returns_none() {
    let _g = setup();
    populate_schema_layout();
    assert!(walker::lookup_offset("client.dll", "C_DoesNotExist", "anything").is_none());
}

#[test]
fn walker_field_not_found_returns_none() {
    let _g = setup();
    populate_schema_layout();
    assert!(walker::lookup_offset("client.dll", "C_BaseEntity", "missing_field").is_none());
}

#[test]
fn walker_module_not_found_returns_none() {
    let _g = setup();
    populate_schema_layout();
    assert!(walker::lookup_offset("missing.dll", "C_BaseEntity", "m_iHealth").is_none());
}

#[test]
fn walker_schema_system_pattern_miss_caches_zero() {
    let _g = setup();
    // No pattern override → schema_system() resolves to 0, RESOLVED becomes true.
    assert!(walker::lookup_offset("client.dll", "X", "y").is_none());
    // Second call hits the RESOLVED branch.
    assert!(walker::lookup_offset("client.dll", "X", "y2").is_none());
}

#[test]
fn walker_schema_system_via_pattern_override() {
    let _g = setup();
    let ts_pattern: &[Option<u8>] = &[
        Some(0x4C),
        Some(0x8D),
        Some(0x35),
        None,
        None,
        None,
        None,
        Some(0x0F),
        Some(0x28),
        Some(0x45),
    ];
    let target = 0x77_00_0000usize;
    sigscan::set_pattern_rip32("schemasystem.dll", ts_pattern, 3, Some(target));
    // Empty type_scopes vec → lookup fails cleanly.
    populate(|s| {
        s.write_u32(target + 0x190, 0);
        s.write_usize(target + 0x190 + 8, 0);
    });
    assert!(walker::lookup_offset("client.dll", "Foo", "bar").is_none());
}

#[test]
fn walker_skips_scope_with_zero_ptr() {
    let _g = setup();
    let ss_base = 0x88_00_0000usize;
    let scope_array = 0x88_10_0000usize;
    populate(|s| {
        s.write_u32(ss_base + 0x190, 2);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, 0);
        s.write_usize(scope_array + 8, 0);
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "X", "y").is_none());
}

#[test]
fn walker_skips_scope_with_mismatched_name() {
    let _g = setup();
    let ss_base = 0x99_00_0000usize;
    let scope_array = 0x99_10_0000usize;
    let scope = 0x99_20_0000usize;
    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, scope);
        s.write_cstr(scope + 0x08, "engine2.dll");
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "X", "y").is_none());
}

#[test]
fn walker_walks_free_blob_chain() {
    let _g = setup();
    let ss_base = 0xAA_00_0000usize;
    let scope_array = 0xAA_10_0000usize;
    let type_scope = 0xAA_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let blob = 0xAA_50_0000usize;
    let binding = 0xAA_60_0000usize;
    let class_name = 0xAA_70_0000usize;
    let fields = 0xAA_80_0000usize;
    let field_name = 0xAA_90_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        // blocks_allocated = 0 → allocated walk yields nothing
        s.write_u32(class_bindings + 0x0C, 0);
        s.write_u32(class_bindings + 0x10, 1);
        s.write_usize(class_bindings + 0x20, blob);

        s.write_usize(blob + 0x00, 0);
        s.write_usize(blob + 0x10, binding);

        s.write_usize(binding + 0x08, class_name);
        s.write_i16(binding + 0x24, 1);
        s.write_usize(binding + 0x30, fields);
        s.write_cstr(class_name, "FromFreeChain");

        s.write_usize(fields + 0x00, field_name);
        s.write_u32(fields + 0x10, 0x84);
        s.write_cstr(field_name, "m_x");
    });

    walker::_test_set_schema_system(ss_base);
    assert_eq!(walker::lookup_offset("client.dll", "FromFreeChain", "m_x"), Some(0x84));
}

#[test]
fn walker_visit_binding_skips_null_name_ptr() {
    let _g = setup();
    let ss_base = 0xBB_00_0000usize;
    let scope_array = 0xBB_10_0000usize;
    let type_scope = 0xBB_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let node = 0xBB_30_0000usize;
    let binding = 0xBB_40_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 1);
        s.write_u32(class_bindings + 0x10, 0);
        s.write_usize(class_bindings + 0x20, 0);

        s.write_usize(class_bindings + 0x60 + 0x10, node);
        s.write_usize(node + 0x08, 0);
        s.write_usize(node + 0x10, binding);

        // binding's name_ptr = 0 → visit_binding returns true (skip)
        s.write_usize(binding + 0x08, 0);
    });

    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "C", "f").is_none());
}

#[test]
fn walker_visit_binding_skips_unreadable_name() {
    let _g = setup();
    let ss_base = 0xCC_00_0000usize;
    let scope_array = 0xCC_10_0000usize;
    let type_scope = 0xCC_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let node = 0xCC_30_0000usize;
    let binding = 0xCC_40_0000usize;
    let bad_name = 0xCC_F0_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 1);
        s.write_u32(class_bindings + 0x10, 0);
        s.write_usize(class_bindings + 0x20, 0);

        s.write_usize(class_bindings + 0x60 + 0x10, node);
        s.write_usize(node + 0x08, 0);
        s.write_usize(node + 0x10, binding);

        s.write_usize(binding + 0x08, bad_name);
        s.deny_read(bad_name);
    });

    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "C", "f").is_none());
}

#[test]
fn walker_field_with_null_name_skipped() {
    let _g = setup();
    let SchemaLayout { fields_base, .. } = populate_schema_layout();
    populate(|s| s.write_usize(fields_base + 0x00, 0));
    // Use a fresh field name so the cache doesn't short-circuit.
    assert!(walker::lookup_offset("client.dll", "C_BaseEntity", "no_such_field").is_none());
}

#[test]
fn walker_skips_scope_with_unreadable_name() {
    let _g = setup();
    let ss_base = 0xDD_00_0000usize;
    let scope_array = 0xDD_10_0000usize;
    let scope = 0xDD_20_0000usize;
    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, scope);
        s.deny_read(scope + 0x08);
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "X", "y").is_none());
}

#[test]
fn lookup_or_fallback_returns_runtime_when_present() {
    let _g = setup();
    populate_schema_layout();
    assert_eq!(lookup_or_fallback("client.dll", "C_BaseEntity", "m_iHealth", 0xDEAD), 0x42);
}

#[test]
fn walker_allocated_walk_advances_to_next_node() {
    let _g = setup();
    let ss_base = 0x12_00_0000usize;
    let scope_array = 0x12_10_0000usize;
    let type_scope = 0x12_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let node1 = 0x12_30_0000usize;
    let node2 = 0x12_31_0000usize;
    let binding1 = 0x12_40_0000usize;
    let binding2 = 0x12_41_0000usize;
    let name1 = 0x12_50_0000usize;
    let name2 = 0x12_51_0000usize;
    let fields2 = 0x12_60_0000usize;
    let fn_name = 0x12_70_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        // two allocated bindings, so cap=2 and the chain advances via FIXED_NEXT
        s.write_u32(class_bindings + 0x0C, 2);
        s.write_u32(class_bindings + 0x10, 0);
        s.write_usize(class_bindings + 0x20, 0);

        // bucket 0 head -> node1 -> node2 -> 0
        s.write_usize(class_bindings + 0x60 + 0x10, node1);

        s.write_usize(node1 + 0x08, node2); // next
        s.write_usize(node1 + 0x10, binding1);

        s.write_usize(node2 + 0x08, 0);
        s.write_usize(node2 + 0x10, binding2);

        s.write_usize(binding1 + 0x08, name1);
        s.write_cstr(name1, "C_First");
        // binding1 has no fields - we won't search for them.

        s.write_usize(binding2 + 0x08, name2);
        s.write_cstr(name2, "C_Second");
        s.write_i16(binding2 + 0x24, 1);
        s.write_usize(binding2 + 0x30, fields2);
        s.write_usize(fields2 + 0x00, fn_name);
        s.write_u32(fields2 + 0x10, 0x77);
        s.write_cstr(fn_name, "m_target");
    });
    walker::_test_set_schema_system(ss_base);
    assert_eq!(walker::lookup_offset("client.dll", "C_Second", "m_target"), Some(0x77));
}

#[test]
fn walker_allocated_walk_breaks_when_data_read_fails() {
    let _g = setup();
    let ss_base = 0x13_00_0000usize;
    let scope_array = 0x13_10_0000usize;
    let type_scope = 0x13_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let node = 0x13_30_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 1);
        s.write_u32(class_bindings + 0x10, 0);
        s.write_usize(class_bindings + 0x20, 0);

        s.write_usize(class_bindings + 0x60 + 0x10, node);
        // node's FIXED_DATA field (+0x10) is denied → inner if-let returns None → break
        s.deny_read(node + 0x10);
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "C", "f").is_none());
}

#[test]
fn walker_free_chain_breaks_when_data_read_fails() {
    let _g = setup();
    let ss_base = 0x14_00_0000usize;
    let scope_array = 0x14_10_0000usize;
    let type_scope = 0x14_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let blob = 0x14_50_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        // No allocated entries; free chain has one blob, but its data is denied.
        s.write_u32(class_bindings + 0x0C, 0);
        s.write_u32(class_bindings + 0x10, 1);
        s.write_usize(class_bindings + 0x20, blob);
        s.deny_read(blob + 0x10);
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "C", "f").is_none());
}

#[test]
fn walker_free_chain_capacity_break() {
    let _g = setup();
    let ss_base = 0x15_00_0000usize;
    let scope_array = 0x15_10_0000usize;
    let type_scope = 0x15_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let blob = 0x15_50_0000usize;
    let binding = 0x15_60_0000usize;
    let name = 0x15_70_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 0);
        s.write_u32(class_bindings + 0x10, 1); // peak = 1
        s.write_usize(class_bindings + 0x20, blob);

        s.write_usize(blob + 0x00, 0);
        s.write_usize(blob + 0x10, binding);

        // binding doesn't match → visit returns true → seen2 reaches cap2 → break
        s.write_usize(binding + 0x08, name);
        s.write_cstr(name, "Unrelated");
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "C", "f").is_none());
}

#[test]
fn walker_field_with_unreadable_name_skipped() {
    let _g = setup();
    let SchemaLayout { fields_base, .. } = populate_schema_layout();
    let bad = 0xEE_F0_0000usize;
    populate(|s| {
        s.write_usize(fields_base + 0x00, bad);
        s.deny_read(bad);
    });
    assert!(walker::lookup_offset("client.dll", "C_BaseEntity", "other").is_none());
}

#[test]
fn walker_visit_binding_unreadable_name_field() {
    let _g = setup();
    let ss_base = 0x16_00_0000usize;
    let scope_array = 0x16_10_0000usize;
    let type_scope = 0x16_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let node = 0x16_30_0000usize;
    let binding = 0x16_40_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 1);
        s.write_u32(class_bindings + 0x10, 0);
        s.write_usize(class_bindings + 0x20, 0);

        s.write_usize(class_bindings + 0x60 + 0x10, node);
        s.write_usize(node + 0x08, 0);
        s.write_usize(node + 0x10, binding);

        // The binding's CLASS_NAME field at +0x08 is denied → visit_binding's
        // read_usize_off returns None and the function returns true (skip).
        s.deny_read(binding + 0x08);
    });
    walker::_test_set_schema_system(ss_base);
    assert!(walker::lookup_offset("client.dll", "C", "f").is_none());
}

#[test]
fn walker_free_chain_advances_through_multiple_blobs() {
    let _g = setup();
    let ss_base = 0x17_00_0000usize;
    let scope_array = 0x17_10_0000usize;
    let type_scope = 0x17_20_0000usize;
    let class_bindings = type_scope + 0x560;
    let blob1 = 0x17_50_0000usize;
    let blob2 = 0x17_51_0000usize;
    let binding1 = 0x17_60_0000usize;
    let binding2 = 0x17_61_0000usize;
    let name1 = 0x17_70_0000usize;
    let name2 = 0x17_71_0000usize;
    let fields2 = 0x17_80_0000usize;
    let fn_name = 0x17_90_0000usize;

    populate(|s| {
        s.write_u32(ss_base + 0x190, 1);
        s.write_usize(ss_base + 0x190 + 8, scope_array);
        s.write_usize(scope_array, type_scope);
        s.write_cstr(type_scope + 0x08, "client.dll");

        s.write_u32(class_bindings + 0x0C, 0); // skip allocated walk
        s.write_u32(class_bindings + 0x10, 2); // peak = 2
        s.write_usize(class_bindings + 0x20, blob1);

        // blob1 -> blob2 -> 0
        s.write_usize(blob1 + 0x00, blob2);
        s.write_usize(blob1 + 0x10, binding1);

        s.write_usize(blob2 + 0x00, 0);
        s.write_usize(blob2 + 0x10, binding2);

        // binding1 doesn't match → fall through, advance to blob2
        s.write_usize(binding1 + 0x08, name1);
        s.write_cstr(name1, "Unrelated");

        // binding2 matches the search
        s.write_usize(binding2 + 0x08, name2);
        s.write_cstr(name2, "Wanted");
        s.write_i16(binding2 + 0x24, 1);
        s.write_usize(binding2 + 0x30, fields2);
        s.write_usize(fields2 + 0x00, fn_name);
        s.write_u32(fields2 + 0x10, 0x99);
        s.write_cstr(fn_name, "m_w");
    });
    walker::_test_set_schema_system(ss_base);
    assert_eq!(walker::lookup_offset("client.dll", "Wanted", "m_w"), Some(0x99));
}

#[test]
fn discover_interfaces_in_skips_entry_with_unreadable_name() {
    let _g = setup();
    let (ci_inst, cell) = rel32_instr(0x200);
    let (create_inst, _) = rel32_instr(0x400);
    let reg = 0xB1_0000usize;
    let bad_name = 0xB2_0000usize;

    populate(|s| {
        s.add_export("baz.dll", "CreateInterface", ci_inst);
        s.write_usize(cell, reg);
        s.write_usize(reg + 0x00, create_inst);
        s.write_usize(reg + 0x08, bad_name);
        s.write_usize(reg + 0x10, 0);
        s.deny_read(bad_name);
    });
    let r = interfaces::discover_interfaces_in(&["baz.dll"]);
    assert!(r.map.is_empty());
}
