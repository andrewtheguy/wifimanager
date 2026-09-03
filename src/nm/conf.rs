//! The drop-in that keeps a device disabled across reboots.
//!
//! NetworkManager's `Managed` property is runtime state: a restart or a reboot
//! hands the device back. A `[device-*]` section with `managed=0` in
//! `/etc/NetworkManager/conf.d` is what makes the choice stick, and it is only
//! read when the device appears — at boot, on replug, or on a reload of the
//! configuration for a device that is not yet present — so the property is
//! still what switches a live device.

use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};

const DIR: &str = "/etc/NetworkManager/conf.d";

pub fn dropin_path(interface: &str) -> PathBuf {
    PathBuf::from(DIR).join(format!("90-wifimanager-{interface}.conf"))
}

pub fn dropin_text(interface: &str) -> String {
    format!(
        "# Written by wifimanager: this Wi-Fi device is disabled. Press e in\n\
         # wifimanager to enable it again, or delete this file and reload.\n\
         [device-wifimanager-{interface}]\n\
         match-device=interface-name:{interface}\n\
         managed=0\n"
    )
}

pub fn is_disabled(interface: &str) -> bool {
    dropin_path(interface).exists()
}

pub fn write_disabled(interface: &str) -> Result<()> {
    let path = dropin_path(interface);
    std::fs::write(&path, dropin_text(interface))
        .with_context(|| format!("writing {}", path.display()))
}

pub fn remove_disabled(interface: &str) -> Result<()> {
    let path = dropin_path(interface);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dropin_names_the_interface_and_unmanages_it() {
        assert_eq!(
            dropin_path("wlp2s0").to_str().unwrap(),
            "/etc/NetworkManager/conf.d/90-wifimanager-wlp2s0.conf"
        );
        let text = dropin_text("wlp2s0");
        assert!(text.contains("[device-wifimanager-wlp2s0]\n"));
        assert!(text.contains("match-device=interface-name:wlp2s0\n"));
        assert!(text.ends_with("managed=0\n"));
    }
}
