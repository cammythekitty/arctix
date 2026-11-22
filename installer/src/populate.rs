//! Getting a root filesystem onto the target.
//!
//! Copy mode unpacks the live squashfs (the clean image, not the running system,
//! so there is no /proc, /run or live runtime state to exclude).
//! Pacstrap mode installs a package set from the network (phase 9).

use crate::config::{Config, Mode};
use crate::plan::{Action, Planner, Stage, TARGET, host, t};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub const SQUASHFS: &str = "/run/archiso/bootmnt/arch/x86_64/airootfs.sfs";
/// The ISO's copy of the kernel, used only if the image has no usable one.
pub const LIVE_KERNEL: &str = "/run/archiso/bootmnt/arch/boot/x86_64/vmlinuz-linux-lts";
pub const KERNEL_PKGBASE: &str = "linux-lts";
pub const KERNEL: &str = "/boot/vmlinuz-linux-lts";

/// Checks that belong before the disk is wiped. Runs inside the preflight stage.
pub(crate) fn preflight_steps(p: &mut Planner, cfg: &Config) {
    if cfg.install.mode == Mode::Copy {
        p.add("Make sure the live image is readable", Action::EnsureFile { path: SQUASHFS.into() });
    }
}

pub(crate) fn steps(p: &mut Planner, cfg: &Config) {
    p.stage(Stage::Populate);
    match cfg.install.mode {
        Mode::Copy => {
            p.add(
                    "Unpack the clean live image onto the target (takes a few minutes)",
                    host(&["unsquashfs", "-f", "-no-progress", "-d", TARGET, SQUASHFS]),
                )
                // mkarchiso moves /boot out of the squashfs onto the ISO, so the
                // unpacked tree has no kernel in /boot. Put it back the way
                // mkinitcpio's pacman hook does.
                .add(
                    "Put the kernel in /boot",
                    Action::InstallKernel { root: TARGET.into(), pkgbase: KERNEL_PKGBASE.into() },
                )
                .add("Check the unpacked tree looks complete", Action::EnsureFile { path: t("/usr/bin/pacman") });
        }
        Mode::Pacstrap => {
            let set = cfg.install.package_set.clone().unwrap_or_default();
            p.add("Install the package set", Action::Pacstrap { package_set: set })
                .add("Make sure the kernel is installed", Action::EnsureFile { path: t(KERNEL) });
        }
    }
}

pub fn ensure_file(path: &str) -> Result<()> {
    if Path::new(path).is_file() {
        Ok(())
    } else if path == SQUASHFS {
        bail!("{path} is missing. Is the boot medium still mounted? (copytoram boots unmount it)")
    } else {
        bail!("{path} is missing")
    }
}

/// Copy `usr/lib/modules/<ver>/vmlinuz` of the kernel package `pkgbase` to
/// `boot/vmlinuz-<pkgbase>` under `root`. Falls back to the ISO kernel.
pub fn install_kernel(root: &Path, pkgbase: &str, iso_fallback: &Path) -> Result<PathBuf> {
    let dest = root.join(format!("boot/vmlinuz-{pkgbase}"));
    let source = match package_kernel(root, pkgbase)? {
        Some(src) => src,
        None if iso_fallback.is_file() => iso_fallback.to_path_buf(),
        None => bail!(
            "no kernel for {pkgbase} in {} and no ISO kernel at {}",
            root.join("usr/lib/modules").display(),
            iso_fallback.display()
        ),
    };
    fs::create_dir_all(root.join("boot"))?;
    fs::copy(&source, &dest).with_context(|| format!("copying {} to {}", source.display(), dest.display()))?;
    crate::run::log(&format!("kernel: {} -> {}", source.display(), dest.display()));
    Ok(source)
}

/// The vmlinuz of the one modules directory whose `pkgbase` file says `pkgbase`.
fn package_kernel(root: &Path, pkgbase: &str) -> Result<Option<PathBuf>> {
    let modules = root.join("usr/lib/modules");
    let Ok(entries) = fs::read_dir(&modules) else { return Ok(None) };
    let mut found = Vec::new();
    for entry in entries {
        let dir = entry?.path();
        let base = fs::read_to_string(dir.join("pkgbase")).unwrap_or_default();
        if base.trim() == pkgbase && dir.join("vmlinuz").is_file() {
            found.push(dir.join("vmlinuz"));
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => bail!("more than one {pkgbase} kernel in {}: {found:?}", modules.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch root under the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("arctix-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, rel: &str, contents: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn kernel_comes_from_the_matching_modules_dir() {
        let s = Scratch::new("pkgkernel");
        s.file("usr/lib/modules/6.18.64-1-lts/pkgbase", "linux-lts\n");
        s.file("usr/lib/modules/6.18.64-1-lts/vmlinuz", "lts kernel");
        s.file("usr/lib/modules/6.19.1-arch1-1/pkgbase", "linux\n");
        s.file("usr/lib/modules/6.19.1-arch1-1/vmlinuz", "mainline kernel");
        let iso = s.file("iso/vmlinuz-linux-lts", "iso kernel");

        install_kernel(&s.0, "linux-lts", &iso).unwrap();
        assert_eq!(fs::read_to_string(s.0.join("boot/vmlinuz-linux-lts")).unwrap(), "lts kernel");
    }

    #[test]
    fn falls_back_to_the_iso_kernel() {
        let s = Scratch::new("isokernel");
        let iso = s.file("iso/vmlinuz-linux-lts", "iso kernel");
        install_kernel(&s.0, "linux-lts", &iso).unwrap();
        assert_eq!(fs::read_to_string(s.0.join("boot/vmlinuz-linux-lts")).unwrap(), "iso kernel");
    }

    #[test]
    fn fails_with_no_kernel_anywhere() {
        let s = Scratch::new("nokernel");
        let err = install_kernel(&s.0, "linux-lts", &s.0.join("nope")).unwrap_err();
        assert!(err.to_string().contains("no kernel for linux-lts"));
    }

    #[test]
    fn refuses_to_guess_between_two_matching_kernels() {
        let s = Scratch::new("twokernels");
        for v in ["6.18.1-lts", "6.18.2-lts"] {
            s.file(&format!("usr/lib/modules/{v}/pkgbase"), "linux-lts");
            s.file(&format!("usr/lib/modules/{v}/vmlinuz"), v);
        }
        assert!(install_kernel(&s.0, "linux-lts", &s.0.join("nope")).is_err());
    }

    #[test]
    fn missing_squashfs_explains_itself() {
        let err = ensure_file(SQUASHFS).unwrap_err().to_string();
        assert!(err.contains("boot medium"));
    }
}

#[cfg(test)]
mod plan_tests {
    use crate::config::Config;
    use crate::plan::{self, Firmware, Stage};

    #[test]
    fn image_is_checked_before_the_disk_is_touched() {
        let cfg = Config::parse(include_str!("../examples/arctix.toml")).unwrap();
        let steps = plan::build(&cfg, Firmware::Uefi);
        let check = steps.iter().find(|s| s.action.to_string().contains("airootfs.sfs exists")).unwrap();
        assert_eq!(check.stage, Stage::Preflight);
        let wipe = steps.iter().position(|s| s.action.to_string().starts_with("wipefs")).unwrap();
        let check_pos = steps.iter().position(|s| s.action.to_string().contains("airootfs.sfs exists")).unwrap();
        let confirm = steps.iter().position(|s| s.action.to_string().contains("to confirm")).unwrap();
        assert!(check_pos < confirm && confirm < wipe);
    }
}
