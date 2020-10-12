/*!
 * swdl: Raspberry Pi firmware update engine.
 * partition management utilities.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

// allow unused stuff on x86_64, which won't use a lot of the code here
#![cfg_attr(target_arch = "x86_64", allow(dead_code))]
#![cfg_attr(target_arch = "x86_64", allow(unused_imports))]

use anyhow::{anyhow, Context, Result};

use nimage::format::PartType;

const ROOTFS_DEVS: [&str; 2] = ["/dev/mmcblk0p2", "/dev/mmcblk0p3"];

pub fn get_cmdline() -> std::io::Result<String> {
    std::fs::read_to_string("/proc/cmdline")
}

pub fn get_active_rootfs(cmdline: &str) -> Option<&str> {
    for word in cmdline.split_ascii_whitespace() {
        // strip_prefix is new in Rust 1.45, returns Some(remaining) if a prefix
        // was stripped, otherwise None.
        if let Some(s) = word.strip_prefix("root=") {
            return Some(s);
        }
    }
    None
}

pub fn get_inactive_rootfs(cmdline: &str) -> Option<&'static str> {
    let active = get_active_rootfs(cmdline)?;
    for (i, dev) in ROOTFS_DEVS.iter().enumerate() {
        if active == *dev {
            return Some(ROOTFS_DEVS[(i + 1) % ROOTFS_DEVS.len()]);
        }
    }
    None
}

/// Get the destination path for a raw PartType
#[cfg(not(target_arch = "x86_64"))]
pub fn raw_dest_path(ptype: PartType) -> Result<&'static str> {
    const NOT_FOUND_MSG: &str =
        "failed to get inactive rootfs. root= in /proc/cmdline is missing or unrecognized";

    match ptype {
        PartType::BootImg => Ok("/dev/mmcblk0p1"),
        PartType::Rootfs | PartType::RootfsRw => {
            let cmdline = get_cmdline().with_context(|| "failed to get kernel cmdline")?;
            get_inactive_rootfs(&cmdline).ok_or_else(|| anyhow!(NOT_FOUND_MSG))
        }
        PartType::BootTar | PartType::Invalid => {
            Err(anyhow!("Part type {} is not a raw partition"))
        }
    }
}

/// Get the destination path for a raw PartType
/// on x86, always write to /dev/null
#[cfg(target_arch = "x86_64")]
pub fn raw_dest_path(ptype: PartType) -> Result<&'static str> {
    match ptype {
        PartType::BootImg | PartType::Rootfs | PartType::RootfsRw => Ok("/dev/null"),
        PartType::BootTar | PartType::Invalid => {
            Err(anyhow!("Part type {} is not a raw partition"))
        }
    }
}

#[allow(dead_code)] // FIXME
pub fn update_rootfs(cmdline: &str, new_rootfs: &str, rw: bool) -> String {
    let new_rootfs_word = format!("root={}", new_rootfs);
    let mut set_root = false;
    let mut set_rw = false;
    let mut new: Vec<&str> = Vec::new();

    for word in cmdline.split_ascii_whitespace() {
        if word.starts_with("root=") {
            if !set_root {
                new.push(&new_rootfs_word);
                set_root = true;
            }
        } else if word == "ro" || word == "rw" {
            if !set_rw {
                new.push(if rw { "rw" } else { "ro" });
                set_rw = true;
            }
        } else {
            new.push(word);
        }
    }

    if !set_root {
        new.push(&new_rootfs_word);
    }
    if !set_rw {
        new.push(if rw { "rw" } else { "ro" });
    }

    new.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // an actual /proc/cmdline on my Pi4 (macaddr redacted)
    const LONG_CMDLINE: &'static str = "\
            coherent_pool=1M 8250.nr_uarts=1 cma=64M bcm2708_fb.fbwidth=0 bcm2708_fb.fbheight=0 \
            bcm2708_fb.fbswap=1 smsc95xx.macaddr=DE:AD:BE:EF:12:34 vc_mem.mem_base=0x3ec00000 \
            vc_mem.mem_size=0x40000000  dwc_otg.lpm_enable=0 console=ttyAMA0,115200 \
            root=/dev/mmcblk0p3 ro rootwait";

    #[test]
    fn test_active_rootfs() {
        assert_eq!(get_active_rootfs(LONG_CMDLINE), Some("/dev/mmcblk0p3"));
        assert_eq!(get_active_rootfs("root=foobar"), Some("foobar"));
        assert_eq!(get_active_rootfs(""), None);
    }

    #[test]
    fn test_inactive_rootfs() {
        assert_eq!(get_inactive_rootfs(LONG_CMDLINE), Some("/dev/mmcblk0p2"));
        assert_eq!(
            get_inactive_rootfs("foo bar root=/dev/mmcblk0p2 ro asdf=asdf"),
            Some("/dev/mmcblk0p3")
        );
        assert_eq!(get_inactive_rootfs(""), None);
        assert_eq!(get_inactive_rootfs("test root=/dev/sda1 rw"), None);
    }

    #[test]
    fn test_update_rootfs() {
        let cmdline = "console=tty0 root=/dev/mmcblk0p2 ro rootwait";
        assert_eq!(
            update_rootfs(cmdline, "/dev/mmcblk0p3", true),
            "console=tty0 root=/dev/mmcblk0p3 rw rootwait"
        );
        assert_eq!(
            update_rootfs(cmdline, "/dev/sda1", false),
            "console=tty0 root=/dev/sda1 ro rootwait"
        );
        assert_eq!(
            update_rootfs("console=tty0 root=/dev/sda1 rootwait", "/dev/sdb1", false),
            "console=tty0 root=/dev/sdb1 rootwait ro"
        );
        assert_eq!(update_rootfs("", "/dev/mmcblk0p2", true), "root=/dev/mmcblk0p2 rw");
    }
}
