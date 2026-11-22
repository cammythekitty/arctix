//! Disk discovery (from `lsblk -J -b`, never the human readable output) and
//! the safety checks that decide whether a disk may be wiped.

use crate::run;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use std::path::Path;

/// Smallest disk we will install to. The unpacked image plus room to update.
pub const MIN_DISK_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Where archiso mounts the boot medium on the live system.
pub const LIVE_BOOT_MOUNT: &str = "/run/archiso/bootmnt";

const LSBLK_COLUMNS: &str = "PATH,SIZE,TYPE,RO,RM,TRAN,MODEL,MOUNTPOINTS";

#[derive(Debug, Clone, Deserialize)]
pub struct BlockDevice {
    pub path: String,
    #[serde(deserialize_with = "de_u64")]
    pub size: u64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(deserialize_with = "de_bool")]
    pub ro: bool,
    #[serde(deserialize_with = "de_bool")]
    pub rm: bool,
    #[serde(default)]
    pub tran: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, deserialize_with = "de_mountpoints")]
    pub mountpoints: Vec<String>,
    #[serde(default)]
    pub children: Vec<BlockDevice>,
}

#[derive(Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<BlockDevice>,
}

impl BlockDevice {
    pub fn is_disk(&self) -> bool {
        self.kind == "disk"
    }

    /// Mountpoints of this device and everything on it (partitions, LVM, ...).
    /// Swap shows up as "[SWAP]", which counts as in use too.
    pub fn mountpoints_recursive(&self) -> Vec<String> {
        let mut out = self.mountpoints.clone();
        for child in &self.children {
            out.extend(child.mountpoints_recursive());
        }
        out
    }
}

pub fn parse_lsblk(json: &str) -> Result<Vec<BlockDevice>> {
    let out: LsblkOutput = serde_json::from_str(json).context("could not parse lsblk JSON")?;
    Ok(out.blockdevices)
}

pub fn lsblk() -> Result<Vec<BlockDevice>> {
    parse_lsblk(&run::capture("lsblk", &["-J", "-b", "-o", LSBLK_COLUMNS])?)
}

/// Partition `n` of `device`. Kernel naming rule: if the disk name ends in a
/// digit (nvme0n1, mmcblk0, loop0) a `p` goes in between, otherwise not (sda, vda).
pub fn partition_path(device: &str, n: u32) -> String {
    if device.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{device}p{n}")
    } else {
        format!("{device}{n}")
    }
}

/// The whole disk the live medium was booted from, if it is a local block device.
pub fn live_medium_disk() -> Result<Option<String>> {
    if !Path::new(LIVE_BOOT_MOUNT).exists() {
        return Ok(None);
    }
    let source = run::capture("findmnt", &["-n", "-o", "SOURCE", "--target", LIVE_BOOT_MOUNT])?;
    let source = source.trim();
    if !source.starts_with("/dev/") {
        // Not mounted any more (copytoram) or netbooted: nothing local to protect.
        return Ok(None);
    }
    let parent = run::capture("lsblk", &["-n", "-d", "-o", "PKNAME", source])?;
    let parent = parent.trim();
    Ok(Some(if parent.is_empty() { source.to_string() } else { format!("/dev/{parent}") }))
}

pub fn find<'a>(devices: &'a [BlockDevice], path: &str) -> Option<&'a BlockDevice> {
    devices
        .iter()
        .find_map(|d| if d.path == path { Some(d) } else { find(&d.children, path) })
}

/// Everything that must be true before `device` may be wiped.
pub fn check_target<'a>(
    devices: &'a [BlockDevice],
    device: &str,
    live_disk: Option<&str>,
) -> Result<&'a BlockDevice> {
    let Some(dev) = find(devices, device) else {
        bail!("{device} not found (see `arctix-install list-disks`)");
    };
    if !dev.is_disk() {
        bail!("{device} is a {}, not a whole disk", dev.kind);
    }
    if live_disk == Some(device) {
        bail!("{device} is the disk Arctix is running from");
    }
    if dev.ro {
        bail!("{device} is read-only");
    }
    let mounted = dev.mountpoints_recursive();
    if !mounted.is_empty() {
        bail!("{device} is in use (mounted at {}), unmount it first", mounted.join(", "));
    }
    if dev.size < MIN_DISK_BYTES {
        bail!(
            "{device} is too small ({}, need at least {})",
            human_size(dev.size),
            human_size(MIN_DISK_BYTES)
        );
    }
    Ok(dev)
}

/// What `list-disks` prints, one entry per whole disk. Meant for the UI as well.
#[derive(Debug, Serialize)]
pub struct DiskInfo {
    pub path: String,
    pub size: u64,
    pub size_human: String,
    pub model: Option<String>,
    pub transport: Option<String>,
    pub removable: bool,
    pub read_only: bool,
    pub mountpoints: Vec<String>,
    pub live_medium: bool,
    pub usable: bool,
    pub reason: Option<String>,
}

pub fn disk_infos(devices: &[BlockDevice], live_disk: Option<&str>) -> Vec<DiskInfo> {
    devices
        .iter()
        .filter(|d| d.is_disk())
        .map(|d| {
            let reason = check_target(devices, &d.path, live_disk).err().map(|e| e.to_string());
            DiskInfo {
                path: d.path.clone(),
                size: d.size,
                size_human: human_size(d.size),
                model: d.model.as_ref().map(|m| m.trim().to_string()),
                transport: d.tran.clone(),
                removable: d.rm,
                read_only: d.ro,
                mountpoints: d.mountpoints_recursive(),
                live_medium: live_disk == Some(d.path.as_str()),
                usable: reason.is_none(),
                reason,
            }
        })
        .collect()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

// lsblk has changed its JSON types between util-linux versions (numbers vs
// strings, true vs "1"), so these accept both.

fn de_u64<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => n.as_u64().ok_or_else(|| D::Error::custom("size is not a u64")),
        Value::String(s) => s.parse().map_err(D::Error::custom),
        Value::Null => Ok(0),
        other => Err(D::Error::custom(format!("unexpected size {other}"))),
    }
}

