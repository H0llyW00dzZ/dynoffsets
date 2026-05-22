//! Source 2 interface discovery via CreateInterface registration chains.
//!
//! Powers the `#[interfaces]` macro. See [`RuntimeInterfaces`],
//! [`discover_interfaces`] and [`discover_interfaces_in`].

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use hashbrown::HashMap;
use obfstr::obfstr;

use crate::mem;
use crate::sigscan::resolve_rip32_at;

/// Map of module → interface name → instance pointer obtained by walking
/// each module's `CreateInterface` registration list.
#[derive(Debug, Default, Clone)]
pub struct RuntimeInterfaces {
    pub map: HashMap<String, HashMap<String, usize>>,
}

impl RuntimeInterfaces {
    #[inline]
    pub fn get(&self, module: &str, name: &str) -> Option<usize> {
        self.map.get(module).and_then(|m| m.get(name)).copied()
    }
}

const REG_CREATE: usize = 0x00;
const REG_NAME: usize = 0x08;
const REG_NEXT: usize = 0x10;
const MAX_CHAIN: usize = 1024;
const MAX_NAME: usize = 128;

/// Walk `CreateInterface` registration chains for all known CS2 modules
/// and return the collected interface instances.
///
/// This is the version used by the `#[interfaces]` macro by default.
pub fn discover_interfaces() -> RuntimeInterfaces {
    let owned: Vec<String> = vec![
        obfstr!("animationsystem.dll").to_string(),
        obfstr!("client.dll").to_string(),
        obfstr!("engine2.dll").to_string(),
        obfstr!("filesystem_stdio.dll").to_string(),
        obfstr!("host.dll").to_string(),
        obfstr!("inputsystem.dll").to_string(),
        obfstr!("localize.dll").to_string(),
        obfstr!("matchmaking.dll").to_string(),
        obfstr!("materialsystem2.dll").to_string(),
        obfstr!("meshsystem.dll").to_string(),
        obfstr!("navsystem.dll").to_string(),
        obfstr!("networksystem.dll").to_string(),
        obfstr!("panorama.dll").to_string(),
        obfstr!("particles.dll").to_string(),
        obfstr!("pulse_system.dll").to_string(),
        obfstr!("rendersystemdx11.dll").to_string(),
        obfstr!("resourcesystem.dll").to_string(),
        obfstr!("scenefilecache.dll").to_string(),
        obfstr!("scenesystem.dll").to_string(),
        obfstr!("schemasystem.dll").to_string(),
        obfstr!("server.dll").to_string(),
        obfstr!("soundsystem.dll").to_string(),
        obfstr!("steamaudio.dll").to_string(),
        obfstr!("tier0.dll").to_string(),
        obfstr!("v8system.dll").to_string(),
        obfstr!("vphysics2.dll").to_string(),
        obfstr!("vscript.dll").to_string(),
        obfstr!("worldrenderer.dll").to_string(),
    ];
    let modules: Vec<&str> = owned.iter().map(String::as_str).collect();
    discover_interfaces_in(&modules)
}

/// Walk `CreateInterface` for a caller-provided list of modules.
///
/// Useful when you only care about a subset of modules or want to
/// control the order. The `#[interfaces]` macro ultimately calls this.
pub fn discover_interfaces_in(modules: &[&str]) -> RuntimeInterfaces {
    let mut out = RuntimeInterfaces::default();
    let Some(p) = crate::process() else {
        return out;
    };

    for &m in modules {
        let Some(ci) = p.get_proc_address(m, obfstr!("CreateInterface")) else {
            continue;
        };
        let Some(cell) = resolve_rip32_at(ci, 3) else {
            continue;
        };
        let Some(head) = mem::read_usize(cell) else {
            continue;
        };
        if head == 0 {
            continue;
        }

        let entries = walk(head);
        if !entries.is_empty() {
            let mut mm = HashMap::with_capacity(entries.len());
            for (k, v) in entries {
                mm.insert(k, v);
            }
            out.map.insert(m.to_string(), mm);
        }
    }
    out
}

fn walk(head: usize) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut node = head;
    for _ in 0..MAX_CHAIN {
        if node == 0 {
            break;
        }
        let Some(create) = mem::read_usize_off(node, REG_CREATE) else {
            break;
        };
        let Some(namep) = mem::read_usize_off(node, REG_NAME) else {
            break;
        };
        let next = mem::read_usize_off(node, REG_NEXT).unwrap_or(0);

        if create != 0 && namep != 0 {
            if let Some(name) = mem::read_cstring(namep, MAX_NAME) {
                if let Some(inst) = resolve_rip32_at(create, 3) {
                    out.push((name, inst));
                }
            }
        }
        node = next;
    }
    out
}
