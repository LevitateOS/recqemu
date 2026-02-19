//! recqemu - QEMU command builder and utilities.
//!
//! Provides shared QEMU infrastructure for both development (`leviso run`)
//! and testing (`install-tests`).
//!
//! # Example
//!
//! ```ignore
//! use recqemu::{QemuBuilder, find_ovmf, create_disk};
//!
//! // Create a disk
//! create_disk(Path::new("disk.qcow2"), "20G")?;
//!
//! // Build QEMU command
//! let mut cmd = QemuBuilder::new()
//!     .memory("4G")
//!     .cdrom(iso_path)
//!     .disk(disk_path)
//!     .uefi(find_ovmf().unwrap())
//!     .build();
//!
//! cmd.status()?;
//! ```

pub mod patterns;
pub mod process;
pub mod serial;

pub use serial::{CommandResult, Console};

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Default memory for VMs (4GB - real systems, not toys).
pub const DEFAULT_MEMORY: &str = "4G";

/// Default CPU count.
pub const DEFAULT_SMP: u32 = 4;

/// Default disk size for new VMs.
pub const DEFAULT_DISK_SIZE: &str = "20G";

/// Builder for QEMU commands.
///
/// Provides a fluent API for constructing QEMU command lines with
/// sensible defaults for UEFI boot testing.
#[derive(Default, Clone)]
pub struct QemuBuilder {
    memory: Option<String>,
    smp: Option<u32>,
    cpu: Option<String>,
    kernel: Option<PathBuf>,
    initrd: Option<PathBuf>,
    append: Option<String>,
    cdrom: Option<PathBuf>,
    disk: Option<PathBuf>,
    ovmf_code: Option<PathBuf>,
    ovmf_vars: Option<PathBuf>,
    boot_order: Option<String>,
    nographic: bool,
    serial_stdio: bool,
    serial_file: Option<PathBuf>,
    no_reboot: bool,
    enable_kvm: bool,
    user_network: bool,
    user_network_hostfwd: Vec<(u16, u16)>,
    vga: Option<String>,
    display: Option<String>,
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

impl QemuBuilder {
    /// Create a new QEMU builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            enable_kvm: Path::new("/dev/kvm").exists(),
            ..Default::default()
        }
    }

    /// Set memory size (e.g., "4G", "2048M").
    pub fn memory(mut self, size: &str) -> Self {
        self.memory = Some(size.to_string());
        self
    }

    /// Set number of CPU cores.
    pub fn smp(mut self, cores: u32) -> Self {
        self.smp = Some(cores);
        self
    }

    /// Set CPU model (e.g., "host", "Skylake-Client").
    pub fn cpu(mut self, model: &str) -> Self {
        self.cpu = Some(model.to_string());
        self
    }

    /// Set kernel for direct boot (bypasses bootloader).
    pub fn kernel(mut self, path: impl Into<PathBuf>) -> Self {
        self.kernel = Some(path.into());
        self
    }

    /// Set initrd for direct boot.
    pub fn initrd(mut self, path: impl Into<PathBuf>) -> Self {
        self.initrd = Some(path.into());
        self
    }

    /// Set kernel command line for direct boot.
    pub fn append(mut self, cmdline: &str) -> Self {
        self.append = Some(cmdline.to_string());
        self
    }

    /// Set ISO for CD-ROM boot (virtio-scsi).
    pub fn cdrom(mut self, path: impl Into<PathBuf>) -> Self {
        self.cdrom = Some(path.into());
        self
    }

    /// Add virtio disk.
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

    /// Send serial output to stdio.
    pub fn serial_stdio(mut self) -> Self {
        self.serial_stdio = true;
        self
    }

    /// Send serial output to file.
    pub fn serial_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.serial_file = Some(path.into());
        self
    }

    /// Don't reboot on shutdown/crash.
    pub fn no_reboot(mut self) -> Self {
        self.no_reboot = true;
        self
    }

    /// Explicitly enable/disable KVM.
    pub fn kvm(mut self, enable: bool) -> Self {
        self.enable_kvm = enable;
        self
    }

    /// Enable user-mode networking (DHCP, NAT).
    pub fn user_network(mut self) -> Self {
        self.user_network = true;
        self
    }

    /// Enable user networking with host TCP forward to guest TCP.
    pub fn user_network_with_hostfwd(mut self, host_port: u16, guest_port: u16) -> Self {
        self.user_network = true;
        self.user_network_hostfwd.push((host_port, guest_port));
        self
    }

    /// Set VGA adapter type (e.g., "std", "virtio").
    pub fn vga(mut self, vga_type: &str) -> Self {
        self.vga = Some(vga_type.to_string());
        self
    }

    /// Set display type (e.g., "gtk,gl=on", "none").
    pub fn display(mut self, display: &str) -> Self {
        self.display = Some(display.to_string());
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

    /// Start with no default devices.
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

    /// Build the QEMU command.
    pub fn build(self) -> Command {
        let mut cmd = Command::new("qemu-system-x86_64");

        // No defaults if requested
        if self.nodefaults {
            cmd.arg("-nodefaults");
        }

        // KVM acceleration
        if self.enable_kvm {
            cmd.arg("-enable-kvm");
        }

        // CPU
        if let Some(cpu) = &self.cpu {
            cmd.args(["-cpu", cpu]);
        } else if self.enable_kvm {
            cmd.args(["-cpu", "host"]);
        } else {
            cmd.args(["-cpu", "qemu64"]);
        }

        // SMP
        let smp = self.smp.unwrap_or(DEFAULT_SMP);
        cmd.args(["-smp", &smp.to_string()]);

        // Memory
        let memory = self.memory.as_deref().unwrap_or(DEFAULT_MEMORY);
        cmd.args(["-m", memory]);

        // Direct kernel boot
        if let Some(kernel) = &self.kernel {
            cmd.args(["-kernel", &kernel.to_string_lossy()]);
        }
        if let Some(initrd) = &self.initrd {
            cmd.args(["-initrd", &initrd.to_string_lossy()]);
        }
        if let Some(append) = &self.append {
            cmd.args(["-append", append]);
        }

        // CD-ROM via virtio-scsi
        if let Some(cdrom) = &self.cdrom {
            cmd.args([
                "-device",
                "virtio-scsi-pci,id=scsi0",
                "-device",
                "scsi-cd,drive=cdrom0,bus=scsi0.0",
                "-drive",
                &format!(
                    "id=cdrom0,if=none,format=raw,readonly=on,file={}",
                    cdrom.display()
                ),
                "-device",
                "virtio-scsi-pci,id=scsi1",
                "-device",
                "scsi-hd,drive=cdparts0,bus=scsi1.0",
                "-drive",
                &format!(
                    "id=cdparts0,if=none,format=raw,readonly=on,file={}",
                    cdrom.display()
                ),
            ]);
        }

        // Virtio disk
        if let Some(disk) = &self.disk {
            cmd.args([
                "-drive",
                &format!("file={},format=qcow2,if=virtio", disk.display()),
            ]);
        }

        // UEFI firmware
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

        // Boot order
        if let Some(order) = &self.boot_order {
            cmd.args(["-boot", order]);
        }

        // User-mode networking
        if self.user_network {
            let mut netdev = String::from("user,id=net0");
            for (host_port, guest_port) in &self.user_network_hostfwd {
                netdev.push_str(&format!(
                    ",hostfwd=tcp:127.0.0.1:{}-:{}",
                    host_port, guest_port
                ));
            }
            cmd.args(["-netdev", &netdev]);
            cmd.args(["-device", "virtio-net-pci,netdev=net0"]);
        }

        // Display
        if self.nographic {
            cmd.arg("-nographic");
            // Disable VGA when headless to avoid needing vgabios ROM files
            if self.vga.is_none() {
                cmd.args(["-vga", "none"]);
            }
        } else if let Some(display) = &self.display {
            cmd.args(["-display", display]);
        }

        if let Some(vga) = &self.vga {
            cmd.args(["-vga", vga]);
        }

        // VNC
        if let Some(display) = self.vnc_display {
            cmd.args(["-vnc", &format!(":{}", display)]);
        }

        // Serial
        if self.serial_stdio {
            cmd.args(["-serial", "mon:stdio"]);
        } else if let Some(path) = &self.serial_file {
            cmd.args(["-serial", &format!("file:{}", path.display())]);
        }

        // QMP socket
        if let Some(socket) = &self.qmp_socket {
            cmd.args(["-qmp", &format!("unix:{},server,nowait", socket.display())]);
        }

        // fw_cfg file injection
        for item in &self.fw_cfg_files {
            cmd.args([
                "-fw_cfg",
                &format!("name={},file={}", item.name, item.path.display()),
            ]);
        }

        // No reboot
        if self.no_reboot {
            cmd.arg("-no-reboot");
        }

        cmd
    }

    /// Build command with piped stdio (for serial console control).
    pub fn build_piped(self) -> Command {
        let mut cmd = self.build();
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd
    }

    /// Build command with inherited stdio (for interactive use).
    pub fn build_interactive(self) -> Command {
        let mut cmd = self.build();
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        cmd
    }
}

