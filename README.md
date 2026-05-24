# dynoffsets
[![codecov](https://codecov.io/gh/H0llyW00dzZ/dynoffsets/graph/badge.svg?token=Q1W56W96L7)](https://codecov.io/gh/H0llyW00dzZ/dynoffsets)

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

Windows only today. TODO: Linux support.

See docs.rs for the four attribute macros and the `Process` trait.

## cs2-dumper vs dynoffsets

[cs2-dumper](https://github.com/a2x/cs2-dumper) is an external analysis tool that emits static offset headers.

dynoffsets is the library alternative: it bakes the same patterns into the binary and resolves them at load time (falling back to the literals you wrote when the runtime feature is disabled).

Small game updates that only move addresses can often be survived without regenerating headers.

MSRV 1.72. [MIT](./LICENSE).
