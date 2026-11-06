//! recqemu - scenario VMs and their serial console.
//!
//! VMs run as libvirt domains ([`libvirt`]), so they can be managed in
//! Cockpit; this crate also finds the OVMF firmware, makes disk images, and
//! drives a guest through its serial console ([`serial`]).

pub mod libvirt;
pub mod serial;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Host address SSH forwards listen on.
pub const HOSTFWD_ADDRESS: &str = "127.0.0.1";

/// The guest's sshd port, which SSH forwards point at.
pub const GUEST_SSH_PORT: u16 = 22;

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
