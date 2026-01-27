# recqemu

QEMU command builder and utilities for Linux VM testing.

Fluent API for constructing QEMU commands with UEFI boot, virtio devices, and common configurations.

## Status

**Beta.** Library only (no CLI).

## Library Usage

```rust
use recqemu::{QemuBuilder, find_ovmf, create_disk};

// Create a disk
create_disk(Path::new("disk.qcow2"), "20G")?;

// Build QEMU command
let mut cmd = QemuBuilder::new()
    .memory("4G")
    .smp(4)
    .cdrom("/path/to/iso")
    .disk("/path/to/disk.qcow2")
    .uefi(find_ovmf().unwrap())
    .user_network()
    .build_interactive();

cmd.status()?;
```

## Features

### QemuBuilder

Fluent builder for QEMU commands:

```rust
QemuBuilder::new()
    .memory("4G")              // RAM size
    .smp(4)                    // CPU cores
    .cdrom(path)               // ISO via virtio-scsi
    .disk(path)                // Disk via virtio-blk
    .uefi(ovmf_path)           // UEFI boot with OVMF
    .uefi_vars(vars_path)      // Writable UEFI vars
    .user_network()            // QEMU user networking
    .display("gtk,gl=on")      // Display backend
    .vga("virtio")             // VGA adapter
    .serial_stdio()            // Serial to stdout
    .serial_file(path)         // Serial to file
    .nographic()               // Headless mode
    .no_reboot()               // Exit on reboot
    .build()                   // Get Command
    .build_interactive()       // stdin/stdout inherited
    .build_piped()             // stdin/stdout piped
```

### Utilities

```rust
// Find OVMF firmware
let ovmf = find_ovmf();           // Returns Option<PathBuf>
let vars = find_ovmf_vars();      // Returns Option<PathBuf>

// Create qcow2 disk
create_disk(Path::new("disk.qcow2"), "20G")?;

// Check KVM availability
if kvm_available() {
    println!("Hardware acceleration available");
}
```

## OVMF Search Paths

Searches in order:
- `/usr/share/edk2/ovmf/OVMF_CODE.fd` (Fedora)
- `/usr/share/OVMF/OVMF_CODE.fd` (Debian/Ubuntu)
- `/usr/share/ovmf/x64/OVMF_CODE.fd` (Arch)

## What It Does

- Builds QEMU command lines with sensible defaults
- Handles OVMF firmware discovery across distros
- Creates qcow2 disks with qemu-img
- Detects KVM availability

## What It Does NOT Do

- Run QEMU (returns `Command` for you to run)
- Handle serial console I/O (see install-tests)
- Implement QMP protocol (see install-tests)
- Manage test coordination/locking

## Requirements

- qemu-system-x86_64: `sudo dnf install qemu-system-x86`
- OVMF: `sudo dnf install edk2-ovmf`

## Building

```bash
cargo build --release
```

## Consumers

- `leviso` - `run_iso()` and `test_iso()` commands
- `install-tests` - E2E testing (wraps and adds anti-cheat)

## License

MIT OR Apache-2.0
