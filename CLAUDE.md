# CLAUDE.md - recqemu

## What is recqemu?

Shared QEMU command builder, serial console, and utilities. Used by both leviso (development) and install-tests (E2E testing).

## What Belongs Here

- `QemuBuilder` - fluent QEMU command construction
- `find_ovmf()` / `find_ovmf_vars()` - OVMF firmware discovery
- `create_disk()` - qcow2 disk creation
- `kvm_available()` - KVM detection
- `patterns` - Boot/error pattern constants
- `process` - Test locking and stale process cleanup
- `serial` - Serial console I/O (Console, exec, auth, boot)

## What Does NOT Belong Here

| Don't put here | Put it in |
|----------------|-----------|
| QMP protocol | `testing/install-tests/src/qemu/qmp/` |
| Anti-cheat protections | `testing/install-tests/src/qemu/builder.rs` |
| DistroContext-aware code | `testing/install-tests/` |
| Executor trait | `testing/install-tests/src/executor.rs` |

## Commands

```bash
cargo build
cargo test
```

## Usage

```rust
use recqemu::{QemuBuilder, Console, find_ovmf, create_disk};

// Create a disk
create_disk(Path::new("disk.qcow2"), "20G")?;

// Build QEMU command
let mut cmd = QemuBuilder::new()
    .memory("4G")
    .cdrom(iso_path)
    .disk(disk_path)
    .uefi(find_ovmf().unwrap())
    .user_network()
    .nographic()
    .build_piped();

// Spawn and control via serial
let mut child = cmd.spawn()?;
let mut console = Console::new(&mut child)?;
console.wait_for_boot(Duration::from_secs(90))?;
console.exec_ok("ls -la", Duration::from_secs(10))?;
```

## Consumers

- `leviso` - `run_iso()` and `test_iso()` commands
- `install-tests` - E2E installation testing (extends with anti-cheat)
- `fsdbg` - Future: boot command for runtime verification
