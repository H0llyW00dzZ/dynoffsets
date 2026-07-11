//! Test-only mock Process implementation.
//!
//! A single global `MockProcess` is installed as the registered [`crate::Process`].
//! Tests acquire `TEST_LOCK` first (serialising them), reset the state, populate
//! the fake memory / modules / exports, then drive the runtime walkers.

#![allow(dead_code)]
#![cfg(test)]

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::Process;

pub struct MockState {
    pub modules: HashMap<String, usize>,
    pub mem: HashMap<usize, u8>,
    pub exports: HashMap<String, HashMap<String, usize>>,
    /// Any address whose first byte is in this set causes `read_bytes` to fail.
    pub deny: HashSet<usize>,
}

impl MockState {
    fn new() -> Self {
        Self {
            modules: HashMap::new(),
            mem: HashMap::new(),
            exports: HashMap::new(),
            deny: HashSet::new(),
        }
    }

    pub fn reset(&mut self) {
        self.modules.clear();
        self.mem.clear();
        self.exports.clear();
        self.deny.clear();
    }

    pub fn add_module(&mut self, name: &str, base: usize) {
        self.modules.insert(name.to_string(), base);
    }

    pub fn add_export(&mut self, module: &str, name: &str, addr: usize) {
        self.exports.entry(module.to_string()).or_default().insert(name.to_string(), addr);
    }

    pub fn write_bytes(&mut self, addr: usize, data: &[u8]) {
        for (i, &b) in data.iter().enumerate() {
            self.mem.insert(addr + i, b);
        }
    }

    pub fn write_usize(&mut self, addr: usize, v: usize) {
        self.write_bytes(addr, &v.to_le_bytes());
    }

    pub fn write_u32(&mut self, addr: usize, v: u32) {
        self.write_bytes(addr, &v.to_le_bytes());
    }

    pub fn write_i16(&mut self, addr: usize, v: i16) {
        self.write_bytes(addr, &v.to_le_bytes());
    }

    pub fn write_cstr(&mut self, addr: usize, s: &str) {
        self.write_bytes(addr, s.as_bytes());
        self.mem.insert(addr + s.len(), 0);
    }

    pub fn deny_read(&mut self, addr: usize) {
        self.deny.insert(addr);
    }
}

static MOCK_STATE: OnceLock<Mutex<MockState>> = OnceLock::new();
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn mock_state() -> &'static Mutex<MockState> {
    MOCK_STATE.get_or_init(|| Mutex::new(MockState::new()))
}

struct MockHandle;

impl Process for MockHandle {
    fn read_bytes(&self, addr: usize, buf: &mut [u8]) -> Option<()> {
        let s = mock_state().lock().unwrap();
        // Any byte in [addr, addr+len) being denied fails the whole read.
        for i in 0..buf.len() {
            if s.deny.contains(&(addr + i)) {
                return None;
            }
        }
        // Unwritten bytes are 0 (mimics OS zero-init of committed pages).
        for (i, slot) in buf.iter_mut().enumerate() {
            *slot = s.mem.get(&(addr + i)).copied().unwrap_or(0);
        }
        Some(())
    }

    fn module_base(&self, m: &str) -> Option<usize> {
        mock_state().lock().unwrap().modules.get(m).copied()
    }

    fn get_proc_address(&self, m: &str, n: &str) -> Option<usize> {
        mock_state().lock().unwrap().exports.get(m).and_then(|x| x.get(n)).copied()
    }
}

fn install() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        crate::init(MockHandle);
    });
}

/// Acquire the global test lock, install the mock (once), reset its state and
/// clear sigscan overrides. Hold the returned guard for the duration of the test.
pub fn setup() -> MutexGuard<'static, ()> {
    let g = match TEST_LOCK.lock() {
        Ok(x) => x,
        Err(p) => p.into_inner(),
    };
    install();
    mock_state().lock().unwrap().reset();
    crate::sigscan::clear_pattern_overrides();
    crate::walker::_test_reset_schema_system();
    crate::r#static::_test_clear();
    g
}

/// Convenience: populate state with a closure (lock auto-released afterwards).
pub fn populate(f: impl FnOnce(&mut MockState)) {
    let mut s = mock_state().lock().unwrap();
    f(&mut s);
}
