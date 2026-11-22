//! Partitioning, formatting and mounting the target disk, plus the checks
//! that guard them and the unmount that runs when an install fails.

use crate::config::Config;
use crate::disks::{self, partition_path};
use crate::plan::{Action, Firmware, Planner, Stage, TARGET, host, t};
use crate::run;
use anyhow::{Context, Result, bail};
use std::io::{BufRead, Write};
use std::path::Path;

pub const ESP_MOUNT: &str = "/boot/efi";

/// Exists only on the archiso live system.
pub const LIVE_MARKER: &str = "/run/archiso";

pub(crate) fn steps(p: &mut Planner, cfg: &Config, fw: Firmware) {
    let dev = cfg.disk.device.as_str();
    let p1 = partition_path(dev, 1);
    let p2 = partition_path(dev, 2);

    p.stage(Stage::Partition)
        .add("Wipe old filesystem signatures", host(&["wipefs", "-a", dev]))
        .add("Clear old partition tables", host(&["sgdisk", "--zap-all", dev]));

    match fw {
        Firmware::Uefi => p.add(
            "GPT: 512M EFI system partition, rest is root",
            host(&["sgdisk", "-n", "1:0:+512M", "-t", "1:ef00", "-c", "1:EFI",
                   "-n", "2:0:0", "-t", "2:8300", "-c", "2:arctix", dev]),
        ),
        Firmware::Bios => p.add(
            "GPT: 1M bios_grub partition, rest is root",
            host(&["sgdisk", "-n", "1:0:+1M", "-t", "1:ef02", "-c", "1:bios_grub",
                   "-n", "2:0:0", "-t", "2:8300", "-c", "2:arctix", dev]),
        ),
    };

    p.add("Tell the kernel about the new partitions", host(&["partprobe", dev]))
        .add("Let udev create the partition device nodes", host(&["udevadm", "settle"]))
        .add("Make sure both partitions showed up", Action::WaitForPaths { paths: vec![p1.clone(), p2.clone()] })
        // New partitions can start where old filesystems lived; clear any leftovers
        // so blkid (and genfstab later) never sees two signatures.
        .add("Wipe leftover signatures inside the new partitions", host(&["wipefs", "-a", &p1, &p2]));
    if fw == Firmware::Uefi {
        p.add("Format the ESP as FAT32", host(&["mkfs.fat", "-F", "32", "-n", "ARCTIX_EFI", &p1]));
    }
    p.add("Format root as ext4", host(&["mkfs.ext4", "-F", "-L", "arctix", &p2]))
        .add("Let udev pick up the new filesystems", host(&["udevadm", "settle"]))
        .add("Mount root", host(&["mount", &p2, TARGET]));
    if fw == Firmware::Uefi {
        p.add(
            "Mount the ESP",
            host(&["mount", "--mkdir", "-o", "fmask=0077,dmask=0077", &p1, &t(ESP_MOUNT)]),
        );
    }
}

/// Runs right before anything destructive. Every check here is cheap and
/// re-done at execution time, so nothing from planning is trusted.
pub fn check_ready(device: &str) -> Result<()> {
    if !is_root()? {
        bail!("the installer needs root, run it with sudo");
    }
    if !Path::new(LIVE_MARKER).exists() {
        bail!("{LIVE_MARKER} is missing, so this is not the Arctix live ISO. Refusing to touch any disk.");
    }
    let busy = target_mounts()?;
    if !busy.is_empty() {
        bail!("{TARGET} is already in use ({}), unmount it first", busy.join(", "));
    }
    let devices = disks::lsblk()?;
    let live = disks::live_medium_disk()?;
    disks::check_target(&devices, device, live.as_deref())?;
    Ok(())
}

