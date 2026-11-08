//! recqemu - scenario VMs and their serial console.
//!
//! VMs run as libvirt domains ([`libvirt`]), so they can be managed in
//! Cockpit; this crate also finds the OVMF firmware, makes disk images, and
//! drives a guest through its serial console ([`serial`]).

pub mod guest;
pub mod libvirt;
pub mod serial;

pub use guest::{shell_quote, Guest, Outcome};

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Host address SSH forwards listen on.
pub const HOSTFWD_ADDRESS: &str = "127.0.0.1";

/// The guest's sshd port, which SSH forwards point at.
pub const GUEST_SSH_PORT: u16 = 22;

/// `qcow2` when the image starts with the qcow2 magic, else `raw`; an image
/// that cannot be read is an error.
pub fn disk_image_format(path: &Path) -> Result<&'static str> {
    use std::io::Read;
    const QCOW2_MAGIC: [u8; 4] = [0x51, 0x46, 0x49, 0xFB];
    let mut header = [0u8; 4];
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening disk image '{}'", path.display()))?;
    match file.read_exact(&mut header) {
        Ok(()) if header == QCOW2_MAGIC => Ok("qcow2"),
        Ok(()) => Ok("raw"),
        // Shorter than the magic: an empty or tiny raw image.
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok("raw"),
        Err(err) => Err(err).with_context(|| format!("reading disk image '{}'", path.display())),
    }
}

/// The host's OVMF builds, as (CODE image, its VARS template), across
/// distros (Fedora/RHEL, Debian/Ubuntu, Arch, NixOS). libvirt's QEMU runs
/// every VM, so its host's firmware is the one that matches.
const OVMF_FILES: &[(&str, &str)] = &[
    ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"),
    ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
    ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
    ("/usr/share/edk2-ovmf/x64/OVMF_CODE.fd", "/usr/share/edk2-ovmf/x64/OVMF_VARS.fd"),
    ("/run/libvirt/nix-ovmf/OVMF_CODE.fd", "/run/libvirt/nix-ovmf/OVMF_VARS.fd"),
];

fn host_ovmf() -> Result<(PathBuf, PathBuf)> {
    OVMF_FILES
        .iter()
        .map(|(code, vars)| (PathBuf::from(code), PathBuf::from(vars)))
        .find(|(code, vars)| code.is_file() && vars.is_file())
        .context("no OVMF firmware with its variable store on this host; install edk2-ovmf (or ovmf)")
}

/// The host's OVMF firmware (UEFI code).
pub fn find_ovmf() -> Result<PathBuf> {
    host_ovmf().map(|(code, _)| code)
}

/// The variable-store template that pairs with [`find_ovmf`]. Copy it before
/// use; the guest writes its boot entries into it.
pub fn find_ovmf_vars() -> Result<PathBuf> {
    host_ovmf().map(|(_, vars)| vars)
}

/// Create a qcow2 VM disk image of `size` (e.g., "20G").
pub fn create_disk(path: &Path, size: &str) -> Result<()> {
    qemu_img_create(&[], path, Some(size))
}

/// Create a qcow2 image at `path` that reads through to `backing` (also
/// qcow2) and keeps every write itself, so `backing` is never changed.
pub fn create_overlay(backing: &Path, path: &Path) -> Result<()> {
    let backing = std::fs::canonicalize(backing)
        .with_context(|| format!("resolving the overlay's backing image '{}'", backing.display()))?;
    qemu_img_create(&["-F".as_ref(), "qcow2".as_ref(), "-b".as_ref(), backing.as_os_str()], path, None)
}

fn qemu_img_create(options: &[&std::ffi::OsStr], path: &Path, size: Option<&str>) -> Result<()> {
    let status = Command::new("qemu-img")
        .args(["create", "-q", "-f", "qcow2"])
        .args(options)
        .arg(path)
        .args(size)
        .status()
        .context("running qemu-img (is QEMU installed?)")?;
    if !status.success() {
        bail!("qemu-img create {} failed with {status}", path.display());
    }
    Ok(())
}
