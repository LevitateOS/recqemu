//! recqemu - QEMU command builder and utilities.
//!
//! The one place QEMU command lines are assembled, for development boots
//! (`cargo xtask scenarios boot`) and the install tests.
//!
//! # Example
//!
//! ```text
//! use recqemu::{QemuBuilder, find_ovmf, create_disk};
//!
//! create_disk(Path::new("disk.qcow2"), "20G")?;
//! let mut cmd = QemuBuilder::new()
//!     .cdrom(iso_path)
//!     .disk(disk_path)
//!     .uefi(find_ovmf()?)
//!     .build();
//! cmd.status()?;
//! ```

pub mod serial;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Guest memory; every VM gets this (there is no setter).
pub const DEFAULT_MEMORY: &str = "4G";

/// Guest CPU count; every VM gets this (there is no setter).
pub const DEFAULT_SMP: u32 = 4;

/// Where the guest's serial port goes.
#[derive(Clone, Debug)]
pub enum Serial {
    /// Multiplexed with the QEMU monitor on stdio.
    Stdio,
    /// Appended to a file.
    File(PathBuf),
}

/// Builder for QEMU commands.
#[derive(Clone)]
pub struct QemuBuilder {
    program: PathBuf,
    cdrom: Option<PathBuf>,
    cdrom_as_disk: bool,
    extra_cdroms: Vec<PathBuf>,
    disk: Option<PathBuf>,
    ovmf_code: Option<PathBuf>,
    ovmf_vars: Option<PathBuf>,
    boot_order: Option<String>,
    nographic: bool,
    serial: Option<Serial>,
    no_reboot: bool,
    enable_kvm: bool,
    user_network: bool,
    user_network_hostfwd: Vec<(u16, u16)>,
    vga: Option<String>,
    display: Option<String>,
    devices: Vec<String>,
    no_monitor: bool,
    qmp_socket: Option<PathBuf>,
    vnc_display: Option<u16>,
    nodefaults: bool,
    fw_cfg_files: Vec<FwCfgFile>,
}

#[derive(Debug, Clone)]
struct FwCfgFile {
    name: String,
    path: PathBuf,
}

/// The host address user-mode networking forwards guest ports on.
pub const HOSTFWD_ADDRESS: &str = "127.0.0.1";

impl Default for QemuBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl QemuBuilder {
    /// A builder for `qemu-system-x86_64`, using KVM when `/dev/kvm` exists.
    pub fn new() -> Self {
        Self {
            program: PathBuf::from("qemu-system-x86_64"),
            cdrom: None,
            cdrom_as_disk: false,
            extra_cdroms: Vec::new(),
            disk: None,
            ovmf_code: None,
            ovmf_vars: None,
            boot_order: None,
            nographic: false,
            serial: None,
            no_reboot: false,
            enable_kvm: kvm_available(),
            user_network: false,
            user_network_hostfwd: Vec::new(),
            vga: None,
            display: None,
            devices: Vec::new(),
            no_monitor: false,
            qmp_socket: None,
            vnc_display: None,
            nodefaults: false,
            fw_cfg_files: Vec::new(),
        }
    }

    /// Run this QEMU binary instead of `qemu-system-x86_64` from PATH.
    pub fn program(mut self, path: impl Into<PathBuf>) -> Self {
        self.program = path.into();
        self
    }

    /// Set ISO for CD-ROM boot (virtio-scsi).
    pub fn cdrom(mut self, path: impl Into<PathBuf>) -> Self {
        self.cdrom = Some(path.into());
        self
    }

    /// Also attach the CD-ROM image as a read-only disk, so the guest kernel
    /// sees the partitions appended to a hybrid ISO (where the LevitateOS
    /// live payloads can live; see `distro_spec::shared::live_boot`).
    pub fn cdrom_as_disk(mut self) -> Self {
        self.cdrom_as_disk = true;
        self
    }

    /// Add an additional read-only CD-ROM image.
    pub fn extra_cdrom(mut self, path: impl Into<PathBuf>) -> Self {
        self.extra_cdroms.push(path.into());
        self
    }

    /// Add a virtio disk; its format (qcow2 or raw) is read from the image.
    pub fn disk(mut self, path: impl Into<PathBuf>) -> Self {
        self.disk = Some(path.into());
        self
    }

    /// Enable UEFI boot with OVMF firmware.
    pub fn uefi(mut self, ovmf_code: impl Into<PathBuf>) -> Self {
        self.ovmf_code = Some(ovmf_code.into());
        self
    }

    /// Set UEFI variable storage (for persistent boot entries).
    pub fn uefi_vars(mut self, ovmf_vars: impl Into<PathBuf>) -> Self {
        self.ovmf_vars = Some(ovmf_vars.into());
        self
    }

    /// Set boot order (e.g., "dc" = cdrom first, then disk).
    pub fn boot_order(mut self, order: &str) -> Self {
        self.boot_order = Some(order.to_string());
        self
    }

    /// Disable graphics, use serial console.
    pub fn nographic(mut self) -> Self {
        self.nographic = true;
        self
    }

