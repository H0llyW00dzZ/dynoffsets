//! Runtime schema field offset walker.
//!
//! Finds the SchemaSystem singleton via signature, then walks the type
//! scopes and class bindings to resolve field offsets for the `#[schema]`
//! macro.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use hashbrown::HashMap;
use obfstr::obfstr;

use crate::mem;
use crate::sigscan;
use crate::sync::{Mutex, OnceCell};

const SCHEMA_SYSTEM_PATTERN: &[Option<u8>] = &[
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
const SCHEMA_SYSTEM_DISP: usize = 3;

const MAX_NAME: usize = 256;
const MAX_CHAIN: usize = 4096;

const SS_TYPE_SCOPES: usize = 0x190;
const VEC_COUNT: usize = 0;
const VEC_DATA: usize = 8;
const VEC_STRIDE: usize = 8;
const TS_NAME: usize = 8;
const TS_NAME_LEN: usize = 256;
const TS_CLASS_BINDINGS: usize = 0x560;

const HASH_ENTRY_MEM: usize = 0;
const HASH_BUCKETS: usize = 0x60;
const HASH_BUCKET_STRIDE: usize = 0x18;
const HASH_BUCKET_COUNT: usize = 256;

const MEMPOOL_ALLOC: usize = 0x0C;
const MEMPOOL_PEAK: usize = 0x10;
const MEMPOOL_FREE_HEAD: usize = 0x20;

const BUCKET_FIRST_UNC: usize = 0x10;
const FIXED_NEXT: usize = 8;
const FIXED_DATA: usize = 0x10;
const BLOB_NEXT: usize = 0;
const BLOB_DATA: usize = 0x10;

const CLASS_NAME: usize = 8;
const CLASS_FIELD_COUNT: usize = 0x24;
const CLASS_FIELDS: usize = 0x30;
const CLASS_BASE_CLASSES: usize = 0x40;
const BASE_INFO_CLASS: usize = 0x18;
const BASE_CLASS_NAME: usize = 0x10;
const MAX_BASE_DEPTH: usize = 16;

const FIELD_NAME: usize = 0;
const FIELD_OFF: usize = 0x10;
const FIELD_STRIDE: usize = 0x20;

static SCHEMA_SYSTEM: AtomicUsize = AtomicUsize::new(0);
static RESOLVED: AtomicBool = AtomicBool::new(false);

type Key = (u32, u32, u32);
type ClassKey = (u32, u32);

fn cache() -> &'static Mutex<HashMap<Key, u32>> {
    static C: OnceCell<Mutex<HashMap<Key, u32>>> = OnceCell::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn class_cache() -> &'static Mutex<HashMap<ClassKey, usize>> {
    static C: OnceCell<Mutex<HashMap<ClassKey, usize>>> = OnceCell::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
pub(crate) fn _test_reset_schema_system() {
    SCHEMA_SYSTEM.store(0, Ordering::Release);
    RESOLVED.store(false, Ordering::Release);
    cache().lock().clear();
    class_cache().lock().clear();
}

#[cfg(test)]
pub(crate) fn _test_set_schema_system(addr: usize) {
    SCHEMA_SYSTEM.store(addr, Ordering::Release);
    RESOLVED.store(true, Ordering::Release);
}

fn schema_system() -> Option<usize> {
    if RESOLVED.load(Ordering::Acquire) {
        let p = SCHEMA_SYSTEM.load(Ordering::Acquire);
        return (p != 0).then_some(p);
    }
    let v = sigscan::find_pattern_rip32(
        obfstr!("schemasystem.dll"),
        SCHEMA_SYSTEM_PATTERN,
        SCHEMA_SYSTEM_DISP,
    )
    .unwrap_or_default();
    SCHEMA_SYSTEM.store(v, Ordering::Release);
    RESOLVED.store(true, Ordering::Release);
    (v != 0).then_some(v)
}

pub fn lookup_offset(module: &str, class: &str, field: &str) -> Option<u32> {
    let mh = crate::fnv1a(module);
    let ch = crate::fnv1a(class);
    let fh = crate::fnv1a(field);
    lookup_offset_h(mh, module.len() as u16, ch, class.len() as u16, fh, field.len() as u16)
}

/// Hash-keyed lookup (no `String` keys on hot path; strings from target memory only).
///
/// Matches each scope/class/field on `(fnv1a_hash, byte_len)` — both must equal the wanted pair.
pub fn lookup_offset_h(
    dll_hash: u32,
    dll_len: u16,
    class_hash: u32,
    class_len: u16,
    field_hash: u32,
    field_len: u16,
) -> Option<u32> {
    {
        let g = cache().lock();
        if let Some(&o) = g.get(&(dll_hash, class_hash, field_hash)) {
            return Some(o);
        }
    }
    let off = lookup_uncached_h(dll_hash, dll_len, class_hash, class_len, field_hash, field_len)?;
    cache().lock().insert((dll_hash, class_hash, field_hash), off);
    Some(off)
}

fn lookup_uncached_h(
    dll_h: u32,
    dll_len: u16,
    class_h: u32,
    class_len: u16,
    field_h: u32,
    field_len: u16,
) -> Option<u32> {
    let scope = type_scope_h(dll_h, dll_len)?;
    let cls = resolve_class_h(dll_h, scope, class_h, class_len)?;
    field_off_h(dll_h, scope, cls, field_h, field_len, 0)
}

fn resolve_class_h(dll_h: u32, scope: usize, class_h: u32, class_len: u16) -> Option<usize> {
    let key = (dll_h, class_h);
    if let Some(&b) = class_cache().lock().get(&key) {
        return Some(b);
    }
    let b = find_class_h(scope, class_h, class_len)?;
    class_cache().lock().insert(key, b);
    Some(b)
}

fn type_scope_h(dll_h: u32, dll_len: u16) -> Option<usize> {
    let ss = schema_system()?;
    let type_scopes = ss.checked_add(SS_TYPE_SCOPES)?;
    let cnt = mem::read_u32_off(type_scopes, VEC_COUNT)? as usize;
    let data = mem::read_ptr(type_scopes, VEC_DATA)?;

    for i in 0..cnt {
        let elem = data.checked_add(i.checked_mul(VEC_STRIDE)?)?;
        let ptr = mem::read_usize(elem)?;
        if ptr == 0 {
            continue;
        }
        let name_p = ptr.checked_add(TS_NAME)?;
        if let Some(n) = mem::read_cstring(name_p, TS_NAME_LEN) {
            // Normalize for .dll suffix so fnv1a("client") and fnv1a("client.dll") both work.
            // Length discriminator: requested `dll_len` may correspond to either form.
            let base = n.trim_end_matches(".dll");
            let base_len = base.len() as u16;
            let full_len = base_len.saturating_add(4); // ".dll"
            let h_base = crate::fnv1a(base);
            // Hash for the `.dll`-suffixed form, computed without an extra allocation by
            // continuing FNV-1a-32 from `h_base` over the literal four extra bytes.
            let h_full = fnv1a_continue(h_base, b".dll");
            if (h_base == dll_h && base_len == dll_len) || (h_full == dll_h && full_len == dll_len)
            {
                return Some(ptr);
            }
        }
    }
    None
}

fn find_class_h(scope: usize, class_h: u32, class_len: u16) -> Option<usize> {
    let mut found = None;
    walk_classes(scope, |n, b| {
        if n.len() as u16 == class_len && crate::fnv1a(n) == class_h {
            found = Some(b);
            false
        } else {
            true
        }
    });
    found
}

/// Continue an in-progress FNV-1a-32 over `extra` bytes. Used to derive
/// `fnv1a("foo.dll")` from `fnv1a("foo")` without allocating a new String.
fn fnv1a_continue(mut hash: u32, extra: &[u8]) -> u32 {
    const FNV_PRIME: u32 = 0x01000193;
    let mut i = 0;
    while i < extra.len() {
        hash ^= extra[i] as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

fn walk_classes<F>(scope: usize, mut visit: F)
where
    F: FnMut(&str, usize) -> bool,
{
    let Some(hash) = scope.checked_add(TS_CLASS_BINDINGS) else {
        return;
    };
    let Some(mempool) = hash.checked_add(HASH_ENTRY_MEM) else {
        return;
    };
    let Some(buckets) = hash.checked_add(HASH_BUCKETS) else {
        return;
    };

    let alloc = mem::read_u32_off(mempool, MEMPOOL_ALLOC).unwrap_or(0) as usize;
    let peak = mem::read_u32_off(mempool, MEMPOOL_PEAK).unwrap_or(0) as usize;

    let mut seen = 0usize;
    let cap = if alloc == 0 { MAX_CHAIN } else { alloc };

    'outer: for b in 0..HASH_BUCKET_COUNT {
        let Some(bucket) =
            b.checked_mul(HASH_BUCKET_STRIDE).and_then(|offset| buckets.checked_add(offset))
        else {
            break;
        };

        let mut node = mem::read_usize_off(bucket, BUCKET_FIRST_UNC).unwrap_or(0);
        let mut len = 0usize;
        while node != 0 && len < MAX_CHAIN {
            len += 1;
            if let Some(binding) = mem::read_usize_off(node, FIXED_DATA) {
                if binding != 0 {
                    if !visit_binding(binding, &mut visit) {
                        return;
                    }
                    seen += 1;
                    if seen >= cap {
                        break 'outer;
                    }
                }
            } else {
                break;
            }
            node = mem::read_usize_off(node, FIXED_NEXT).unwrap_or(0);
        }
    }

    let mut blob = mem::read_usize_off(mempool, MEMPOOL_FREE_HEAD).unwrap_or(0);
    let mut len = 0usize;
    let cap2 = if peak == 0 { MAX_CHAIN } else { peak };
    let mut seen2 = 0usize;
    while blob != 0 && len < MAX_CHAIN {
        len += 1;
        if let Some(binding) = mem::read_usize_off(blob, BLOB_DATA) {
            if binding != 0 {
                if !visit_binding(binding, &mut visit) {
                    return;
                }
                seen2 += 1;
                if seen2 >= cap2 {
                    break;
                }
            }
        } else {
            break;
        }
        blob = mem::read_usize_off(blob, BLOB_NEXT).unwrap_or(0);
    }
}
fn visit_binding<F>(binding: usize, v: &mut F) -> bool
where
    F: FnMut(&str, usize) -> bool,
{
    let p = match mem::read_usize_off(binding, CLASS_NAME) {
        Some(x) => x,
        None => return true,
    };
    if p == 0 {
        return true;
    }
    if let Some(name) = mem::read_cstring(p, MAX_NAME) {
        v(&name, binding)
    } else {
        true
    }
}

fn field_off_h(
    dll_h: u32,
    scope: usize,
    cls: usize,
    want_h: u32,
    want_len: u16,
    depth: usize,
) -> Option<u32> {
    if let Some(off) = field_off_direct_h(cls, want_h, want_len) {
        return Some(off);
    }
    if depth >= MAX_BASE_DEPTH {
        return None;
    }
    let base_info = mem::read_usize_off(cls, CLASS_BASE_CLASSES).unwrap_or(0);
    if base_info == 0 {
        return None;
    }
    let parent_lite = mem::read_usize_off(base_info, BASE_INFO_CLASS).unwrap_or(0);
    if parent_lite == 0 {
        return None;
    }
    let name_ptr = mem::read_usize_off(parent_lite, BASE_CLASS_NAME).unwrap_or(0);
    if name_ptr == 0 {
        return None;
    }
    let (parent_h, parent_len) = mem::read_cstring_hash_len(name_ptr, MAX_NAME)?;
    let parent_cls = resolve_class_h(dll_h, scope, parent_h, parent_len)?;
    if parent_cls == cls {
        // Self-referential parent — bail rather than recurse forever.
        return None;
    }
    field_off_h(dll_h, scope, parent_cls, want_h, want_len, depth + 1)
}

fn field_off_direct_h(cls: usize, want_h: u32, want_len: u16) -> Option<u32> {
    let cnt = mem::read_i16_off(cls, CLASS_FIELD_COUNT).unwrap_or(0) as usize;
    if cnt == 0 {
        return None;
    }
    let base = mem::read_ptr(cls, CLASS_FIELDS)?;

    for i in 0..cnt {
        let e = base.checked_add(i.checked_mul(FIELD_STRIDE)?)?;
        let np = mem::read_usize_off(e, FIELD_NAME)?;
        if np == 0 {
            continue;
        }
        if let Some((nh, nl)) = mem::read_cstring_hash_len(np, MAX_NAME) {
            if nl == want_len && nh == want_h {
                return mem::read_u32_off(e, FIELD_OFF);
            }
        }
    }
    None
}
