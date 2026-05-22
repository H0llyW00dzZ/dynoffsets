//! Runtime schema field offset walker.
//!
//! Finds the SchemaSystem singleton via signature, then walks the type
//! scopes and class bindings to resolve field offsets for the `#[schema]`
//! macro.

use alloc::string::{String, ToString};
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use hashbrown::HashMap;

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

const FIELD_NAME: usize = 0;
const FIELD_OFF: usize = 0x10;
const FIELD_STRIDE: usize = 0x20;

static SCHEMA_SYSTEM: AtomicUsize = AtomicUsize::new(0);
static RESOLVED: AtomicBool = AtomicBool::new(false);

type Key = (String, String, String);

fn cache() -> &'static Mutex<HashMap<Key, u32>> {
    static C: OnceCell<Mutex<HashMap<Key, u32>>> = OnceCell::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
pub(crate) fn _test_reset_schema_system() {
    SCHEMA_SYSTEM.store(0, Ordering::Release);
    RESOLVED.store(false, Ordering::Release);
    cache().lock().clear();
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
    let v =
        sigscan::find_pattern_rip32("schemasystem.dll", SCHEMA_SYSTEM_PATTERN, SCHEMA_SYSTEM_DISP)
            .unwrap_or(0);
    SCHEMA_SYSTEM.store(v, Ordering::Release);
    RESOLVED.store(true, Ordering::Release);
    (v != 0).then_some(v)
}

fn cstr(addr: usize, max: usize) -> Option<String> {
    let mut b = vec![0u8; max];
    crate::process()?.read_bytes(addr, &mut b)?;
    let n = b.iter().position(|&x| x == 0).unwrap_or(max);
    core::str::from_utf8(&b[..n]).ok().map(Into::into)
}

pub fn lookup_offset(module: &str, class: &str, field: &str) -> Option<u32> {
    {
        let g = cache().lock();
        if let Some(&o) = g.get(&(module.to_string(), class.to_string(), field.to_string())) {
            return Some(o);
        }
    }
    let off = lookup_uncached(module, class, field)?;
    cache().lock().insert((module.to_string(), class.to_string(), field.to_string()), off);
    Some(off)
}

fn lookup_uncached(m: &str, c: &str, f: &str) -> Option<u32> {
    let scope = type_scope(m)?;
    let cls = find_class(scope, c)?;
    field_off(cls, f)
}

fn type_scope(module: &str) -> Option<usize> {
    let ss = schema_system()?;
    let vec = ss + SS_TYPE_SCOPES;
    let cnt = mem::read_u32_off(vec, VEC_COUNT)? as usize;
    let data = mem::read_ptr(vec, VEC_DATA)?;

    for i in 0..cnt {
        let elem = data.checked_add(i * VEC_STRIDE)?;
        let ptr = mem::read_usize(elem)?;
        if ptr == 0 {
            continue;
        }
        let name_p = ptr + TS_NAME;
        if let Some(n) = cstr(name_p, TS_NAME_LEN) {
            if n == module {
                return Some(ptr);
            }
        }
    }
    None
}

fn find_class(scope: usize, name: &str) -> Option<usize> {
    let mut found = None;
    walk_classes(scope, |n, b| {
        if n == name {
            found = Some(b);
            false
        } else {
            true
        }
    });
    found
}

fn walk_classes<F>(scope: usize, mut visit: F)
where
    F: FnMut(&str, usize) -> bool,
{
    let hash = scope + TS_CLASS_BINDINGS;
    let mempool = hash + HASH_ENTRY_MEM;
    let buckets = hash + HASH_BUCKETS;

    let alloc = mem::read_u32_off(mempool, MEMPOOL_ALLOC).unwrap_or(0) as usize;
    let peak = mem::read_u32_off(mempool, MEMPOOL_PEAK).unwrap_or(0) as usize;

    let mut seen = 0usize;
    let cap = if alloc == 0 { MAX_CHAIN } else { alloc };

    'outer: for b in 0..HASH_BUCKET_COUNT {
        let bucket = buckets + b * HASH_BUCKET_STRIDE;
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

fn field_off(cls: usize, want: &str) -> Option<u32> {
    let cnt = mem::read_i16_off(cls, CLASS_FIELD_COUNT)? as usize;
    let base = mem::read_ptr(cls, CLASS_FIELDS)?;

    for i in 0..cnt {
        let e = base.checked_add(i * FIELD_STRIDE)?;
        let np = mem::read_usize_off(e, FIELD_NAME)?;
        if np == 0 {
            continue;
        }
        if let Some(n) = mem::read_cstring(np, MAX_NAME) {
            if n == want {
                return mem::read_u32_off(e, FIELD_OFF);
            }
        }
    }
    None
}
