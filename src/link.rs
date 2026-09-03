//! Taking a network interface down.
//!
//! NetworkManager leaves an interface administratively up when it releases it,
//! and an idle-but-up radio still listens. Down is what the device would be
//! after a reboot with nothing managing it, so a disable ends by putting it
//! there. When the device is enabled again NetworkManager brings it back up.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use anyhow::{Context, Result, bail};

pub fn set_down(interface: &str) -> Result<()> {
    let name = interface.as_bytes();
    if name.is_empty() || name.len() >= libc::IFNAMSIZ {
        bail!("{interface:?} is not an interface name");
    }
    let mut req: libc::ifreq = unsafe { std::mem::zeroed() };
    for (dst, src) in req.ifr_name.iter_mut().zip(name) {
        *dst = *src as libc::c_char;
    }

    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("opening a socket for ioctl");
    }
    let sock = unsafe { OwnedFd::from_raw_fd(fd) };

    if unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCGIFFLAGS, &mut req) } < 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("reading the flags of {interface}"));
    }
    unsafe { req.ifr_ifru.ifru_flags &= !(libc::IFF_UP as libc::c_short) };
    if unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFFLAGS, &req) } < 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("taking {interface} down"));
    }
    Ok(())
}
