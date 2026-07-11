//! Registry for `r#static` macro mode.
//!
//! `#[globals(r#static)]` etc. emit `AtomicUsize` cells and a
//! `__dynoffsets_register()` fn. Call registers after `init`, then
//! `populate()` to fill live values from the process.

use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
#[cfg(feature = "runtime")]
use core::sync::atomic::Ordering;

use spin::Mutex;

/// `(name, &AtomicUsize)` for a `r#static` slot.
pub type Slot = (&'static str, &'static AtomicUsize);

/// (module, class, slots) for schema `r#static`.
type SchemaEntry = (&'static str, &'static str, &'static [Slot]);
/// (module, slots) for interfaces `r#static`.
type InterfaceEntry = (&'static str, &'static [Slot]);

static GLOBALS: Mutex<Vec<&'static [Slot]>> = Mutex::new(Vec::new());
static SCHEMA: Mutex<Vec<SchemaEntry>> = Mutex::new(Vec::new());
static INTERFACES: Mutex<Vec<InterfaceEntry>> = Mutex::new(Vec::new());
static BUTTONS: Mutex<Vec<&'static [Slot]>> = Mutex::new(Vec::new());

/// Internal. Called by generated `__dynoffsets_register()`.
#[doc(hidden)]
pub fn register_globals(slots: &'static [Slot]) {
    GLOBALS.lock().push(slots);
}

/// Internal. Called by generated `__dynoffsets_register()`.
#[doc(hidden)]
pub fn register_schema(module: &'static str, class: &'static str, slots: &'static [Slot]) {
    SCHEMA.lock().push((module, class, slots));
}

/// Internal. Called by generated `__dynoffsets_register()`.
#[doc(hidden)]
pub fn register_interfaces(module: &'static str, slots: &'static [Slot]) {
    INTERFACES.lock().push((module, slots));
}

/// Internal. Called by generated `__dynoffsets_register()`.
#[doc(hidden)]
pub fn register_buttons(slots: &'static [Slot]) {
    BUTTONS.lock().push(slots);
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PopulateStats {
    pub globals: usize,
    pub schema: usize,
    pub interfaces: usize,
    pub buttons: usize,
}

#[cfg(feature = "runtime")]
fn populate_simple(
    reg: &Mutex<Vec<&'static [Slot]>>,
    count: &mut usize,
    get: impl Fn(&str) -> Option<usize>,
) {
    for group in reg.lock().iter() {
        for (name, atom) in group.iter() {
            if let Some(v) = get(name) {
                atom.store(v, Ordering::Relaxed);
                *count += 1;
            }
        }
    }
}

/// Populates registered `r#static` slots via live discovery (or no-op).
/// Returns per-category counts. See [`PopulateStats`].
#[allow(unused_mut)]
pub fn populate() -> PopulateStats {
    let mut stats = PopulateStats::default();

    #[cfg(feature = "runtime")]
    {
        if crate::process().is_some() {
            let g = crate::offsets::discover_globals();
            populate_simple(&GLOBALS, &mut stats.globals, |n| g.get(n));

            for (module, class, group) in SCHEMA.lock().iter() {
                for (field, atom) in group.iter() {
                    if let Some(off) = crate::walker::lookup_offset(module, class, field) {
                        atom.store(off as usize, Ordering::Relaxed);
                        stats.schema += 1;
                    }
                }
            }

            let i = crate::interfaces::discover_interfaces();
            for (module, group) in INTERFACES.lock().iter() {
                for (name, atom) in group.iter() {
                    if let Some(v) = i.get(module, name) {
                        atom.store(v, Ordering::Relaxed);
                        stats.interfaces += 1;
                    }
                }
            }

            let b = crate::buttons::discover_buttons();
            populate_simple(&BUTTONS, &mut stats.buttons, |n| b.get(n));
        }
    }

    stats
}

#[cfg(test)]
pub(crate) fn _test_clear() {
    GLOBALS.lock().clear();
    SCHEMA.lock().clear();
    INTERFACES.lock().clear();
    BUTTONS.lock().clear();
}

#[cfg(test)]
pub(crate) fn _test_counts() -> (usize, usize, usize, usize) {
    (GLOBALS.lock().len(), SCHEMA.lock().len(), INTERFACES.lock().len(), BUTTONS.lock().len())
}
