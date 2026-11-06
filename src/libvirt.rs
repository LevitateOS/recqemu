//! Scenario VMs as libvirt domains, so they can be watched and managed in
//! Cockpit (Virtual Machines) or with `virsh`.
//!
//! Each VM is a transient domain on the user's libvirt session
//! (`qemu:///session` unless `LEVITATE_LIBVIRT_URI` says otherwise): libvirt
//! runs QEMU as the user, so images under the user's home need no
//! relabelling, and user-mode networking (passt) forwards the guest's SSH to
//! localhost. A guest reboot or power-off ends the domain, as `-no-reboot`
//! did. The serial port goes to a unix socket the caller connects to (or a
//! pty for `virsh console`), with a transcript in a log file; the screen is
//! a VNC display Cockpit shows.

use anyhow::{bail, Context, Result};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{disk_image_format, GUEST_SSH_PORT, HOSTFWD_ADDRESS};

/// Guest memory in MiB.
const MEMORY_MIB: u32 = 4096;
/// Guest CPU count.
const VCPUS: u32 = 4;

/// The libvirt connection scenario VMs use.
pub fn uri() -> String {
    std::env::var("LEVITATE_LIBVIRT_URI").unwrap_or_else(|_| "qemu:///session".to_string())
}

/// Where the guest's serial port goes.
#[derive(Clone, Debug)]
pub enum SerialTarget {
    /// A unix socket the caller connects to (see `Console::connect`).
    Socket(PathBuf),
    /// A pty, for `virsh console`.
    Pty,
}

/// The UEFI variable store.
#[derive(Clone, Debug)]
pub enum UefiVars {
    /// An existing store the guest keeps writing (an installed system's).
    Existing(PathBuf),
    /// A fresh copy of `template` at `path`, removed when the domain ends.
    Fresh { template: PathBuf, path: PathBuf },
}

/// What a scenario VM consists of.
#[derive(Clone, Debug)]
pub struct DomainSpec {
    /// Domain name shown in Cockpit and `virsh list`, e.g. `levitate-install`.
    pub name: String,
    /// One line describing the VM, shown in Cockpit.
    pub title: String,
    /// UEFI firmware (OVMF code).
    pub uefi_code: PathBuf,
    pub uefi_vars: UefiVars,
    /// The writable system disk, booted second when an ISO is attached.
    pub disk: Option<PathBuf>,
    /// An ISO booted first, as a CD and also as a read-only disk (so the
    /// live init sees the ISO's partitions).
    pub boot_iso: Option<PathBuf>,
    /// Images attached as CDs that are not booted.
    pub extra_cdroms: Vec<PathBuf>,
    /// `fw_cfg` entries (`opt/...` names) and the files they carry.
    pub fw_cfg: Vec<(String, PathBuf)>,
    /// Host port forwarded to the guest's SSH, on [`HOSTFWD_ADDRESS`].
    pub ssh_forward: Option<u16>,
    pub serial: SerialTarget,
    /// Everything the serial port prints, appended here.
    pub serial_log: PathBuf,
}

