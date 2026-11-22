//! GRUB on the installed system, for UEFI (ESP at /boot/efi) and BIOS
//! (GPT with a bios_grub partition).

use crate::config::Config;
use crate::partition::ESP_MOUNT;
use crate::plan::{Action, Firmware, Planner, Stage, chroot, t};
use anyhow::{Result, bail};
use std::fs;

pub const GRUB_CFG: &str = "/boot/grub/grub.cfg";
/// Where `grub-install --removable` puts the UEFI loader. Firmware finds it
/// without an NVRAM entry, which also suits VMs and moved disks.
pub const UEFI_FALLBACK_LOADER: &str = "/EFI/BOOT/BOOTX64.EFI";

pub(crate) fn steps(p: &mut Planner, cfg: &Config, fw: Firmware) {
    p.stage(Stage::Bootloader)
        .add(
            "Brand the boot menu",
            Action::SetKeyValue { path: t("/etc/default/grub"), key: "GRUB_DISTRIBUTOR".into(), value: "\"Arctix\"".into() },
        )
        // GRUB's 31_efi_bootnext adds a menu entry for every firmware boot option
        // present at install time: PXE, DVD drives, and the USB stick we installed from.
        .add(
            "Keep firmware boot options out of the menu",
            Action::SetKeyValue { path: t("/etc/default/grub"), key: "GRUB_DISABLE_BOOTNEXT".into(), value: "true".into() },
        );
    match fw {
        Firmware::Uefi => {
            p.add(
                "Install GRUB to the ESP",
                chroot(&["grub-install", "--target=x86_64-efi", &format!("--efi-directory={ESP_MOUNT}"),
                         "--bootloader-id=Arctix", "--removable"]),
            )
            .add(
                "Check the UEFI loader is in place",
                Action::EnsureFile { path: t(&format!("{ESP_MOUNT}{UEFI_FALLBACK_LOADER}")) },
            );
        }
        Firmware::Bios => {
            p.add("Install GRUB to the disk", chroot(&["grub-install", "--target=i386-pc", &cfg.disk.device]))
                .add("Check GRUB's boot code is in the MBR", Action::CheckMbr { device: cfg.disk.device.clone() });
        }
    }
    p.add("Generate the GRUB menu", chroot(&["grub-mkconfig", "-o", GRUB_CFG]))
        .add(
            "Check the menu boots our kernel and initramfs",
            Action::FileContains {
                path: t(GRUB_CFG),
                needles: vec!["vmlinuz-linux-lts".into(), "initramfs-linux-lts.img".into(), "Arctix".into()],
            },
        );
}

/// GRUB's boot.img (the 440 bytes of MBR boot code) contains the text "GRUB".
pub fn check_mbr(device: &str) -> Result<()> {
    use std::io::Read;
    let mut sector = [0u8; 512];
    fs::File::open(device)?.read_exact(&mut sector)?;
    if mbr_has_grub(&sector) { Ok(()) } else { bail!("no GRUB boot code in the first sector of {device}") }
}

fn mbr_has_grub(sector: &[u8; 512]) -> bool {
    sector[..440].windows(4).any(|w| w == b"GRUB") && sector[510..] == [0x55, 0xAA]
}

/// Fail unless the file contains every needle.
pub fn file_contains(path: &str, needles: &[String]) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let missing: Vec<&str> = needles.iter().map(String::as_str).filter(|n| !text.contains(n)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        bail!("{path} does not mention {}", missing.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;

    fn lines(fw: Firmware) -> Vec<String> {
        let cfg = Config::parse(include_str!("../examples/arctix.toml")).unwrap();
        plan::build(&cfg, fw).iter().map(|s| s.action.to_string()).collect()
    }

    #[test]
    fn uefi_installs_the_removable_loader_and_checks_it() {
        let l = lines(Firmware::Uefi);
        let pos = |n: &str| l.iter().position(|x| x.contains(n)).unwrap_or_else(|| panic!("no {n}"));
        assert!(pos("--target=x86_64-efi") < pos("/mnt/boot/efi/EFI/BOOT/BOOTX64.EFI exists"));
        assert!(pos("grub-mkconfig") < pos("grub.cfg contains"));
        assert!(pos("grub.cfg contains") < pos("umount -R /mnt"));
    }

    #[test]
    fn bios_has_no_esp_loader_check() {
        let l = lines(Firmware::Bios);
        assert!(!l.iter().any(|x| x.contains("BOOTX64.EFI")));
        assert!(l.iter().any(|x| x.contains("grub-install --target=i386-pc /dev/vda")));
    }

    #[test]
    fn mbr_check_wants_grub_code_and_signature() {
        let mut sector = [0u8; 512];
        assert!(!mbr_has_grub(&sector));
        sector[0x180..0x184].copy_from_slice(b"GRUB");
        assert!(!mbr_has_grub(&sector), "no 55AA signature yet");
        sector[510] = 0x55;
        sector[511] = 0xAA;
        assert!(mbr_has_grub(&sector));
    }

    #[test]
    fn bios_checks_the_mbr_after_grub_install() {
        let l = lines(Firmware::Bios);
        let pos = |n: &str| l.iter().position(|x| x.contains(n)).unwrap();
        assert!(pos("--target=i386-pc") < pos("GRUB boot code in the first sector of /dev/vda"));
    }

    #[test]
    fn file_contains_reports_what_is_missing() {
        let path = std::env::temp_dir().join(format!("arctix-grubcfg-{}", std::process::id()));
        fs::write(&path, "menuentry 'Arctix' { linux /boot/vmlinuz-linux-lts }").unwrap();
        let p = path.to_str().unwrap();
        assert!(file_contains(p, &["vmlinuz-linux-lts".into(), "Arctix".into()]).is_ok());
        let err = file_contains(p, &["initramfs-linux-lts.img".into()]).unwrap_err().to_string();
        assert!(err.contains("initramfs-linux-lts.img"));
        fs::remove_file(path).unwrap();
    }
}
