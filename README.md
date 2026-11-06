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
    .cdrom("/path/to/iso")
    .disk("/path/to/disk.qcow2")
    .uefi(find_ovmf()?)
    .user_network()
    .build();

cmd.status()?;
```

## Features

### QemuBuilder

Fluent builder for QEMU commands:

```rust
QemuBuilder::new()              // 4G, 4 CPUs, KVM when /dev/kvm exists
    .cdrom(path)               // ISO via virtio-scsi
    .cdrom_as_disk()           // also expose the ISO's appended partitions
    .extra_cdrom(path)         // another read-only CD
    .disk(path)                // Disk via virtio-blk (qcow2/raw detected)
    .uefi(ovmf_path)           // UEFI boot with OVMF
    .uefi_vars(vars_path)      // Writable UEFI vars
    .user_network()            // QEMU user networking
    .display("gtk,gl=on")      // Display backend
    .vga("virtio")             // VGA adapter
    .serial_stdio()            // Serial to stdout (or .serial_file(path))
    .nographic()               // Headless mode
    .no_reboot()               // Exit on reboot
    .build()                   // Get Command
    .build_piped()             // stdin/stdout piped, for Console
```

### Utilities

```rust
// Find OVMF firmware
let ovmf = find_ovmf()?;          // OVMF_PATH, else distro package paths
let vars = find_ovmf_vars()?;     // OVMF_VARS_PATH, else the VARS file next to it

// Create qcow2 disk
create_disk(Path::new("disk.qcow2"), "20G")?;

// Check KVM availability
if kvm_available() {
    println!("Hardware acceleration available");
}
```

## OVMF Search Paths

`OVMF_PATH` (and `OVMF_VARS_PATH`) when set — a path that is not a file is an
error — else the Fedora, Debian/Ubuntu, Arch and NixOS package locations in
`OVMF_FILES` in `src/lib.rs`.

## What It Does

- Builds QEMU command lines with sensible defaults
- Handles OVMF firmware discovery across distros
- Creates qcow2 disks with qemu-img
- Detects KVM availability

## Serial test protocol

`serial::protocol` and `guest/serial-test.sh` are the two sides of it: the
guest script (installed as a profile.d script by the live overlays) turns echo
off on the serial console, empties the prompt and prints `SHELL_READY_MARKER`;
`Console::exec` wraps each command in unique `printf`-printed markers and reads
its exit status, and `Console::login` drives getty and confirms the shell
with a marker probe.

## What It Does NOT Do

- Run QEMU (returns `Command` for you to run)
- Implement QMP protocol (see install-tests)

## Requirements

- qemu-system-x86_64: `sudo dnf install qemu-system-x86`
- OVMF: `sudo dnf install edk2-ovmf`

## Building

```bash
cargo build --release
```

## Consumers

- `xtask` - `cargo xtask scenarios boot`
- `install-tests` - E2E testing

## License

MIT OR Apache-2.0
