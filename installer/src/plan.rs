//! Turns a config into a list of Steps without running anything.
//! `--dry-run` prints this list; the real install will execute the same list.

use crate::config::{Config, Mode};
use crate::partition;
use crate::{bootloader, cleanup, configure, populate};
use std::fmt;
use std::path::Path;

pub const TARGET: &str = "/mnt";
pub const LIVE_USER: &str = "arctix";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firmware {
    Uefi,
    Bios,
}

impl Firmware {
    pub fn detect() -> Self {
        if Path::new("/sys/firmware/efi").exists() { Self::Uefi } else { Self::Bios }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "uefi" => Some(Self::Uefi),
            "bios" => Some(Self::Bios),
            _ => None,
        }
    }
}

impl fmt::Display for Firmware {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Self::Uefi => "uefi",
            Self::Bios => "bios",
        })
    }
}

/// Stages in execution order, so `<=` means "runs no later than".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Preflight,
    Partition,
    Populate,
    Configure,
    Initramfs,
    Cleanup,
    User,
    Packages,
    Bootloader,
    Finish,
}

impl Stage {
    pub const ALL: [Stage; 10] = [
        Self::Preflight,
        Self::Partition,
        Self::Populate,
        Self::Configure,
        Self::Initramfs,
        Self::Cleanup,
        Self::User,
        Self::Packages,
        Self::Bootloader,
        Self::Finish,
    ];

    /// Short name used on the command line (`--stop-after partition`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Partition => "partition",
            Self::Populate => "populate",
            Self::Configure => "configure",
            Self::Initramfs => "initramfs",
            Self::Cleanup => "cleanup",
            Self::User => "user",
            Self::Packages => "packages",
            Self::Bootloader => "bootloader",
            Self::Finish => "finish",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|st| st.key() == s)
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Self::Preflight => "preflight",
            Self::Partition => "partition",
            Self::Populate => "populate",
            Self::Configure => "configure",
            Self::Initramfs => "initramfs",
            Self::Cleanup => "strip live config",
            Self::User => "user",
            Self::Packages => "keyring, mirrors, update",
            Self::Bootloader => "bootloader",
            Self::Finish => "finish",
        })
    }
}

/// When a step runs. Online steps are skipped quietly when there is no network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    /// Only with a network; a failure fails the install.
    Online,
    /// Only with a network; a failure is a warning and the install carries on.
    OnlineBestEffort,
}

impl When {
    pub fn needs_network(self) -> bool {
        self != When::Always
    }
}

/// An external command, optionally run inside `arch-chroot /mnt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub chroot: bool,
}

impl Cmd {
    pub(crate) fn new(argv: &[&str], chroot: bool) -> Self {
        let (program, args) = argv.split_first().expect("a command needs a program");
        Self {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            chroot,
        }
    }

