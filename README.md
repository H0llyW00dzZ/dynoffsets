# dynoffsets
[![codecov](https://codecov.io/gh/H0llyW00dzZ/dynoffsets/graph/badge.svg?token=Q1W56W96L7)](https://codecov.io/gh/H0llyW00dzZ/dynoffsets)
[![Crates.io](https://img.shields.io/crates/v/dynoffsets.svg)](https://crates.io/crates/dynoffsets)
[![Documentation](https://docs.rs/dynoffsets/badge.svg)](https://docs.rs/dynoffsets)

Resolve offsets, globals, interfaces and buttons at call time with literal fallbacks.

<table align="center"><tr><td>

```console
     _                   __  __          _       
  __| |_   _ _ __   ___ / _|/ _|___  ___| |_ ___ 
 / _` | | | | '_ \ / _ \ |_| |_/ __|/ _ \ __/ __|
| (_| | |_| | | | | (_) |  _|  _\__ \  __/ |_\__ \ 
 \__,_|\__, |_| |_|\___/|_| |_| |___/\___|\__|___/
       |___/                                     

  dynoffsets by H0llyW00dzZ (@github.com/H0llyW00dzZ)
```

</td></tr></table>

## Current Release

- Official public source of truth: GitHub
- Repository: <https://github.com/H0llyW00dzZ/dynoffsets>

This project is intended to be open-sourced publicly on GitHub. If you see the
same source posted elsewhere, for example on UC forums, treat it as an
unofficial repost or copy of this repository unless the GitHub repository links
to it explicitly.

## Installation

**Recommended**:

```console
cargo add dynoffsets
cargo add dynoffsets --features runtime     # for runtime discovery
```

**Before it is published**, use the git dependency:

```console
cargo add --git https://github.com/H0llyW00dzZ/dynoffsets
cargo add --git https://github.com/H0llyW00dzZ/dynoffsets --features runtime
```

**Local development** (when you have a local copy of dynoffsets):

```toml
dynoffsets = { path = "../dynoffsets" }
```

Use this when you're working on dynoffsets itself together with another project.

Recommended usage (with macros):

```rust
#[schema]
pub mod C_BaseEntity {
    pub const m_iHealth: usize = 0xDEAD_BEEF;
}

#[globals]
pub mod client_dll {
    pub const dw_entity_list: usize = 0xDEAD_BEEF;
}

// Access as functions (live value or dead)
let hp   = C_BaseEntity::m_iHealth();
let list = client_dll::dw_entity_list();
```

With the `runtime` feature + a `Process` impl, you get live values.
Without it, you get the literal. `no_std` + `alloc` ok.

### Custom memory backends

dynoffsets is **backend-agnostic** — you must bring your own `Process` implementation:

```rust
impl Process for MyBackend { ... }   // usermode, kernel, DMA, etc.
dynoffsets::init(MyBackend::new());
```

Supported backends include (but are not limited to):
- usermode `ReadProcessMemory`
- kernel drivers (any IOCTL, physical memory, etc.)
- DMA / PCIe cards, FPGA, Thunderbolt DMA
- hypervisor / VM introspection

Pattern scanning for runtime globals and related discovery now goes through
reader-backed `pe-sigscan` APIs, so custom backends are supported for scanning
too, not just primitive reads.

If your backend is purely local and can safely dereference module memory in the
current process, override `Process::scan_text` and `Process::resolve_rel32_at`
to call the direct `pe-sigscan` fast path for maximum performance.

See docs.rs for the four attribute macros and the `Process` trait.

## Using With MinHook

`dynoffsets` does not ship a detour API or MinHook bindings. Its job is to
resolve addresses; your hook layer installs the hook.

The most common pattern is:

1. Use `#[interfaces]` to resolve a live interface pointer.
2. Read the vtable slot for the method you want to hook.
3. Pass that function entry address to MinHook.

`#[schema]`, `#[globals]`, and `#[buttons]` usually resolve data addresses, not
hook targets by themselves.

Example sketch using `minhook-sys`:

```rust
use core::{ffi::c_void, mem, ptr};

use dynoffsets::interfaces;
use minhook_sys::{MH_CreateHook, MH_EnableHook, MH_Initialize, MH_OK};

#[interfaces("engine2.dll")]
mod engine2 {
    pub const Source2EngineToClient001: usize = 0;
}

type TargetFn = unsafe extern "system" fn(this: *mut c_void, arg: i32) -> i32;

static mut ORIGINAL_TARGET: Option<TargetFn> = None;

unsafe extern "system" fn hk_target(this: *mut c_void, arg: i32) -> i32 {
    let original = ORIGINAL_TARGET.expect("hook not installed");
    original(this, arg)
}

unsafe fn vfunc(instance: usize, index: usize) -> *mut c_void {
    let vtable = *(instance as *const *const usize);
    *vtable.add(index) as *mut c_void
}

unsafe fn install_hook() {
    let iface = engine2::Source2EngineToClient001();
    assert_ne!(iface, 0, "interface was not resolved");

    // Replace 42 with the real vtable index for the method you want.
    let target = vfunc(iface, 42);

    assert_eq!(MH_Initialize(), MH_OK);

    let mut original = ptr::null_mut();
    assert_eq!(
        MH_CreateHook(target, hk_target as *mut c_void, &mut original),
        MH_OK,
    );

    ORIGINAL_TARGET = Some(mem::transmute(original));

    assert_eq!(MH_EnableHook(target), MH_OK);
}
```

The same idea works for exported functions too: resolve the function entry with
your own backend or module logic, then hand that address to MinHook.

## TODO

- [ ] Native Linux support. The crate is Windows-only today.

## cs2-dumper vs dynoffsets

[cs2-dumper](https://github.com/a2x/cs2-dumper) is an external analysis tool that emits static offset headers.

dynoffsets is the library alternative: it bakes the same patterns into the binary and resolves them at load time (falling back to the literals you wrote when the runtime feature is disabled).

Small game updates that only move addresses can often be survived without regenerating headers.

MSRV 1.72. [MIT](./LICENSE).
