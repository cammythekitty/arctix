//! Strip live-ISO-only config from the target so the installed system boots
//! like a normal install: no autologin, no live services, no passwordless sudo.

use crate::configure::walk;
use crate::plan::{Action, Planner, Stage, chroot, remove, t};
use crate::run;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Units from the live profile that must not survive onto the installed system.
/// Removing them from /etc/systemd/system only drops live-only units and enablement
/// links; units shipped by packages in /usr/lib stay installed, just disabled.
pub const LIVE_UNITS: &[&str] = &[
    "choose-mirror.service",
    "livecd-talk.service",
    "livecd-alsa-unmuter.service",
    "pacman-init.service",
    // A tmpfs over the keyring on the live system. Must go before pacman-key --init.
    "etc-pacman.d-gnupg.mount",
    "reflector.service",
    // Enabled on the live ISO for the dev loop. An installed desktop should not
    // expose sshd by default; users can enable it themselves.
    "sshd.service",
];

/// Live-only files and directories (from releng/airootfs) removed from the target.
pub const LIVE_PATHS: &[(&str, &str)] = &[
    ("/etc/systemd/system/getty@tty1.service.d", "tty1 autologin"),
    ("/etc/systemd/system/cloud-init.target.wants", "cloud-init enablement"),
    ("/etc/systemd/journald.conf.d/volatile-storage.conf", "the RAM-only journal setting"),
    ("/etc/systemd/logind.conf.d/do-not-suspend.conf", "the no-suspend / ignore-lid override"),
    ("/etc/systemd/resolved.conf.d/archiso.conf", "archiso resolved drop-in"),
    ("/etc/machine-info", "ISO pretty hostname"),
    ("/etc/motd", "live MOTD"),
    ("/etc/pacman.d/hooks/uncomment-mirrors.hook", "ISO build hook"),
    ("/etc/pacman.d/hooks/zzzz99-remove-custom-hooks-from-airootfs.hook", "ISO build hook"),
    ("/root/.automated_script.sh", "live root autorun script"),
    ("/root/.zlogin", "live root login hook"),
    ("/usr/local/bin/choose-mirror", "live helper"),
    ("/usr/local/bin/livecd-sound", "live helper"),
    ("/usr/local/bin/Installation_guide", "live helper"),
    ("/usr/local/share/livecd-sound", "live helper data"),
    // The installed system must not offer the installer.
    ("/usr/local/bin/arctix-install", "the installer"),
    ("/usr/local/share/applications/arctix-install.desktop", "the installer launcher"),
    ("/home/arctix/.config/quickshell/ii/installer.qml", "the graphical installer"),
];

/// The live image pins the installer in ii's dock and launcher. This exact line
/// is removed from the live user's ii config on the target.
pub const II_CONFIG: &str = "/home/arctix/.config/illogical-impulse/config.json";
pub const INSTALLER_PIN_LINE: &str = "\"arctix-install\",";

pub(crate) fn steps(p: &mut Planner) {
    p.stage(Stage::Cleanup);
    for (path, what) in LIVE_PATHS {
        p.add(&format!("Remove {what}"), remove(path, true));
    }
    let units_dir = t("/etc/systemd/system");
    for unit in LIVE_UNITS {
        p.add(&format!("Disable live unit {unit}"), Action::RemoveUnit { dir: units_dir.clone(), name: unit.to_string() });
    }
    p.add(
        "Unpin the installer from the dock and launcher",
        Action::RemoveLines { path: t(II_CONFIG), line: INSTALLER_PIN_LINE.into() },
    );
    p.add("Remove NOPASSWD sudo rules", Action::RemoveNopasswdSudoers { dir: t("/etc/sudoers.d") })
        // The live image ships a usable root password. The installed system gets
        // a locked root; the user has sudo instead.
        .add("Lock the root account", chroot(&["usermod", "-p", "!*", "root"]));
}

/// `rm -f` / `rm -rf` that never follows symlinks. Missing paths are fine.
pub fn remove_path(path: &str, recursive: bool) -> Result<bool> {
    let Ok(meta) = fs::symlink_metadata(path) else { return Ok(false) };
    if meta.is_dir() {
        if recursive {
            fs::remove_dir_all(path)
        } else {
            fs::remove_dir(path)
        }
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("removing {path}"))?;
    run::log(&format!("removed {path}"));
    Ok(true)
}

pub fn remove_matching(dir: &str, prefix: &str) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return Ok(removed) };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(prefix) && !entry.file_type()?.is_dir() {
            fs::remove_file(entry.path())?;
            removed.push(entry.path());
        }
    }
    log_removed(&removed);
    Ok(removed)
}

/// Remove every entry called `name` anywhere under `dir`: the unit file itself
/// and its enablement symlinks in *.wants / *.requires directories.
pub fn remove_unit(dir: &str, name: &str) -> Result<Vec<PathBuf>> {
    let mut hits = Vec::new();
    if fs::symlink_metadata(dir).is_err() {
        return Ok(hits);
    }
    walk(Path::new(dir), &mut |path, _| {
        if path.file_name().is_some_and(|n| n == name) {
            hits.push(path.to_path_buf());
        }
        Ok(())
    })?;
    for path in &hits {
        fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    }
    log_removed(&hits);
    Ok(hits)
}

