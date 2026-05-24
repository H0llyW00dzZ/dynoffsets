//! Source 2 interface discovery via CreateInterface registration chains.
//!
//! Powers the `#[interfaces]` macro. See [`RuntimeInterfaces`],
//! [`discover_interfaces`] and [`discover_interfaces_in`].

use alloc::string::String;
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
    let modules = [
        String::from(obfstr!("animationsystem.dll")),
        String::from(obfstr!("client.dll")),
        String::from(obfstr!("engine2.dll")),
        String::from(obfstr!("filesystem_stdio.dll")),
        String::from(obfstr!("host.dll")),
        String::from(obfstr!("inputsystem.dll")),
        String::from(obfstr!("localize.dll")),
        String::from(obfstr!("matchmaking.dll")),
        String::from(obfstr!("materialsystem2.dll")),
        String::from(obfstr!("meshsystem.dll")),
        String::from(obfstr!("navsystem.dll")),
        String::from(obfstr!("networksystem.dll")),
        String::from(obfstr!("panorama.dll")),
        String::from(obfstr!("particles.dll")),
        String::from(obfstr!("pulse_system.dll")),
        String::from(obfstr!("rendersystemdx11.dll")),
        String::from(obfstr!("resourcesystem.dll")),
        String::from(obfstr!("scenefilecache.dll")),
        String::from(obfstr!("scenesystem.dll")),
        String::from(obfstr!("schemasystem.dll")),
        String::from(obfstr!("server.dll")),
        String::from(obfstr!("soundsystem.dll")),
        String::from(obfstr!("steamaudio.dll")),
        String::from(obfstr!("tier0.dll")),
        String::from(obfstr!("v8system.dll")),
        String::from(obfstr!("vphysics2.dll")),
        String::from(obfstr!("vscript.dll")),
        String::from(obfstr!("worldrenderer.dll")),
    ];
    let modules = modules.iter().map(String::as_str).collect::<Vec<_>>();
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

        let entries: HashMap<_, _> = walk(head).into_iter().collect();
        if !entries.is_empty() {
            out.map.insert(m.to_owned(), entries);
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