    /// Send serial output to stdio (with the monitor).
    pub fn serial_stdio(mut self) -> Self {
        self.serial = Some(Serial::Stdio);
        self
    }

    /// Append serial output to a file.
    pub fn serial_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.serial = Some(Serial::File(path.into()));
        self
    }

    /// Don't reboot on shutdown/crash.
    pub fn no_reboot(mut self) -> Self {
        self.no_reboot = true;
        self
    }

    /// Enable user-mode networking (DHCP, NAT).
    pub fn user_network(mut self) -> Self {
        self.user_network = true;
        self
    }

    /// Enable user networking with a forward from `HOSTFWD_ADDRESS:host_port`
    /// to the guest's TCP `guest_port`.
    pub fn user_network_with_hostfwd(mut self, host_port: u16, guest_port: u16) -> Self {
        self.user_network = true;
        self.user_network_hostfwd.push((host_port, guest_port));
        self
    }

    /// Set VGA adapter type (e.g., "std", "virtio", "none").
    pub fn vga(mut self, vga_type: &str) -> Self {
        self.vga = Some(vga_type.to_string());
        self
    }

    /// Set display type (e.g., "gtk,gl=on", "vnc=:1").
    pub fn display(mut self, display: &str) -> Self {
        self.display = Some(display.to_string());
        self
    }

    /// Add a device (`-device <spec>`).
    pub fn device(mut self, spec: &str) -> Self {
        self.devices.push(spec.to_string());
        self
    }

    pub fn no_monitor(mut self) -> Self {
        self.no_monitor = true;
        self
    }

    /// Set QMP socket path for programmatic control.
    pub fn qmp_socket(mut self, path: impl Into<PathBuf>) -> Self {
        self.qmp_socket = Some(path.into());
        self
    }

    /// Set VNC display number (0 = port 5900).
    pub fn vnc_display(mut self, display: u16) -> Self {
        self.vnc_display = Some(display);
        self
    }

    pub fn nodefaults(mut self) -> Self {
        self.nodefaults = true;
        self
    }

    /// Attach a file via QEMU fw_cfg.
    ///
    /// The guest can read this from:
    /// `/sys/firmware/qemu_fw_cfg/by_name/<name>/raw`
    pub fn fw_cfg_file(mut self, name: &str, path: impl Into<PathBuf>) -> Self {
        self.fw_cfg_files.push(FwCfgFile {
            name: name.to_string(),
            path: path.into(),
        });
        self
    }

    pub fn build(self) -> Command {
        let mut cmd = Command::new(&self.program);

        if self.nodefaults {
            cmd.arg("-nodefaults");
        }
        if self.enable_kvm {
            cmd.args(["-enable-kvm", "-cpu", "host"]);
        } else {
            cmd.args(["-cpu", "qemu64"]);
        }
        cmd.args(["-smp", &DEFAULT_SMP.to_string()]);
        cmd.args(["-m", DEFAULT_MEMORY]);

        // Read-only images on virtio-scsi, one controller each.
        let mut scsi = 0usize;
        let mut attach_scsi = |cmd: &mut Command, kind: &str, file: &Path| {
            cmd.arg("-device")
                .arg(format!("virtio-scsi-pci,id=scsi{scsi}"))
                .arg("-device")
                .arg(format!("{kind},drive=ro{scsi},bus=scsi{scsi}.0"))
                .arg("-drive")
                .arg(format!(
                    "id=ro{scsi},if=none,format=raw,readonly=on,file={}",
                    file.display()
                ));
            scsi += 1;
        };
        if let Some(cdrom) = &self.cdrom {
            attach_scsi(&mut cmd, "scsi-cd", cdrom);
            if self.cdrom_as_disk {
                attach_scsi(&mut cmd, "scsi-hd", cdrom);
            }
        }
        for cdrom in &self.extra_cdroms {
            attach_scsi(&mut cmd, "scsi-cd", cdrom);
        }

        if let Some(disk) = &self.disk {
            cmd.args([
                "-drive",
                &format!(
                    "file={},format={},if=virtio",
                    disk.display(),
                    disk_image_format(disk)
                ),
            ]);
        }

        if let Some(ovmf_code) = &self.ovmf_code {
            cmd.args([
                "-drive",
                &format!(
                    "if=pflash,format=raw,readonly=on,file={}",
                    ovmf_code.display()
                ),
            ]);
        }
        if let Some(ovmf_vars) = &self.ovmf_vars {
            cmd.args([
                "-drive",
                &format!("if=pflash,format=raw,file={}", ovmf_vars.display()),
            ]);
        }

        if let Some(order) = &self.boot_order {
            cmd.args(["-boot", order]);
        }

        if self.user_network {
            let mut netdev = String::from("user,id=net0");
            for (host_port, guest_port) in &self.user_network_hostfwd {
                netdev.push_str(&format!(
                    ",hostfwd=tcp:{HOSTFWD_ADDRESS}:{host_port}-:{guest_port}"
                ));
            }
            cmd.args(["-netdev", &netdev]);
            cmd.args(["-device", "virtio-net-pci,netdev=net0"]);
        }

        if self.nographic {
            cmd.arg("-nographic");
        } else if let Some(display) = &self.display {
            cmd.args(["-display", display]);
        }
        // A headless VM gets no VGA adapter, so QEMU needs no vgabios ROMs.
        match (&self.vga, self.nographic) {
            (Some(vga), _) => {
                cmd.args(["-vga", vga]);
            }
            (None, true) => {
                cmd.args(["-vga", "none"]);
            }
            (None, false) => {}
        }
        for device in &self.devices {
            cmd.args(["-device", device]);
        }
        if let Some(display) = self.vnc_display {
            cmd.args(["-vnc", &format!(":{}", display)]);
        }

        match &self.serial {
            Some(Serial::Stdio) => {
                cmd.args(["-serial", "mon:stdio"]);
            }
            Some(Serial::File(path)) => {
                cmd.args(["-serial", &format!("file:{}", path.display())]);
            }
            None => {}
        }
        if self.no_monitor {
            cmd.args(["-monitor", "none"]);
        }
        if let Some(socket) = &self.qmp_socket {
            cmd.args(["-qmp", &format!("unix:{},server,nowait", socket.display())]);
        }
        for item in &self.fw_cfg_files {
            cmd.args([
                "-fw_cfg",
                &format!("name={},file={}", item.name, item.path.display()),
            ]);
        }
        if self.no_reboot {
            cmd.arg("-no-reboot");
        }

        cmd
    }

    /// Build command with piped stdio (for `Console` serial control).
    pub fn build_piped(self) -> Command {
        let mut cmd = self.build();
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd
    }
}

