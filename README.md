# recqemu

Scenario VMs as libvirt domains, and the serial console that drives them.

Every VM the scenarios and `cargo xtask run` boot is a transient libvirt
domain on the user's session (`qemu:///session`, `libvirt::URI`),
so it can be watched and managed in Cockpit (Virtual Machines, user session)
or with `virsh -c qemu:///session list`. libvirt runs QEMU as the user, so
images under the user's home need no relabelling.

## Status

**Beta.** Library only (no CLI).

## Usage

```rust
use recqemu::libvirt::{Domain, DomainSpec, SerialTarget, UefiVars};
use recqemu::serial::Console;

let spec = DomainSpec {
    name: "levitate-live-boot".into(),
    title: "Scenario VM levitate-live-boot".into(),
    memory_mib: DomainSpec::DEFAULT_MEMORY_MIB,
    vcpus: DomainSpec::DEFAULT_VCPUS,
    uefi_code: recqemu::find_ovmf()?,
    uefi_vars: UefiVars::Fresh { template: recqemu::find_ovmf_vars()?, path: vars_path },
    disk: Some(disk),                 // virtio, qcow2 or raw (detected)
    boot_iso: Some(iso),              // booted first: a CD and a read-only disk
    extra_cdroms: vec![],             // CDs that are not booted
    ssh_forward: Some(port),          // 127.0.0.1:port -> guest 22 (passt)
    serial: SerialTarget::Socket(socket.clone()),
    serial_log: log,                  // every serial line, appended
};
let domain = Domain::create_paused(&spec)?;   // paused: no output is missed
let mut console = Console::connect(&socket, Duration::from_secs(20))?;
domain.resume()?;
// ... console.wait_for_boot(...), console.login(...), console.exec(...)
// Dropping `domain` destroys it; a guest reboot or power-off ends it too.
// A running domain of the same name is an error, never killed.
```

Each domain has the memory and CPUs its spec gives (KVM when `/dev/kvm` exists, else TCG), a
q35 machine with UEFI (OVMF), user networking through passt, a VNC display on
127.0.0.1 for Cockpit, and `on_reboot`/`on_poweroff` set to destroy, so a
guest reboot ends the domain and the caller starts the next boot.
`SerialTarget::Pty` instead gives a pty for `virsh console`
(`cargo xtask run`), and `Domain::keep_running` leaves a domain running after
its handle is dropped.

### Utilities

```rust
let ovmf = find_ovmf()?;          // the host's OVMF code image
let vars = find_ovmf_vars()?;     // the VARS template paired with it
create_disk(Path::new("disk.qcow2"), "20G")?;   // qemu-img
create_overlay(Path::new("disk.qcow2"), Path::new("run.qcow2"))?;   // writes stay in run.qcow2
```

## OVMF Search Paths

The host's packages only (libvirt's QEMU runs every VM, so its host's
firmware is the one that matches): the Fedora, Debian/Ubuntu, Arch and NixOS
CODE/VARS pairs in `OVMF_FILES` in `src/lib.rs`.

## Serial test protocol

`serial::protocol` and `guest/serial-test.sh` are the two sides of it: the
guest script (installed as a profile.d script by the live overlays) turns echo
off on the serial console, empties the prompt and prints `SHELL_READY_MARKER`;
`Console::exec` (the `Guest` interface, which `recshuttle::Remote` also
implements; a command that runs out of time is `Outcome::TimedOut`, and its
late output is drained before the next one) wraps each command in unique
`printf`-printed markers and reads
its exit status, and `Console::login` drives getty and confirms the shell
with a marker probe.

## Requirements

- libvirt with the QEMU driver: `sudo dnf install libvirt-daemon-kvm libvirt-client`
  (the session daemon starts on demand)
- passt: `sudo dnf install passt`
- OVMF: `sudo dnf install edk2-ovmf`
- qemu-img (for `create_disk` and `create_overlay`)
- Cockpit with `cockpit-machines`, to manage the VMs in a browser (optional)

`cargo xtask doctor` checks these.

## Consumers

- `xtask` - `cargo xtask run`
- `install-tests` - the scenarios

## License

MIT OR Apache-2.0
