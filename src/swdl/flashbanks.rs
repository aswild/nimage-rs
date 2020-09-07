/*!
 * swdl: Raspberry Pi firmware update engine.
 * partition management utilities.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

// FIXME: remove this when we actually start using this code
#![allow(dead_code)]

const ROOTFS_DEVS: [&str; 2] = ["/dev/mmcblk0p2", "/dev/mmcblk0p3"];

#[cfg(not(target_arch = "x86_64"))]
pub fn get_cmdline() -> std::io::Result<String> {
    std::fs::read_to_string("/proc/cmdline")
}

#[cfg(target_arch = "x86_64")]
pub fn get_cmdline() -> std::io::Result<String> {
    // Example of what /proc/cmdline looks like on a Pi4. Used for testing on x86
    Ok("coherent_pool=1M 8250.nr_uarts=1 snd_bcm2835.enable_compat_alsa=0 \
        snd_bcm2835.enable_hdmi=1 snd_bcm2835.enable_headphones=1 bcm2708_fb.fbwidth=0 \
        bcm2708_fb.fbheight=0 bcm2708_fb.fbswap=1 smsc95xx.macaddr=DE:AD:BE:EF:12:34 \
        vc_mem.mem_base=0x3ec00000 vc_mem.mem_size=0x40000000 dwc_otg.lpm_enable=0 \
        console=ttyAMA0,115200 root=/dev/mmcblk0p2 ro rootwait"
        .to_owned())
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