/// `qcow2` when the image starts with the qcow2 magic, else `raw`.
pub fn disk_image_format(path: &Path) -> &'static str {
    use std::io::Read;
    const QCOW2_MAGIC: [u8; 4] = [0x51, 0x46, 0x49, 0xFB];
    let mut header = [0u8; 4];
    let is_qcow2 = std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && header == QCOW2_MAGIC;
    if is_qcow2 {
        "qcow2"
    } else {
        "raw"
    }
}

/// OVMF candidate directories across distros (Fedora/RHEL, Debian/Ubuntu,
/// Arch, NixOS), each holding a CODE and a VARS image.
const OVMF_FILES: &[(&str, &str)] = &[
    ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "OVMF_VARS.fd"),
    ("/usr/share/OVMF/OVMF_CODE.fd", "OVMF_VARS.fd"),
    ("/usr/share/OVMF/OVMF_CODE_4M.fd", "OVMF_VARS_4M.fd"),
    ("/usr/share/edk2-ovmf/x64/OVMF_CODE.fd", "OVMF_VARS.fd"),
    ("/run/libvirt/nix-ovmf/OVMF_CODE.fd", "OVMF_VARS.fd"),
];

fn env_file(var: &str) -> Result<Option<PathBuf>> {
    match std::env::var_os(var) {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_file() {
                bail!("{var} is set to '{}', which is not a file", path.display());
            }
            Ok(Some(path))
        }
        None => Ok(None),
    }
}

/// OVMF firmware (UEFI code): `OVMF_PATH`, else the first installed
/// distro package location.
pub fn find_ovmf() -> Result<PathBuf> {
    if let Some(path) = env_file("OVMF_PATH")? {
        return Ok(path);
    }
    OVMF_FILES
        .iter()
        .map(|(code, _)| PathBuf::from(code))
        .find(|p| p.is_file())
        .context("OVMF firmware not found; install edk2-ovmf or set OVMF_PATH")
}

/// OVMF variable-store template matching `find_ovmf()`: `OVMF_VARS_PATH`,
/// else the VARS image next to the firmware.
///
/// Copy it before use; the guest writes its boot entries into it.
pub fn find_ovmf_vars() -> Result<PathBuf> {
    if let Some(path) = env_file("OVMF_VARS_PATH")? {
        return Ok(path);
    }
    let code = find_ovmf()?;
    let dir = code.parent().unwrap_or(Path::new("/"));
    let code_name = code.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let paired = OVMF_FILES
        .iter()
        .filter(|(c, _)| c.ends_with(code_name))
        .map(|(_, vars)| *vars)
        .chain(["OVMF_VARS.fd", "OVMF_VARS_4M.fd"]);
    for vars in paired {
        let candidate = dir.join(vars);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!(
        "no OVMF variable store next to {}; set OVMF_VARS_PATH",
        code.display()
    )
}

/// Create a qcow2 VM disk image of `size` (e.g., "20G").
pub fn create_disk(path: &Path, size: &str) -> Result<()> {
    let status = Command::new("qemu-img")
        .args(["create", "-q", "-f", "qcow2"])
        .arg(path)
        .arg(size)
        .status()
        .context("running qemu-img (is QEMU installed?)")?;
    if !status.success() {
        bail!("qemu-img create {} failed with {status}", path.display());
    }
    Ok(())
}

pub fn kvm_available() -> bool {
    Path::new("/dev/kvm").exists()
}
