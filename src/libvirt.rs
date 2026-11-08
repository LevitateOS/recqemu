//! Scenario VMs as libvirt domains, so they can be watched and managed in
//! Cockpit (Virtual Machines) or with `virsh`.
//!
//! Each VM is a transient domain on the user's libvirt session
//! (`qemu:///session`, [`URI`]): libvirt
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

/// The libvirt connection every scenario VM runs on: the user's session.
pub const URI: &str = "qemu:///session";

/// The libvirt connection scenario VMs use ([`URI`]).
pub fn uri() -> &'static str {
    URI
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
    /// A fresh copy of `template` at `path`, which libvirt makes when the
    /// domain starts (replacing what was there) and which stays afterwards.
    Fresh { template: PathBuf, path: PathBuf },
}

/// What a scenario VM consists of.
#[derive(Clone, Debug)]
pub struct DomainSpec {
    /// Domain name shown in Cockpit and `virsh list`, e.g. `levitate-install`.
    pub name: String,
    /// One line describing the VM, shown in Cockpit.
    pub title: String,
    pub memory_mib: u32,
    pub vcpus: u32,
    /// UEFI firmware (OVMF code).
    pub uefi_code: PathBuf,
    pub uefi_vars: UefiVars,
    /// The writable system disk, booted second when an ISO is attached.
    pub disk: Option<PathBuf>,
    /// An ISO booted first, as a CD.
    pub boot_iso: Option<PathBuf>,
    /// Images attached as CDs that are not booted.
    pub extra_cdroms: Vec<PathBuf>,
    /// Host port forwarded to the guest's SSH, on [`HOSTFWD_ADDRESS`].
    pub ssh_forward: Option<u16>,
    pub serial: SerialTarget,
    /// Everything the serial port prints, appended here.
    pub serial_log: PathBuf,
}

impl DomainSpec {
    /// Guest memory in MiB and CPU count when the caller has no reason to differ.
    pub const DEFAULT_MEMORY_MIB: u32 = 4096;
    pub const DEFAULT_VCPUS: u32 = 4;
    /// Guest memory for a desktop system: a whole desktop session, and on a
    /// live boot a tmpfs of half the memory for its writes (docs/desktop.md:
    /// the prototypes ran in 4 GiB with no room to spare).
    pub const DESKTOP_MEMORY_MIB: u32 = 6144;

    /// The libvirt domain XML.
    pub fn xml(&self) -> Result<String> {
        let mut x = String::new();
        let _ = writeln!(x, "<domain type='{}'>", if kvm_available() { "kvm" } else { "qemu" });
        let _ = writeln!(x, "  <name>{}</name>", esc(&self.name));
        let _ = writeln!(x, "  <title>{}</title>", esc(&self.title));
        let _ = writeln!(x, "  <memory unit='MiB'>{}</memory>", self.memory_mib);
        let _ = writeln!(x, "  <vcpu>{}</vcpu>", self.vcpus);
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
        }
        for cdrom in &self.extra_cdroms {
            scsi(&mut x, "cdrom", cdrom, None);
        }
        if let Some(disk) = &self.disk {
            let _ = writeln!(x, "    <disk type='file' device='disk'>");
            let _ = writeln!(x, "      <driver name='qemu' type='{}'/>", disk_image_format(disk)?);
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
        Ok(x)
    }
}

/// A running transient domain; dropping it destroys the domain.
pub struct Domain {
    name: String,
    /// Leave the domain running when this handle is dropped.
    keep: bool,
}

impl Domain {
    /// Create the domain paused, so whoever reads the serial port can attach
    /// before the guest prints anything; [`resume`](Self::resume) starts it.
    /// A running domain of the same name (another run's, or one left in
    /// Cockpit) is an error, not something to kill.
    pub fn create_paused(spec: &DomainSpec) -> Result<Self> {
        if domain_exists(&spec.name)? {
            let info = virsh(URI, &["dominfo", &spec.name])?;
            if info.lines().any(|l| l.starts_with("Persistent:") && l.contains("yes")) {
                bail!(
                    "a defined libvirt domain is called '{}'; scenario VMs are transient and need the name (virsh -c {URI} undefine {})",
                    spec.name,
                    spec.name
                );
            }
            bail!(
                "libvirt domain '{}' is already running (another run of it, or one left in Cockpit); stop it first: virsh -c {URI} destroy {}",
                spec.name,
                spec.name
            );
        }
        if let SerialTarget::Socket(path) = &spec.serial {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
            }
            remove_if_present(path)?;
        }
        if let Some(parent) = spec.serial_log.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
        }
        if let UefiVars::Fresh { path, .. } = &spec.uefi_vars {
            remove_if_present(path)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
            }
        }
        let xml_file = tempfile::Builder::new()
            .prefix(&format!("{}-", spec.name))
            .suffix(".xml")
            .tempfile()
            .context("writing the domain XML")?;
        std::fs::write(xml_file.path(), spec.xml()?)?;
        virsh(URI, &["create", "--paused", &xml_file.path().to_string_lossy()])
            .with_context(|| format!("starting libvirt domain '{}' on {URI}", spec.name))?;
        Ok(Self {
            name: spec.name.clone(),
            keep: false,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The libvirt connection the domain runs on.
    pub fn uri(&self) -> &str {
        URI
    }

    pub fn resume(&self) -> Result<()> {
        virsh(URI, &["resume", &self.name]).map(|_| ())
    }

    /// The domain still exists (running or paused).
    pub fn is_running(&self) -> Result<bool> {
        domain_exists(&self.name)
    }

    /// Wait for the guest to end the domain (a reboot or power-off).
    pub fn wait_for_shutdown(&self, timeout: Duration) -> Result<()> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !self.is_running()? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("libvirt domain '{}' did not shut down within {}s", self.name, timeout.as_secs())
    }

    /// Stop the domain now.
    pub fn destroy(&self) -> Result<()> {
        if self.is_running()? {
            virsh(URI, &["destroy", &self.name])?;
        }
        Ok(())
    }

    /// Let go of the domain without stopping it: it keeps running (for
    /// Cockpit) until the guest powers off or someone destroys it.
    pub fn keep_running(mut self) {
        self.keep = true;
    }
}

impl Drop for Domain {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Err(err) = self.destroy() {
            eprintln!(
                "warning: could not stop libvirt domain '{}' ({err:#}); stop it with: virsh -c {URI} destroy {}",
                self.name, self.name
            );
        }
    }
}

/// Whether domain `name` exists on [`URI`]; an error when libvirt cannot say.
fn domain_exists(name: &str) -> Result<bool> {
    let out = Command::new("virsh")
        .args(["-c", URI, "domstate", name])
        .stdin(Stdio::null())
        .output()
        .context("running virsh (is libvirt installed?)")?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("failed to get domain") || stderr.contains("Domain not found") {
        return Ok(false);
    }
    bail!("virsh -c {URI} domstate {name} failed: {}", stderr.trim())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(err).with_context(|| format!("removing '{}'", path.display()))
        }
        _ => Ok(()),
    }
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

/// Per-user runtime directory for sockets, serial transcripts and temporary
/// UEFI variable stores (unix socket paths must stay short).
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("scenario-vm")
}

/// Whether VMs run under KVM; without `/dev/kvm` libvirt runs them under
/// TCG emulation, several times slower.
pub fn kvm_available() -> bool {
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