/// Delete sudoers drop-ins that grant NOPASSWD on any non-comment line.
pub fn remove_nopasswd_sudoers(dir: &str) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return Ok(removed) };
    for entry in entries {
        let path = entry?.path();
        if !fs::symlink_metadata(&path)?.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        let grants = text.lines().any(|l| !l.trim_start().starts_with('#') && l.contains("NOPASSWD"));
        if grants {
            fs::remove_file(&path)?;
            removed.push(path);
        }
    }
    log_removed(&removed);
    Ok(removed)
}

/// Remove every line whose trimmed text is exactly `line`. JSON files must
/// still parse afterwards, otherwise nothing is written. A missing file is fine.
pub fn remove_lines(path: &str, line: &str) -> Result<usize> {
    let Ok(text) = fs::read_to_string(path) else { return Ok(0) };
    let kept: Vec<&str> = text.lines().filter(|l| l.trim() != line).collect();
    let removed = text.lines().count() - kept.len();
    if removed == 0 {
        return Ok(0);
    }
    let mut new = kept.join("\n");
    if text.ends_with('\n') {
        new.push('\n');
    }
    if path.ends_with(".json") {
        serde_json::from_str::<serde_json::Value>(&new)
            .with_context(|| format!("removing {line:?} would break the JSON in {path}, left it alone"))?;
    }
    fs::write(path, new).with_context(|| format!("writing {path}"))?;
    run::log(&format!("removed {removed} line(s) {line:?} from {path}"));
    Ok(removed)
}

fn log_removed(paths: &[PathBuf]) {
    for p in paths {
        run::log(&format!("removed {}", p.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arctix-clean-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn s(p: &Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn removes_unit_files_and_their_enablement_links() {
        let dir = scratch("units");
        fs::write(dir.join("pacman-init.service"), "[Unit]").unwrap();
        fs::create_dir_all(dir.join("multi-user.target.wants")).unwrap();
        symlink("/etc/systemd/system/pacman-init.service", dir.join("multi-user.target.wants/pacman-init.service")).unwrap();
        symlink("/usr/lib/systemd/system/NetworkManager.service", dir.join("multi-user.target.wants/NetworkManager.service")).unwrap();

        let removed = remove_unit(s(&dir), "pacman-init.service").unwrap();
        assert_eq!(removed.len(), 2);
        assert!(fs::symlink_metadata(dir.join("multi-user.target.wants/NetworkManager.service")).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removes_only_nopasswd_sudoers() {
        let dir = scratch("sudo");
        fs::write(dir.join("g_wheel"), "%wheel ALL=(ALL:ALL) NOPASSWD: ALL\n").unwrap();
        fs::write(dir.join("10-wheel"), "%wheel ALL=(ALL:ALL) ALL\n").unwrap();
        fs::write(dir.join("notes"), "# NOPASSWD is bad\n%wheel ALL=(ALL:ALL) ALL\n").unwrap();

        let removed = remove_nopasswd_sudoers(s(&dir)).unwrap();
        assert_eq!(removed, [dir.join("g_wheel")]);
        assert!(dir.join("10-wheel").exists() && dir.join("notes").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_path_does_not_follow_symlinks() {
        let dir = scratch("rm");
        let real = dir.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("keep"), "x").unwrap();
        symlink(&real, dir.join("link")).unwrap();

        assert!(remove_path(s(&dir.join("link")), true).unwrap());
        assert!(real.join("keep").exists());
        assert!(!remove_path(s(&dir.join("missing")), true).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unpins_the_installer_and_keeps_valid_json() {
        let dir = scratch("pin");
        let cfg = dir.join("config.json");
        let json = "{\n    \"dock\": {\n        \"pinnedApps\": [\n            \"arctix-install\",\n            \"org.gnome.Nautilus\",\n            \"kitty\"\n        ]\n    },\n    \"launcher\": {\n        \"pinnedApps\": [\n            \"arctix-install\",\n            \"kitty\"\n        ]\n    }\n}\n";
        fs::write(&cfg, json).unwrap();
        assert_eq!(remove_lines(s(&cfg), INSTALLER_PIN_LINE).unwrap(), 2);
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(v["dock"]["pinnedApps"], serde_json::json!(["org.gnome.Nautilus", "kitty"]));
        assert_eq!(v["launcher"]["pinnedApps"], serde_json::json!(["kitty"]));
        assert_eq!(remove_lines(s(&cfg), INSTALLER_PIN_LINE).unwrap(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_to_break_json() {
        let dir = scratch("badjson");
        let cfg = dir.join("config.json");
        // the pin is the last element, so removing it leaves a trailing comma problem
        let json = "{\"a\": [\n\"x\",\n\"arctix-install\",\n]}\n";
        fs::write(&cfg, json).unwrap();
        assert!(remove_lines(s(&cfg), INSTALLER_PIN_LINE).is_err());
        assert_eq!(fs::read_to_string(&cfg).unwrap(), json);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removes_host_keys_by_prefix() {
        let dir = scratch("ssh");
        for f in ["ssh_host_ed25519_key", "ssh_host_ed25519_key.pub", "sshd_config"] {
            fs::write(dir.join(f), "").unwrap();
        }
        assert_eq!(remove_matching(s(&dir), "ssh_host_").unwrap().len(), 2);
        assert!(dir.join("sshd_config").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
