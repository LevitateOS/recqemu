# CLAUDE.md - recqemu

## What is recqemu?

Shared QEMU command builder and utilities. Used by both leviso (development) and install-tests (E2E testing).

## What Belongs Here

- `QemuBuilder` - fluent QEMU command construction
- `find_ovmf()` / `find_ovmf_vars()` - OVMF firmware discovery
- `create_disk()` - qcow2 disk creation
- `kvm_available()` - KVM detection

## What Does NOT Belong Here

| Don't put here | Put it in |
|----------------|-----------|
| Serial console I/O | `testing/install-tests/src/qemu/serial/` |
| QMP protocol | `testing/install-tests/src/qemu/qmp/` |
| Boot pattern matching | `leviso/src/qemu.rs` |
| Anti-cheat protections | `testing/install-tests/src/qemu/builder.rs` |
| Test locking | `testing/install-tests/` |

## Commands

```bash
cargo build
cargo test
```

## Usage

```rust
use recqemu::{QemuBuilder, find_ovmf, create_disk};

// Create a disk
create_disk(Path::new("disk.qcow2"), "20G")?;

// Build QEMU command
let mut cmd = QemuBuilder::new()
    .memory("4G")
    .cdrom(iso_path)
    .disk(disk_path)
    .uefi(find_ovmf().unwrap())
    .user_network()
    .build_interactive();

cmd.status()?;
```

## Consumers

- `leviso` - `run_iso()` and `test_iso()` commands
- `install-tests` - E2E installation testing (extends with anti-cheat)