fn de_bool<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    match Value::deserialize(d)? {
        Value::Bool(b) => Ok(b),
        Value::Number(n) => Ok(n.as_u64() != Some(0)),
        Value::String(s) => match s.as_str() {
            "1" | "true" => Ok(true),
            "0" | "false" | "" => Ok(false),
            _ => Err(D::Error::custom(format!("unexpected flag {s:?}"))),
        },
        Value::Null => Ok(false),
        other => Err(D::Error::custom(format!("unexpected flag {other}"))),
    }
}

fn de_mountpoints<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<String>, D::Error> {
    let raw: Option<Vec<Option<String>>> = Option::deserialize(d)?;
    Ok(raw.unwrap_or_default().into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn sample() -> Vec<BlockDevice> {
        let json = format!(
            r#"{{"blockdevices": [
            {{"name":"sr0","path":"/dev/sr0","size":1200000000,"type":"rom","ro":false,"rm":true,
              "tran":"sata","model":"QEMU DVD-ROM","mountpoints":["/run/archiso/bootmnt"]}},
            {{"name":"vda","path":"/dev/vda","size":{big},"type":"disk","ro":false,"rm":false,
              "tran":null,"model":null,"mountpoints":[null]}},
            {{"name":"sda","path":"/dev/sda","size":"{big}","type":"disk","ro":"0","rm":"1",
              "tran":"usb","model":"Stick  ","mountpoints":[null],
              "children":[{{"name":"sda1","path":"/dev/sda1","size":{big},"type":"part",
                            "ro":false,"rm":true,"mountpoints":["/run/media/x"]}}]}},
            {{"name":"nvme0n1","path":"/dev/nvme0n1","size":{big},"type":"disk","ro":false,"rm":false,
              "mountpoints":null,
              "children":[{{"name":"nvme0n1p1","path":"/dev/nvme0n1p1","size":{big},"type":"part",
                            "ro":false,"rm":false,"mountpoints":[null]}}]}},
            {{"name":"vdb","path":"/dev/vdb","size":{small},"type":"disk","ro":false,"rm":false,"mountpoints":[null]}},
            {{"name":"vdc","path":"/dev/vdc","size":{big},"type":"disk","ro":true,"rm":false,"mountpoints":[null]}}
        ]}}"#,
            big = 20 * GIB,
            small = 2 * GIB
        );
        parse_lsblk(&json).unwrap()
    }

    #[test]
    fn partition_naming() {
        assert_eq!(partition_path("/dev/sda", 1), "/dev/sda1");
        assert_eq!(partition_path("/dev/vda", 2), "/dev/vda2");
        assert_eq!(partition_path("/dev/nvme0n1", 1), "/dev/nvme0n1p1");
        assert_eq!(partition_path("/dev/nvme1n12", 2), "/dev/nvme1n12p2");
        assert_eq!(partition_path("/dev/mmcblk0", 1), "/dev/mmcblk0p1");
        assert_eq!(partition_path("/dev/loop0", 3), "/dev/loop0p3");
    }

    #[test]
    fn parses_both_lsblk_json_styles() {
        let devs = sample();
        let sda = find(&devs, "/dev/sda").unwrap();
        assert_eq!(sda.size, 20 * GIB);
        assert!(sda.rm && !sda.ro);
        let vda = find(&devs, "/dev/vda").unwrap();
        assert!(vda.mountpoints.is_empty());
        assert!(find(&devs, "/dev/nvme0n1").unwrap().mountpoints.is_empty());
    }

    #[test]
    fn blank_disk_passes() {
        let devs = sample();
        assert!(check_target(&devs, "/dev/vda", Some("/dev/sr0")).is_ok());
        assert!(check_target(&devs, "/dev/nvme0n1", None).is_ok());
    }

    #[test]
    fn refuses_unsafe_targets() {
        let devs = sample();
        let err = |dev: &str, live: Option<&str>| check_target(&devs, dev, live).unwrap_err().to_string();
        assert!(err("/dev/sda", None).contains("mounted at /run/media/x"));
        assert!(err("/dev/nvme0n1p1", None).contains("not a whole disk"));
        assert!(err("/dev/sr0", None).contains("not a whole disk"));
        assert!(err("/dev/vda", Some("/dev/vda")).contains("running from"));
        assert!(err("/dev/vdb", None).contains("too small"));
        assert!(err("/dev/vdc", None).contains("read-only"));
        assert!(err("/dev/sdz", None).contains("not found"));
    }

    #[test]
    fn disk_infos_lists_only_whole_disks() {
        let devs = sample();
        let infos = disk_infos(&devs, Some("/dev/sr0"));
        let paths: Vec<_> = infos.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["/dev/vda", "/dev/sda", "/dev/nvme0n1", "/dev/vdb", "/dev/vdc"]);
        assert!(infos[0].usable && infos[0].reason.is_none());
        assert!(!infos[1].usable);
        assert_eq!(infos[1].model.as_deref(), Some("Stick"));
    }

    #[test]
    fn sizes_read_nicely() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(20 * GIB), "20.0 GiB");
    }
}