    /// Full argv as it will be executed.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        if self.chroot {
            argv.extend(["arch-chroot".to_string(), TARGET.to_string()]);
        }
        argv.push(self.program.clone());
        argv.extend(self.args.iter().cloned());
        argv
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let quoted: Vec<String> = self.argv().iter().map(|a| shell_quote(a)).collect();
        f.write_str(&quoted.join(" "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Re-check the target right before touching it (exists, whole disk, unmounted, not live).
    CheckTarget { device: String },
    /// The user types the device path to confirm. Skipped only with `--yes`.
    Confirm { device: String },
    Run(Cmd),
    /// Run a command and write its stdout to a file.
    RunToFile { cmd: Cmd, path: String },
    WriteFile { path: String, contents: String, mode: u32 },
    Remove { path: String, recursive: bool },
    /// Remove every file in `dir` whose name starts with `prefix`.
    RemoveMatching { dir: String, prefix: String },
    /// Remove every entry called `name` anywhere under `dir` (unit files and enablement links).
    RemoveUnit { dir: String, name: String },
    /// Delete sudoers drop-ins in `dir` that contain a NOPASSWD rule.
    RemoveNopasswdSudoers { dir: String },
    UncommentLine { path: String, line: String },
    SetKeyValue { path: String, key: String, value: String },
    /// Poll until all these paths exist (device nodes after partitioning).
    WaitForPaths { paths: Vec<String> },
    /// Fail unless `path` is an existing file.
    EnsureFile { path: String },
    /// Copy the kernel of package `pkgbase` into `root`/boot, like mkinitcpio's pacman hook.
    InstallKernel { root: String, pkgbase: String },
    Pacstrap { package_set: String },
    /// Check timezone, locale and keymap exist on the live system.
    CheckSystemValues { timezone: String, locale: String, keymap: String },
    /// Ask for the password (before anything is touched) and keep it in memory.
    AskPassword { user: String },
    /// Feed the password collected by AskPassword to chpasswd on stdin.
    SetPassword { user: String },
    ReplaceInTree { dir: String, from: String, to: String },
    /// Set `kb_layout` in a Hyprland Lua config (warning only if it is not there).
    SetDesktopLayout { path: String, layout: String },
    /// Remove lines whose trimmed text equals `line`; JSON files must still parse.
    RemoveLines { path: String, line: String },
    /// Fail unless the disk's MBR holds GRUB's boot code (BIOS installs).
    CheckMbr { device: String },
    /// Fail unless the file at `path` contains every needle.
    FileContains { path: String, needles: Vec<String> },
    /// Run a command and fail if its output contains `needle`.
    ExpectAbsent { cmd: Cmd, needle: String },
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::CheckTarget { device } => {
                write!(
                    f,
                    "check we are root on the live ISO, {TARGET} is free, and {device} is a whole, \
                     unmounted, writable disk that is big enough and not the live medium"
                )
            }
            Self::WaitForPaths { paths } => write!(f, "wait until {} exist", paths.join(" and ")),
            Self::Confirm { device } => write!(f, "ask you to type {device} to confirm wiping it (--yes skips this)"),
            Self::Run(cmd) => write!(f, "{cmd}"),
            Self::RunToFile { cmd, path } => write!(f, "{cmd} > {path}"),
            Self::WriteFile { path, mode, .. } => write!(f, "write {path} (mode {mode:04o})"),
            Self::Remove { path, recursive: true } => write!(f, "rm -rf {path}"),
            Self::Remove { path, recursive: false } => write!(f, "rm -f {path}"),
            Self::RemoveMatching { dir, prefix } => write!(f, "rm -f {dir}/{prefix}*"),
            Self::RemoveUnit { dir, name } => write!(f, "remove every {name} under {dir}"),
            Self::RemoveNopasswdSudoers { dir } => write!(f, "delete files in {dir} that grant NOPASSWD"),
            Self::UncommentLine { path, line } => write!(f, "uncomment \"{line}\" in {path}"),
            Self::SetKeyValue { path, key, value } => write!(f, "set {key}={value} in {path}"),
            Self::EnsureFile { path } => write!(f, "check {path} exists"),
            Self::InstallKernel { root, pkgbase } => write!(
                f,
                "copy {root}/usr/lib/modules/<ver>/vmlinuz ({pkgbase}) to {root}/boot/vmlinuz-{pkgbase}, \
                 else the ISO's kernel"
            ),
            Self::Pacstrap { package_set } => write!(f, "pacstrap -K {TARGET} <packages from {package_set}.txt>"),
            Self::CheckSystemValues { timezone, locale, keymap } => {
                write!(f, "check timezone {timezone}, locale {locale} and keymap {keymap} exist")
            }
            Self::AskPassword { user } => {
                write!(f, "ask for {user}'s password twice, hidden (--password-stdin reads one line instead)")
            }
            Self::SetPassword { user } => {
                write!(f, "pipe {user}'s password to chpasswd in the chroot on stdin (never logged)")
            }
            Self::ReplaceInTree { dir, from, to } => write!(f, "replace {from} with {to} in files under {dir}"),
            Self::SetDesktopLayout { path, layout } => write!(f, "set kb_layout = \"{layout}\" in {path}"),
            Self::RemoveLines { path, line } => write!(f, "remove lines {line} from {path}"),
            Self::CheckMbr { device } => write!(f, "check for GRUB boot code in the first sector of {device}"),
            Self::FileContains { path, needles } => write!(f, "check {path} contains {}", needles.join(", ")),
            Self::ExpectAbsent { cmd, needle } => write!(f, "{cmd}  (must not mention \"{needle}\")"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub stage: Stage,
    pub desc: String,
    pub when: When,
    pub action: Action,
}

#[derive(Default)]
pub(crate) struct Planner {
    steps: Vec<Step>,
    stage: Option<Stage>,
}

impl Planner {
    pub(crate) fn stage(&mut self, stage: Stage) -> &mut Self {
        self.stage = Some(stage);
        self
    }

    fn push(&mut self, when: When, desc: &str, action: Action) -> &mut Self {
        let stage = self.stage.expect("set a stage before adding steps");
        self.steps.push(Step { stage, desc: desc.to_string(), when, action });
        self
    }

    pub(crate) fn add(&mut self, desc: &str, action: Action) -> &mut Self {
        self.push(When::Always, desc, action)
    }

    fn online(&mut self, desc: &str, action: Action) -> &mut Self {
        self.push(When::Online, desc, action)
    }

    fn online_best_effort(&mut self, desc: &str, action: Action) -> &mut Self {
        self.push(When::OnlineBestEffort, desc, action)
    }
}

pub(crate) fn host(argv: &[&str]) -> Action {
    Action::Run(Cmd::new(argv, false))
}

pub(crate) fn chroot(argv: &[&str]) -> Action {
    Action::Run(Cmd::new(argv, true))
}

/// A path inside the target, e.g. t("/etc/hostname") -> "/mnt/etc/hostname".
pub(crate) fn t(path: &str) -> String {
    format!("{TARGET}{path}")
}

pub(crate) fn write(path: &str, contents: impl Into<String>, mode: u32) -> Action {
    Action::WriteFile { path: t(path), contents: contents.into(), mode }
}

pub(crate) fn remove(path: &str, recursive: bool) -> Action {
    Action::Remove { path: t(path), recursive }
}

pub fn build(cfg: &Config, fw: Firmware) -> Vec<Step> {
    let mut p = Planner::default();
    preflight(&mut p, cfg);
    partition::steps(&mut p, cfg, fw);
    populate::steps(&mut p, cfg);
    configure::system_steps(&mut p, cfg);
    configure::initramfs_steps(&mut p);
    if cfg.install.mode == Mode::Copy {
        cleanup::steps(&mut p);
    }
    configure::user_steps(&mut p, cfg);
    packages(&mut p, cfg);
    bootloader::steps(&mut p, cfg, fw);
    finish(&mut p);
    p.steps
}

fn preflight(p: &mut Planner, cfg: &Config) {
    let dev = &cfg.disk.device;
    p.stage(Stage::Preflight)
        .add("Make sure the target disk is safe to wipe", Action::CheckTarget { device: dev.clone() });
    populate::preflight_steps(p, cfg);
    configure::preflight_steps(p, cfg);
    p.add("Confirm before anything destructive", Action::Confirm { device: dev.clone() });
    configure::password_steps(p, cfg);
}


fn packages(p: &mut Planner, cfg: &Config) {
    if cfg.install.mode != Mode::Copy {
        // pacstrap -K already set up the keyring and the packages are fresh.
        return;
    }
    // The live image gets its keyring from pacman-init.service (removed in
    // cleanup), so the unpacked copy has none. No network needed for this.
    p.stage(Stage::Packages)
        .add("Init the pacman keyring", chroot(&["pacman-key", "--init"]))
        .add("Populate the Arch keys", chroot(&["pacman-key", "--populate", "archlinux"]))
        // A failure here just keeps the ISO's mirrorlist, which works.
        .online_best_effort(
            "Pick fast mirrors",
            chroot(&["reflector", "--latest", "10", "--protocol", "https", "--sort", "rate",
                     "--connection-timeout", "5", "--download-timeout", "5",
                     "--save", "/etc/pacman.d/mirrorlist"]),
        );
    if cfg.install.update {
        // "Installing fresh". If it fails, the system is exactly what the ISO
        // shipped, which boots; the user can update later.
        p.online_best_effort("Installing fresh: update everything", chroot(&["pacman", "-Syu", "--noconfirm"]))
            .online("Rebuild the initramfs in case the kernel updated", chroot(&["mkinitcpio", "-P"]));
    }
}

fn finish(p: &mut Planner) {
    p.stage(Stage::Finish)
        .add("Flush writes", host(&["sync"]))
        .add("Unmount the target", host(&["umount", "-R", TARGET]));
}

/// Human readable dry-run output.
pub fn render(cfg: &Config, fw: Firmware, steps: &[Step]) -> String {
    let mut out = String::new();
    let mode = match cfg.install.mode {
        Mode::Copy => "copy (installing fresh)".to_string(),
        Mode::Pacstrap => format!("pacstrap ({})", cfg.install.package_set.as_deref().unwrap_or("?")),
    };
    out += "Arctix install plan (dry run, nothing will be touched)\n";
    out += &format!("  disk:     {} ({fw})\n", cfg.disk.device);
    out += &format!("  mode:     {mode}\n");
    out += &format!("  system:   {} / {} / {} / {}\n", cfg.system.hostname, cfg.system.timezone, cfg.system.locale, cfg.system.keymap);
    out += &format!("  user:     {}{}\n", cfg.user.name, if cfg.user.sudo { " (sudo)" } else { "" });

    let mut current = None;
    for (i, step) in steps.iter().enumerate() {
        if current != Some(step.stage) {
            current = Some(step.stage);
            out += &format!("\n== {} ==\n", step.stage);
        }
        let tag = match step.when {
            When::Always => "",
            When::Online => "  [only if online]",
            When::OnlineBestEffort => "  [only if online, failure is only a warning]",
        };
        out += &format!("{:>3}. {}{tag}\n       {}\n", i + 1, step.desc, step.action);
        if let Action::WriteFile { contents, .. } = &step.action {
            for line in contents.lines() {
                out += &format!("       | {line}\n");
            }
        }
    }
    out += &format!("\n{} steps\n", steps.len());
    out
}

/// Single-quote an argument for display if it has anything shell-special in it.
fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_./:=@%+,-".contains(c));
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../examples/arctix.toml");

    fn cfg(edit: impl Fn(&mut Config)) -> Config {
        let mut c = Config::parse(SAMPLE).unwrap();
        edit(&mut c);
        c
    }

    fn lines(steps: &[Step]) -> Vec<String> {
        steps.iter().map(|s| s.action.to_string()).collect()
    }

    fn has(steps: &[Step], needle: &str) -> bool {
        lines(steps).iter().any(|l| l.contains(needle))
    }

    fn index_of(steps: &[Step], needle: &str) -> usize {
        lines(steps).iter().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("no step with {needle:?}"))
    }

    #[test]
    fn uefi_copy_plan_has_the_essentials_in_order() {
        let steps = build(&cfg(|_| {}), Firmware::Uefi);
        let order = [
            "/dev/vda is a whole",
            "ask you to type /dev/vda",
            "wipefs -a /dev/vda",
            "1:ef00",
            "udevadm settle",
            "mkfs.fat -F 32 -n ARCTIX_EFI /dev/vda1",
            "mkfs.ext4 -F -L arctix /dev/vda2",
            "mount /dev/vda2 /mnt",
            "mount --mkdir -o fmask=0077,dmask=0077 /dev/vda1 /mnt/boot/efi",
            "unsquashfs",
            "genfstab -U /mnt > /mnt/etc/fstab",
            "rm -f /mnt/etc/mkinitcpio.conf.d/archiso.conf",
            "arch-chroot /mnt mkinitcpio -P",
            "lsinitcpio",
            "rm -rf /mnt/etc/systemd/system/getty@tty1.service.d",
            "usermod -l cammy -d /home/cammy -m arctix",
            "chpasswd",
            "pacman-key --init",
            "pacman -Syu --noconfirm",
            "grub-install --target=x86_64-efi --efi-directory=/boot/efi --bootloader-id=Arctix --removable",
            "grub-mkconfig -o /boot/grub/grub.cfg",
            "umount -R /mnt",
        ];
        let idx: Vec<usize> = order.iter().map(|n| index_of(&steps, n)).collect();
        assert!(idx.windows(2).all(|w| w[0] < w[1]), "steps out of order: {idx:?}");
        assert_eq!(steps.last().unwrap().action.to_string(), "umount -R /mnt");
    }

    #[test]
    fn udevadm_settle_follows_partitioning_and_mkfs() {
        let steps = build(&cfg(|_| {}), Firmware::Uefi);
        let l = lines(&steps);
        let settles: Vec<usize> = l.iter().enumerate().filter(|(_, s)| *s == "udevadm settle").map(|(i, _)| i).collect();
        assert_eq!(settles.len(), 2);
        assert!(settles[0] > index_of(&steps, "partprobe") && settles[0] < index_of(&steps, "mkfs.fat"));
        assert!(settles[1] > index_of(&steps, "mkfs.ext4") && settles[1] < index_of(&steps, "mount /dev/vda2"));
    }

    #[test]
    fn bios_plan_uses_bios_grub_and_no_esp() {
        let steps = build(&cfg(|_| {}), Firmware::Bios);
        assert!(has(&steps, "1:ef02"));
        assert!(!has(&steps, "mkfs.fat"));
        assert!(!has(&steps, "/boot/efi"));
        assert!(has(&steps, "grub-install --target=i386-pc /dev/vda"));
    }

    #[test]
    fn nvme_partitions_are_named_right() {
        let steps = build(&cfg(|c| c.disk.device = "/dev/nvme0n1".into()), Firmware::Uefi);
        assert!(has(&steps, "mkfs.fat -F 32 -n ARCTIX_EFI /dev/nvme0n1p1"));
        assert!(has(&steps, "mount /dev/nvme0n1p2 /mnt"));
    }

    #[test]
    fn only_the_configured_disk_is_touched() {
        let steps = build(&cfg(|c| c.disk.device = "/dev/vdq".into()), Firmware::Bios);
        for l in lines(&steps) {
            for dev in ["/dev/sd", "/dev/nvme", "/dev/vda"] {
                assert!(!l.contains(dev), "unexpected device in: {l}");
            }
        }
    }

    #[test]
    fn network_steps_are_online_only() {
        let steps = build(&cfg(|_| {}), Firmware::Uefi);
        for s in &steps {
            let Action::Run(cmd) = &s.action else { continue };
            let l = cmd.to_string();
            if cmd.program == "reflector" || l.contains("pacman -Syu") {
                assert_eq!(s.when, When::OnlineBestEffort, "{l} must be skippable and never fatal");
            }
            if cmd.program == "pacman-key" {
                assert_eq!(s.when, When::Always, "the keyring is needed offline too");
            }
        }
        let no_update = build(&cfg(|c| c.install.update = false), Firmware::Uefi);
        assert!(!has(&no_update, "pacman -Syu"));
        assert!(has(&no_update, "reflector"));
    }

    #[test]
    fn live_user_name_skips_the_rename() {
        let steps = build(&cfg(|c| c.user.name = LIVE_USER.into()), Firmware::Uefi);
        assert!(!has(&steps, "usermod -l"));
        assert!(has(&steps, "chpasswd"));
    }

    #[test]
    fn no_sudo_means_no_wheel() {
        let steps = build(&cfg(|c| c.user.sudo = false), Firmware::Uefi);
        assert!(has(&steps, "gpasswd -d cammy wheel"));
        assert!(!has(&steps, "10-wheel"));
        // the live NOPASSWD rule is removed either way
        assert!(has(&steps, "NOPASSWD"));
    }

    #[test]
    fn desktop_layout_follows_the_console_keymap() {
        let steps = build(&cfg(|c| c.system.keymap = "de-latin1".into()), Firmware::Uefi);
        assert!(has(&steps, "set kb_layout = \"de\" in /mnt/home/cammy/.config/hypr/hyprland/general.lua"));
        // after the rename, so the path exists
        assert!(index_of(&steps, "usermod -l cammy") < index_of(&steps, "set kb_layout"));
    }

    #[test]
    fn root_gets_locked() {
        let steps = build(&cfg(|_| {}), Firmware::Uefi);
        assert!(has(&steps, "arch-chroot /mnt usermod -p '!*' root"));
    }

    #[test]
    fn live_services_are_stripped() {
        let steps = build(&cfg(|_| {}), Firmware::Uefi);
        for unit in ["pacman-init.service", "etc-pacman.d-gnupg.mount", "choose-mirror.service", "sshd.service"] {
            assert!(has(&steps, &format!("remove every {unit}")), "{unit} not removed");
        }
        // the gnupg tmpfs mount must be gone before the keyring is created
        assert!(index_of(&steps, "etc-pacman.d-gnupg.mount") < index_of(&steps, "pacman-key --init"));
    }

    #[test]
    fn pacstrap_plan_skips_copy_only_steps() {
        let steps = build(&cfg(|c| c.install.mode = Mode::Pacstrap), Firmware::Uefi);
        assert!(has(&steps, "pacstrap -K /mnt <packages from desktop.txt>"));
        assert!(has(&steps, "useradd -m -s /usr/bin/fish -G wheel cammy"));
        for gone in ["unsquashfs", "usermod -l", "getty@tty1", "pacman-key", "pacman -Syu"] {
            assert!(!has(&steps, gone), "{gone} should not be in a pacstrap plan");
        }
    }


    #[test]
    fn display_quotes_odd_arguments() {
        assert_eq!(shell_quote("/dev/vda"), "/dev/vda");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn render_shows_file_contents_and_counts() {
        let c = cfg(|_| {});
        let steps = build(&c, Firmware::Uefi);
        let text = render(&c, Firmware::Uefi, &steps);
        assert!(text.contains("| HOOKS=(base udev autodetect"));
        assert!(text.contains("[only if online]"));
        assert!(text.ends_with(&format!("{} steps\n", steps.len())));
    }
}