impl DomainSpec {
    /// The libvirt domain XML.
    pub fn xml(&self) -> String {
        let mut x = String::new();
        let _ = writeln!(x, "<domain type='{}'>", if kvm_available() { "kvm" } else { "qemu" });
        let _ = writeln!(x, "  <name>{}</name>", esc(&self.name));
        let _ = writeln!(x, "  <title>{}</title>", esc(&self.title));
        let _ = writeln!(x, "  <memory unit='MiB'>{MEMORY_MIB}</memory>");
        let _ = writeln!(x, "  <vcpu>{VCPUS}</vcpu>");
        let _ = writeln!(x, "  <os>");
        let _ = writeln!(x, "    <type arch='x86_64' machine='q35'>hvm</type>");
        let _ = writeln!(
            x,
            "    <loader readonly='yes' secure='no' type='pflash' format='raw'>{}</loader>",
            esc_path(&self.uefi_code)
        );
        match &self.uefi_vars {
            UefiVars::Existing(path) => {
                let _ = writeln!(x, "    <nvram format='raw'>{}</nvram>", esc_path(path));
            }
            UefiVars::Fresh { template, path } => {
                let _ = writeln!(
                    x,
                    "    <nvram template='{}' format='raw'>{}</nvram>",
                    esc_path(template),
                    esc_path(path)
                );
            }
        }
        let _ = writeln!(x, "  </os>");
        let _ = writeln!(x, "  <features><acpi/><apic/><smm state='on'/></features>");
        if kvm_available() {
            let _ = writeln!(x, "  <cpu mode='host-passthrough'/>");
        }
        if !self.fw_cfg.is_empty() {
            let _ = writeln!(x, "  <sysinfo type='fwcfg'>");
            for (name, file) in &self.fw_cfg {
                let _ = writeln!(x, "    <entry name='{}' file='{}'/>", esc(name), esc_path(file));
            }
            let _ = writeln!(x, "  </sysinfo>");
        }
        // A guest reboot ends the domain; the caller starts the next boot.
        let _ = writeln!(x, "  <on_poweroff>destroy</on_poweroff>");
        let _ = writeln!(x, "  <on_reboot>destroy</on_reboot>");
        let _ = writeln!(x, "  <on_crash>destroy</on_crash>");
        let _ = writeln!(x, "  <devices>");
        let _ = writeln!(x, "    <controller type='scsi' model='virtio-scsi'/>");

        let mut scsi_unit = 0;
        let mut scsi = |x: &mut String, device: &str, file: &Path, boot: Option<u32>| {
            let dev = format!("sd{}", (b'a' + scsi_unit as u8) as char);
            let _ = writeln!(x, "    <disk type='file' device='{device}'>");
            let _ = writeln!(x, "      <driver name='qemu' type='raw'/>");
            let _ = writeln!(x, "      <source file='{}'/>", esc_path(file));
            let _ = writeln!(x, "      <target dev='{dev}' bus='scsi'/>");
            let _ = writeln!(x, "      <readonly/>");
            if let Some(order) = boot {
                let _ = writeln!(x, "      <boot order='{order}'/>");
            }
            let _ = writeln!(x, "    </disk>");
            scsi_unit += 1;
        };
        let mut boot_order = 1;
        if let Some(iso) = &self.boot_iso {
            scsi(&mut x, "cdrom", iso, Some(boot_order));
            boot_order += 1;
            scsi(&mut x, "disk", iso, None);
        }
        for cdrom in &self.extra_cdroms {
            scsi(&mut x, "cdrom", cdrom, None);
        }
        if let Some(disk) = &self.disk {
            let _ = writeln!(x, "    <disk type='file' device='disk'>");
            let _ = writeln!(x, "      <driver name='qemu' type='{}'/>", disk_image_format(disk));
            let _ = writeln!(x, "      <source file='{}'/>", esc_path(disk));
            let _ = writeln!(x, "      <target dev='vda' bus='virtio'/>");
            let _ = writeln!(x, "      <boot order='{boot_order}'/>");
            let _ = writeln!(x, "    </disk>");
        }

        let _ = writeln!(x, "    <interface type='user'>");
        let _ = writeln!(x, "      <backend type='passt'/>");
        let _ = writeln!(x, "      <model type='virtio'/>");
        if let Some(port) = self.ssh_forward {
            let _ = writeln!(x, "      <portForward proto='tcp' address='{HOSTFWD_ADDRESS}'>");
            let _ = writeln!(x, "        <range start='{port}' to='{GUEST_SSH_PORT}'/>");
            let _ = writeln!(x, "      </portForward>");
        }
        let _ = writeln!(x, "    </interface>");

        let log = format!("<log file='{}' append='on'/>", esc_path(&self.serial_log));
        match &self.serial {
            SerialTarget::Socket(path) => {
                let _ = writeln!(x, "    <serial type='unix'>");
                let _ = writeln!(x, "      <source mode='bind' path='{}'/>", esc_path(path));
                let _ = writeln!(x, "      {log}");
                let _ = writeln!(x, "      <target port='0'/>");
                let _ = writeln!(x, "    </serial>");
            }
            SerialTarget::Pty => {
                let _ = writeln!(x, "    <serial type='pty'>");
                let _ = writeln!(x, "      {log}");
                let _ = writeln!(x, "      <target port='0'/>");
                let _ = writeln!(x, "    </serial>");
            }
        }
        let _ = writeln!(x, "    <graphics type='vnc' port='-1' autoport='yes' listen='127.0.0.1'/>");
        let _ = writeln!(x, "    <video><model type='virtio'/></video>");
        let _ = writeln!(x, "  </devices>");
        let _ = writeln!(x, "</domain>");
        x
    }
}