/// Find OVMF firmware (UEFI code) on the system.
///
/// Searches common locations across distros.
pub fn find_ovmf() -> Option<PathBuf> {
    // Check OVMF_PATH env var first (set by recipe when OVMF is extracted to .tools/)
    if let Ok(path) = std::env::var("OVMF_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }

    let candidates = [
        // Fedora/RHEL
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_CODE.fd",
        // Debian/Ubuntu
        "/usr/share/OVMF/OVMF_CODE_4M.fd",
        "/usr/share/qemu/OVMF.fd",
        // Arch
        "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
        // NixOS
        "/run/libvirt/nix-ovmf/OVMF_CODE.fd",
    ];

    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Find OVMF variable storage template.
///
/// This file is copied and used as writable storage for UEFI boot entries.
pub fn find_ovmf_vars() -> Option<PathBuf> {
    let candidates = [
        // Fedora/RHEL
        "/usr/share/edk2/ovmf/OVMF_VARS.fd",
        "/usr/share/OVMF/OVMF_VARS.fd",
        // Debian/Ubuntu
        "/usr/share/OVMF/OVMF_VARS_4M.fd",
        "/usr/share/qemu/OVMF_VARS.fd",
        // Arch
        "/usr/share/edk2-ovmf/x64/OVMF_VARS.fd",
        // NixOS
        "/run/libvirt/nix-ovmf/OVMF_VARS.fd",
    ];

    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Create a qcow2 disk image.
///
/// # Arguments
///
/// * `path` - Path for the new disk image
/// * `size` - Size string (e.g., "20G", "1024M")
pub fn create_disk(path: &Path, size: &str) -> Result<()> {
    let status = Command::new("qemu-img")
        .args(["create", "-f", "qcow2"])
        .arg(path)
        .arg(size)
        .status()
        .context("Failed to run qemu-img. Is QEMU installed?")?;

    if !status.success() {
        bail!("qemu-img create failed with status: {}", status);
    }

    Ok(())
}

/// Check if KVM is available on this system.
pub fn kvm_available() -> bool {
    Path::new("/dev/kvm").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let cmd = QemuBuilder::new().build();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Should have memory and SMP
        assert!(args.iter().any(|a| a == "-m"));
        assert!(args.iter().any(|a| a == "-smp"));
    }

    #[test]
    fn test_builder_cdrom() {
        let cmd = QemuBuilder::new().cdrom("/tmp/test.iso").build();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Should have virtio-scsi
        assert!(args.iter().any(|a| a == "virtio-scsi-pci,id=scsi0"));
    }

    #[test]
    fn test_builder_fw_cfg_file() {
        let cmd = QemuBuilder::new()
            .fw_cfg_file("opt/test/payload", "/tmp/payload.env")
            .build();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args
            .iter()
            .any(|a| a == "name=opt/test/payload,file=/tmp/payload.env"));
    }

    #[test]
    fn test_find_ovmf_returns_existing_path() {
        // If OVMF is found, path should exist
        if let Some(path) = find_ovmf() {
            assert!(path.exists());
        }
    }
}