/// The user has to type the device path back. Anything else aborts.
pub fn confirm(device: &str) -> Result<()> {
    let devices = disks::lsblk()?;
    let dev = disks::find(&devices, device).with_context(|| format!("{device} disappeared"))?;
    let model = dev.model.as_deref().map(str::trim).filter(|m| !m.is_empty()).unwrap_or("unknown model");

    println!();
    println!("  This ERASES everything on {device} ({}, {model}).", disks::human_size(dev.size));
    print!("  Type {device} to continue: ");
    std::io::stdout().flush()?;

    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).context("could not read your answer")?;
    if answer.trim() != device {
        bail!("you typed {:?}, not {device}. Nothing was touched", answer.trim());
    }
    run::log(&format!("user confirmed wiping {device}"));
    Ok(())
}

pub fn is_root() -> Result<bool> {
    let status = std::fs::read_to_string("/proc/self/status").context("could not read /proc/self/status")?;
    Ok(effective_uid(&status) == Some(0))
}

/// Effective uid from /proc/self/status ("Uid: real effective saved fs").
fn effective_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|uid| uid.parse().ok())
}

pub fn target_mounts() -> Result<Vec<String>> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").context("could not read /proc/self/mounts")?;
    Ok(mounts_under(&mounts, TARGET))
}

/// Mountpoints at or below `root`, given the contents of /proc/self/mounts.
fn mounts_under(mounts: &str, root: &str) -> Vec<String> {
    let prefix = format!("{root}/");
    mounts
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .map(unescape_mount)
        .filter(|m| m == root || m.starts_with(&prefix))
        .collect()
}

/// /proc/self/mounts escapes space, tab, newline and backslash as octal.
fn unescape_mount(s: &str) -> String {
    s.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\")
}

/// Best effort cleanup after a failed install: unmount everything under the target.
pub fn unmount_target() {
    match target_mounts() {
        Ok(m) if m.is_empty() => {}
        Ok(_) => {
            eprintln!("cleaning up: unmounting {TARGET}");
            if let Err(e) = run::capture("umount", &["-R", TARGET]) {
                eprintln!("warning: could not unmount {TARGET}: {e:#}");
            }
        }
        Err(e) => eprintln!("warning: could not check mounts: {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;

    #[test]
    fn finds_mounts_under_target_only() {
        let mounts = "\
/dev/vda2 /mnt ext4 rw 0 0
/dev/vda1 /mnt/boot/efi vfat rw 0 0
/dev/vdb1 /mnt2 ext4 rw 0 0
/dev/vdc1 /mnt/my\\040disk ext4 rw 0 0
proc /proc proc rw 0 0
";
        assert_eq!(mounts_under(mounts, "/mnt"), ["/mnt", "/mnt/boot/efi", "/mnt/my disk"]);
        assert!(mounts_under("proc /proc proc rw 0 0\n", "/mnt").is_empty());
    }

    #[test]
    fn reads_effective_uid() {
        assert_eq!(effective_uid("Name:\tx\nUid:\t1000\t0\t0\t0\nGid:\t1\n"), Some(0));
        assert_eq!(effective_uid("Uid:\t1000\t1000\t1000\t1000\n"), Some(1000));
        assert_eq!(effective_uid("nothing here"), None);
    }

    #[test]
    fn partition_steps_wait_and_wipe_before_mkfs() {
        let cfg = Config::parse(include_str!("../examples/arctix.toml")).unwrap();
        let steps = plan::build(&cfg, Firmware::Uefi);
        let lines: Vec<String> = steps.iter().map(|s| s.action.to_string()).collect();
        let pos = |needle: &str| lines.iter().position(|l| l.contains(needle)).unwrap();
        assert!(pos("udevadm settle") < pos("wait until /dev/vda1 and /dev/vda2 exist"));
        assert!(pos("wait until") < pos("wipefs -a /dev/vda1 /dev/vda2"));
        assert!(pos("wipefs -a /dev/vda1 /dev/vda2") < pos("mkfs.fat"));
    }

    #[test]
    fn stages_parse_and_order() {
        for st in Stage::ALL {
            assert_eq!(Stage::parse(st.key()), Some(st));
        }
        assert!(Stage::Preflight < Stage::Partition && Stage::Bootloader < Stage::Finish);
        assert_eq!(Stage::parse("nope"), None);
    }
}
