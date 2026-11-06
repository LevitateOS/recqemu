# recqemu

Scenario VMs as libvirt domains, and the serial console that drives them.

Every VM the scenarios and `cargo xtask run` boot is a transient libvirt
domain on the user's session (`qemu:///session`, or `LEVITATE_LIBVIRT_URI`),
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
    uefi_code: recqemu::find_ovmf()?,
    uefi_vars: UefiVars::Fresh { template: recqemu::find_ovmf_vars()?, path: vars_path },
    disk: Some(disk),                 // virtio, qcow2 or raw (detected)
    boot_iso: Some(iso),              // booted first: a CD and a read-only disk
    extra_cdroms: vec![],             // CDs that are not booted
    fw_cfg: vec![],                   // ("opt/...", file) entries
    ssh_forward: Some(port),          // 127.0.0.1:port -> guest 22 (passt)
    serial: SerialTarget::Socket(socket.clone()),
    serial_log: log,                  // every serial line, appended
};
let domain = Domain::create_paused(&spec)?;   // paused: no output is missed
let mut console = Console::connect(&socket, Duration::from_secs(20))?;
domain.resume()?;
// ... console.wait_for_boot(...), console.login(...), console.exec(...)
// Dropping `domain` destroys it; a guest reboot or power-off ends it too.
```

Each domain has 4 GiB, 4 CPUs (KVM when `/dev/kvm` exists, else TCG), a
q35 machine with UEFI (OVMF), user networking through passt, a VNC display on
127.0.0.1 for Cockpit, and `on_reboot`/`on_poweroff` set to destroy, so a
guest reboot ends the domain and the caller starts the next boot.
`SerialTarget::Pty` instead gives a pty for `virsh console`
(`cargo xtask run`), and `Domain::keep_running` leaves a domain running after
its handle is dropped.

### Utilities

```rust
let ovmf = find_ovmf()?;          // OVMF_PATH, else distro package paths
let vars = find_ovmf_vars()?;     // OVMF_VARS_PATH, else the VARS file next to it
create_disk(Path::new("disk.qcow2"), "20G")?;   // qemu-img
```

## OVMF Search Paths

`OVMF_PATH` (and `OVMF_VARS_PATH`) when set (a path that is not a file is an
error), else the Fedora, Debian/Ubuntu, Arch and NixOS package locations in
`OVMF_FILES` in `src/lib.rs`.

## Serial test protocol

`serial::protocol` and `guest/serial-test.sh` are the two sides of it: the
guest script (installed as a profile.d script by the live overlays) turns echo
off on the serial console, empties the prompt and prints `SHELL_READY_MARKER`;
`Console::exec` wraps each command in unique `printf`-printed markers and reads
its exit status, and `Console::login` drives getty and confirms the shell
with a marker probe.

## Requirements

- libvirt with the QEMU driver: `sudo dnf install libvirt-daemon-kvm libvirt-client`
  (the session daemon starts on demand)
- passt: `sudo dnf install passt`
- OVMF: `sudo dnf install edk2-ovmf`
- qemu-img (for `create_disk`)
- Cockpit with `cockpit-machines`, to manage the VMs in a browser (optional)

`cargo xtask doctor` checks these.

## Consumers

- `xtask` - `cargo xtask run`
- `install-tests` - the scenarios

## License

MIT OR Apache-2.0
