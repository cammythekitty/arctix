//! Machine-readable helpers for the graphical installer:
//! `options` (what can be chosen, with defaults) and `validate` (check a
//! config with exactly the rules the install uses). Both print JSON.

use crate::config::Config;
use crate::configure::{self, check_keymap, check_locale, check_timezone, walk};
use crate::disks::{self, DiskInfo};
use crate::partition;
use crate::plan::{Firmware, LIVE_USER};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Options {
    /// True on the live ISO, where installing is allowed.
    pub live: bool,
    pub firmware: String,
    pub disks: Vec<DiskInfo>,
    pub defaults: Defaults,
    pub timezones: Vec<String>,
    pub keymaps: Vec<String>,
    pub locales: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Defaults {
    pub hostname: String,
    pub timezone: String,
    pub keymap: String,
    pub locale: String,
}

pub fn gather() -> Result<Options> {
    let devices = disks::lsblk()?;
    let live_disk = disks::live_medium_disk()?;
    let root = Path::new("/");
    Ok(Options {
        live: Path::new(partition::LIVE_MARKER).exists(),
        firmware: Firmware::detect().to_string(),
        disks: disks::disk_infos(&devices, live_disk.as_deref()),
        defaults: Defaults {
            hostname: "arctix".into(),
            timezone: live_timezone(),
            keymap: live_keymap(),
            locale: "en_US.UTF-8".into(),
        },
        timezones: timezones(root),
        keymaps: keymaps(root),
        locales: locales(root),
    })
}

/// The live system's timezone, e.g. "UTC".
pub fn live_timezone() -> String {
    fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| p.to_str().and_then(|s| s.split_once("zoneinfo/").map(|(_, tz)| tz.to_string())))
        .unwrap_or_else(|| "UTC".into())
}

/// The live desktop's keyboard layout as a console keymap, if it exists; else "us".
pub fn live_keymap() -> String {
    let path = format!("/home/{LIVE_USER}/{}", configure::HYPR_INPUT_CONFIG);
    fs::read_to_string(path)
        .ok()
        .and_then(|text| configure::lua_kb_layout(&text))
        .map(|xkb| configure::console_from_xkb(&xkb))
        .filter(|keymap| check_keymap(Path::new("/"), keymap).is_ok())
        .unwrap_or_else(|| "us".into())
}

/// Zone names from zone.tab and zone1970.tab (third column), plus UTC.
/// zone1970.tab alone merges zones (Europe/Copenhagen is folded into
/// Europe/Berlin), and people look for their own city.
fn timezones(root: &Path) -> Vec<String> {
    let mut zones = BTreeSet::new();
    for tab in ["zone.tab", "zone1970.tab"] {
        let text = fs::read_to_string(root.join("usr/share/zoneinfo").join(tab)).unwrap_or_default();
        zones.extend(parse_zone_tab(&text));
    }
    zones.insert("UTC".into());
    zones.into_iter().collect()
}

fn parse_zone_tab(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split('\t').nth(2))
        .map(str::to_string)
        .collect()
}

/// Console keymap names (file names under /usr/share/kbd/keymaps).
fn keymaps(root: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    let _ = walk(&root.join("usr/share/kbd/keymaps"), &mut |path, _| {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(base) = name.strip_suffix(".map.gz").or_else(|| name.strip_suffix(".map")) {
                names.insert(base.to_string());
            }
        }
        Ok(())
    });
    names.into_iter().collect()
}

/// UTF-8 locales glibc can generate.
fn locales(root: &Path) -> Vec<String> {
    let text = fs::read_to_string(root.join("usr/share/i18n/SUPPORTED")).unwrap_or_default();
    parse_supported(&text)
}

fn parse_supported(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(name, charset)| charset.trim() == "UTF-8" && name.ends_with(".UTF-8"))
        .map(|(name, _)| name.to_string())
        .collect()
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Validation {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// Check a config the way the install will: shape, values, and that the
/// timezone, locale and keymap exist under `root`.
pub fn validate(text: &str, root: &Path) -> Validation {
    let cfg = match Config::parse_unvalidated(text) {
        Ok(cfg) => cfg,
        Err(e) => return Validation { ok: false, errors: vec![format!("{e:#}")] },
    };
    let mut errors = cfg.problems();
    let sys = &cfg.system;
    for check in [check_timezone(root, &sys.timezone), check_locale(root, &sys.locale), check_keymap(root, &sys.keymap)] {
        if let Err(e) = check {
            errors.push(e.to_string());
        }
    }
    Validation { ok: errors.is_empty(), errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_zone_tab_and_supported() {
        let tab = "# comment\nDK\t+5540+01235\tEurope/Copenhagen\nSE\t+5920+01803\tEurope/Stockholm\n";
        assert_eq!(parse_zone_tab(tab).into_iter().collect::<Vec<_>>(), ["Europe/Copenhagen", "Europe/Stockholm"]);
        let supported = "da_DK.UTF-8 UTF-8\nda_DK ISO-8859-1\nen_US.UTF-8 UTF-8\nen_US ISO-8859-1\n";
        assert_eq!(parse_supported(supported), ["da_DK.UTF-8", "en_US.UTF-8"]);
    }

    #[test]
    fn validate_reports_every_problem_as_its_own_message() {
        let root = Path::new("/nonexistent-root");
        let bad = "[disk]\ndevice = \"vda\"\n[user]\nname = \"Root\"\n";
        let v = validate(bad, root);
        assert!(!v.ok);
        assert!(v.errors.iter().any(|e| e.contains("disk.device")));
        assert!(v.errors.iter().any(|e| e.contains("user.name")));
        assert!(v.errors.iter().any(|e| e.contains("timezone")));
        let broken = validate("[disk", root);
        assert!(!broken.ok && broken.errors.len() == 1);
    }

    #[test]
    fn validate_accepts_a_good_config_on_a_matching_root() {
        let root = std::env::temp_dir().join(format!("arctix-opts-{}", std::process::id()));
        for f in [
            "usr/share/zoneinfo/UTC",
            "usr/share/i18n/locales/en_US",
            "usr/share/i18n/charmaps/UTF-8.gz",
            "usr/share/kbd/keymaps/i386/qwerty/us.map.gz",
        ] {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "").unwrap();
        }
        let good = "[disk]\ndevice = \"/dev/vda\"\n[system]\ntimezone = \"UTC\"\nkeymap = \"us\"\n[user]\nname = \"cammy\"\n";
        assert_eq!(validate(good, &root), Validation { ok: true, errors: vec![] });
        assert_eq!(keymaps(&root), ["us"]);
        fs::remove_dir_all(root).unwrap();
    }
}