/// A running transient domain; dropping it destroys the domain.
pub struct Domain {
    name: String,
    uri: String,
    fresh_vars: Option<PathBuf>,
    /// Leave the domain running when this handle is dropped.
    keep: bool,
}

impl Domain {
    /// Create the domain paused, so whoever reads the serial port can attach
    /// before the guest prints anything; [`resume`](Self::resume) starts it.
    /// A domain of the same name left by an earlier run is destroyed first.
    pub fn create_paused(spec: &DomainSpec) -> Result<Self> {
        let uri = uri();
        if domain_exists(&uri, &spec.name) {
            let persistent = virsh(&uri, &["dominfo", &spec.name])
                .map(|info| info.lines().any(|l| l.starts_with("Persistent:") && l.contains("yes")))
                .unwrap_or(false);
            if persistent {
                bail!(
                    "a defined libvirt domain is called '{}'; scenario VMs are transient and need the name (virsh -c {uri} undefine {})",
                    spec.name,
                    spec.name
                );
            }
            let _ = virsh(&uri, &["destroy", &spec.name]);
        }
        if let SerialTarget::Socket(path) = &spec.serial {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
            }
            let _ = std::fs::remove_file(path);
        }
        if let Some(parent) = spec.serial_log.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
        }
        let fresh_vars = match &spec.uefi_vars {
            UefiVars::Fresh { path, .. } => {
                let _ = std::fs::remove_file(path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                Some(path.clone())
            }
            UefiVars::Existing(_) => None,
        };
        let xml_file = tempfile::Builder::new()
            .prefix(&format!("{}-", spec.name))
            .suffix(".xml")
            .tempfile()
            .context("writing the domain XML")?;
        std::fs::write(xml_file.path(), spec.xml())?;
        virsh(&uri, &["create", "--paused", &xml_file.path().to_string_lossy()])
            .with_context(|| format!("starting libvirt domain '{}' on {uri}", spec.name))?;
        Ok(Self {
            name: spec.name.clone(),
            uri,
            fresh_vars,
            keep: false,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The libvirt connection the domain runs on.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn resume(&self) -> Result<()> {
        virsh(&self.uri, &["resume", &self.name]).map(|_| ())
    }

    /// The domain still exists (running or paused).
    pub fn is_running(&self) -> bool {
        domain_exists(&self.uri, &self.name)
    }

    /// Wait for the guest to end the domain (a reboot or power-off).
    pub fn wait_for_shutdown(&self, timeout: Duration) -> Result<()> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("libvirt domain '{}' did not shut down within {}s", self.name, timeout.as_secs())
    }

    /// Stop the domain now.
    pub fn destroy(&self) {
        if self.is_running() {
            let _ = virsh(&self.uri, &["destroy", &self.name]);
        }
    }

    /// Let go of the domain without stopping it: it keeps running (for
    /// Cockpit) until the guest powers off or someone destroys it.
    pub fn keep_running(mut self) {
        self.keep = true;
    }

    /// Where the VNC display listens, as `virsh domdisplay` reports it.
    pub fn display(&self) -> Option<String> {
        virsh(&self.uri, &["domdisplay", &self.name])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

impl Drop for Domain {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        self.destroy();
        if let Some(vars) = &self.fresh_vars {
            let _ = std::fs::remove_file(vars);
        }
    }
}

fn domain_exists(uri: &str, name: &str) -> bool {
    Command::new("virsh")
        .args(["-c", uri, "domstate", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Run `virsh -c uri args...` and return its output.
pub fn virsh(uri: &str, args: &[&str]) -> Result<String> {
    let out = Command::new("virsh")
        .arg("-c")
        .arg(uri)
        .args(args)
        .output()
        .context("running virsh (is libvirt installed?)")?;
    if !out.status.success() {
        bail!(
            "virsh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Per-user runtime directory for sockets and fresh UEFI variable stores
/// (unix socket paths must stay short).
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("levitate-vm")
}

fn kvm_available() -> bool {
    Path::new("/dev/kvm").exists()
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

fn esc_path(path: &Path) -> String {
    esc(&path.to_string_lossy())
}
