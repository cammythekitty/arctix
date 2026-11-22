//! Machine-specific config, the initramfs fix and the user, plus the file
//! actions those steps use. Everything here writes inside the target.

use crate::config::{Config, Mode};
use crate::plan::{Action, Cmd, LIVE_USER, Planner, Stage, TARGET, chroot, host, remove, t, write};
use crate::run;
use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{PermissionsExt, lchown, symlink};
use std::path::{Path, PathBuf};

pub const INITRAMFS: &str = "/boot/initramfs-linux-lts.img";

pub const MKINITCPIO_CONF: &str = "\
# Written by arctix-install. Live ISO boot hooks must never be added here.
MODULES=()
BINARIES=()
FILES=()
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block filesystems fsck)
";

pub const LINUX_LTS_PRESET: &str = "\
# mkinitcpio preset file for the 'linux-lts' package
ALL_config=\"/etc/mkinitcpio.conf\"
ALL_kver=\"/boot/vmlinuz-linux-lts\"
PRESETS=('default' 'fallback')
default_image=\"/boot/initramfs-linux-lts.img\"
fallback_image=\"/boot/initramfs-linux-lts-fallback.img\"
fallback_options=\"-S autodetect\"
";

/// The live user's Hyprland config, where the desktop keyboard layout lives.
pub const HYPR_INPUT_CONFIG: &str = ".config/hypr/hyprland/general.lua";

/// Files bigger than this are never rewritten by ReplaceInTree.
const REPLACE_MAX_BYTES: u64 = 4 * 1024 * 1024;

// ---------------------------------------------------------------- steps

/// Preflight: things we can check on the live system before wiping anything.
pub(crate) fn preflight_steps(p: &mut Planner, cfg: &Config) {
    let sys = &cfg.system;
    p.add(
        "Check timezone, locale and keymap exist",
        Action::CheckSystemValues {
            timezone: sys.timezone.clone(),
            locale: sys.locale.clone(),
            keymap: sys.keymap.clone(),
        },
    );
}

/// Preflight, after the wipe confirmation: collect the password up front so
/// nobody has to sit through the unpack waiting for a prompt.
pub(crate) fn password_steps(p: &mut Planner, cfg: &Config) {
    p.add("Ask for the user's password", Action::AskPassword { user: cfg.user.name.clone() });
}

pub(crate) fn system_steps(p: &mut Planner, cfg: &Config) {
    let sys = &cfg.system;
    let host_name = &sys.hostname;
    p.stage(Stage::Configure)
        .add(
            "Generate fstab from the mounted filesystems",
            Action::RunToFile { cmd: Cmd::new(&["genfstab", "-U", TARGET], false), path: t("/etc/fstab") },
        )
        .add("Empty machine-id so it is regenerated on first boot", write("/etc/machine-id", "", 0o444))
        .add(
            "Drop any live SSH host keys",
            Action::RemoveMatching { dir: t("/etc/ssh"), prefix: "ssh_host_".into() },
        )
        .add("Hostname", write("/etc/hostname", format!("{host_name}\n"), 0o644))
        .add(
            "Hosts file",
            write(
                "/etc/hosts",
                format!("127.0.0.1 localhost\n::1 localhost\n127.0.1.1 {host_name}.localdomain {host_name}\n"),
                0o644,
            ),
        )
        .add(
            "Timezone",
            chroot(&["ln", "-sf", &format!("/usr/share/zoneinfo/{}", sys.timezone), "/etc/localtime"]),
        )
        .add("Set the hardware clock", chroot(&["hwclock", "--systohc"]))
        .add(
            "Enable the locale",
            Action::UncommentLine { path: t("/etc/locale.gen"), line: cfg.locale_gen_line() },
        )
        .add("Generate locales", chroot(&["locale-gen"]))
        .add("Locale", write("/etc/locale.conf", format!("LANG={}\n", sys.locale), 0o644))
        .add("Console keymap", write("/etc/vconsole.conf", format!("KEYMAP={}\n", sys.keymap), 0o644));
}

pub(crate) fn initramfs_steps(p: &mut Planner) {
    p.stage(Stage::Initramfs)
        .add("Remove the archiso mkinitcpio drop-in", remove("/etc/mkinitcpio.conf.d/archiso.conf", false))
        .add("Remove the archiso mkinitcpio config", remove("/etc/mkinitcpio.archiso.conf", false))
        .add("Write a normal mkinitcpio.conf", write("/etc/mkinitcpio.conf", MKINITCPIO_CONF, 0o644))
        .add("Write the standard linux-lts preset", write("/etc/mkinitcpio.d/linux-lts.preset", LINUX_LTS_PRESET, 0o644))
        .add("Build the initramfs (takes a minute)", chroot(&["mkinitcpio", "-P"]))
        .add(
            "Verify no archiso hook made it in",
            Action::ExpectAbsent { cmd: Cmd::new(&["lsinitcpio", "-c", INITRAMFS], true), needle: "archiso".into() },
        );
}

pub(crate) fn user_steps(p: &mut Planner, cfg: &Config) {
    let u = &cfg.user;
    let name = u.name.as_str();
    let home = format!("/home/{name}");
    let fix_paths = Action::ReplaceInTree {
        dir: t(&format!("{home}/.config")),
        from: format!("/home/{LIVE_USER}"),
        to: home.clone(),
    };
    p.stage(Stage::User);

    match cfg.install.mode {
        Mode::Copy => {
            if name != LIVE_USER {
                // Renaming keeps uid 1000, moves the home and keeps ownership right,
                // so there is never a second uid 1000 user.
                p.add("Rename the live user", chroot(&["usermod", "-l", name, "-d", &home, "-m", LIVE_USER]))
                    .add("Rename the live user's group", chroot(&["groupmod", "-n", name, LIVE_USER]))
                    .add("Point configs at the new home", fix_paths);
            }
            p.add("Login shell", chroot(&["usermod", "-s", &u.shell, name]));
            if u.sudo {
                p.add("Make sure the user is in wheel", chroot(&["usermod", "-aG", "wheel", name]));
            } else {
                p.add("Take the user out of wheel", chroot(&["gpasswd", "-d", name, "wheel"]));
            }
        }
        Mode::Pacstrap => {
            let mut argv = vec!["useradd", "-m", "-s", u.shell.as_str()];
            if u.sudo {
                argv.extend(["-G", "wheel"]);
            }
            argv.push(name);
            p.add("Create the user", chroot(&argv))
                .add(
                    "Copy the Hyprland and ii config from the live user",
                    host(&["cp", "-a", &format!("/home/{LIVE_USER}/.config/."), &t(&format!("{home}/.config/"))]),
                )
                .add("Give the user their files", chroot(&["chown", "-R", &format!("{name}:{name}"), &home]))
                .add("Point configs at the new home", fix_paths);
        }
    }

    // One keyboard layout for the console and the desktop.
    p.add(
        "Desktop keyboard layout",
        Action::SetDesktopLayout {
            path: t(&format!("{home}/{HYPR_INPUT_CONFIG}")),
            layout: xkb_from_console(&cfg.system.keymap),
        },
    );
    p.add("Set the password", Action::SetPassword { user: name.to_string() });
    if u.sudo {
        p.add("Let wheel use sudo (with a password)", write("/etc/sudoers.d/10-wheel", "%wheel ALL=(ALL:ALL) ALL\n", 0o440));
    }
}

// ---------------------------------------------------------------- actions

/// Write a file inside the target. A symlink at `path` is replaced, never
/// followed: an absolute link would otherwise point at the live system.
pub fn write_file(path: &str, contents: &str, mode: u32) -> Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::remove_file(path)?;
    }
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {mode:o} {}", path.display()))
}

pub fn edit_file(path: &str, edit: impl FnOnce(&str) -> String) -> Result<()> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let new = edit(&text);
    if new != text {
        fs::write(path, new).with_context(|| format!("writing {path}"))?;
    }
    Ok(())
}

/// Uncomment `wanted` (e.g. "#en_US.UTF-8 UTF-8" -> "en_US.UTF-8 UTF-8"), or append it.
pub fn uncomment(text: &str, wanted: &str) -> String {
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            if !found && line.trim_start_matches('#').trim() == wanted {
                found = true;
                wanted.to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(wanted.to_string());
    }
    lines.join("\n") + "\n"
}

/// Set `key=value` in a shell-style file, replacing the first (possibly
/// commented) assignment of `key`, or appending one.
pub fn set_key_value(text: &str, key: &str, value: &str) -> String {
    let assign = format!("{key}=");
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            if !found && line.trim_start().trim_start_matches('#').trim_start().starts_with(&assign) {
                found = true;
                format!("{key}={value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(format!("{key}={value}"));
    }
    lines.join("\n") + "\n"
}

/// Run a command and fail if a non-comment line of its output mentions `needle`.
pub fn expect_absent(cmd: &Cmd, needle: &str) -> Result<()> {
    let argv = cmd.argv();
    let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    let out = run::capture(&argv[0], &args)?;
    let hits = matching_lines(&out, needle);
    if hits.is_empty() {
        Ok(())
    } else {
        bail!("found \"{needle}\" in the output of `{cmd}`:\n{}", hits.join("\n"))
    }
}

fn matching_lines<'a>(text: &'a str, needle: &str) -> Vec<&'a str> {
    text.lines().filter(|l| !l.trim_start().starts_with('#') && l.contains(needle)).collect()
}

/// Replace `from` with `to` in text files and symlink targets under `dir`.
/// Never follows symlinks, skips binary and large files, keeps owner and mode.
pub fn replace_in_tree(dir: &Path, from: &str, to: &str) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    if fs::symlink_metadata(dir).is_err() {
        return Ok(changed);
    }
    walk(dir, &mut |path, meta| {
        let ft = meta.file_type();
        if ft.is_symlink() {
            let target = fs::read_link(path)?;
            let target = target.to_string_lossy();
            if target.contains(from) {
                use std::os::unix::fs::MetadataExt;
                fs::remove_file(path)?;
                symlink(target.replace(from, to), path)?;
                lchown(path, Some(meta.uid()), Some(meta.gid()))?;
                changed.push(path.to_path_buf());
            }
        } else if ft.is_file() && meta.len() <= REPLACE_MAX_BYTES {
            let bytes = fs::read(path)?;
            if let Ok(text) = std::str::from_utf8(&bytes) {
                if text.contains(from) {
                    // Writing in place keeps the inode, so owner and mode stay.
                    fs::write(path, text.replace(from, to))?;
                    changed.push(path.to_path_buf());
                }
            }
        }
        Ok(())
    })
    .with_context(|| format!("rewriting paths under {}", dir.display()))?;
    Ok(changed)
}

/// Depth-first walk that never follows symlinks.
pub fn walk(dir: &Path, f: &mut dyn FnMut(&Path, &fs::Metadata) -> std::io::Result<()>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            walk(&path, f)?;
        } else {
            f(&path, &meta)?;
        }
    }
    Ok(())
}

/// Check the configured values exist under `root` (the live system, which has
/// the same files as the image we unpack).
pub fn check_system_values(root: &Path, timezone: &str, locale: &str, keymap: &str) -> Result<()> {
    let errs: Vec<String> = [check_timezone(root, timezone), check_locale(root, locale), check_keymap(root, keymap)]
        .into_iter()
        .filter_map(|r| r.err().map(|e| e.to_string()))
        .collect();
    if errs.is_empty() { Ok(()) } else { bail!("{}", errs.join("; ")) }
}

pub fn check_timezone(root: &Path, timezone: &str) -> Result<()> {
    if root.join("usr/share/zoneinfo").join(timezone).is_file() {
        Ok(())
    } else {
        bail!("unknown timezone {timezone:?} (see /usr/share/zoneinfo)")
    }
}

pub fn check_locale(root: &Path, locale: &str) -> Result<()> {
    let (lang, charset) = locale.split_once('.').unwrap_or((locale, ""));
    if root.join("usr/share/i18n/locales").join(lang).is_file()
        && root.join("usr/share/i18n/charmaps").join(format!("{charset}.gz")).is_file()
    {
        Ok(())
    } else {
        bail!("unknown locale {locale:?} (see /usr/share/i18n/SUPPORTED)")
    }
}

pub fn check_keymap(root: &Path, keymap: &str) -> Result<()> {
    if keymap_exists(&root.join("usr/share/kbd/keymaps"), keymap) {
        Ok(())
    } else {
        bail!("unknown keymap {keymap:?} (see `localectl list-keymaps`)")
    }
}

fn keymap_exists(dir: &Path, keymap: &str) -> bool {
    let names = [format!("{keymap}.map.gz"), format!("{keymap}.map")];
    let mut found = false;
    let _ = walk(dir, &mut |path, _| {
        if path.file_name().is_some_and(|n| names.iter().any(|w| n == w.as_str())) {
            found = true;
        }
        Ok(())
    });
    found
}

// ---------------------------------------------------------------- keyboard

/// Desktop (XKB) layout for a console keymap: "de-latin1" -> "de", "uk" -> "gb".
pub fn xkb_from_console(keymap: &str) -> String {
    let base = keymap.split('-').next().unwrap_or(keymap);
    match base {
        "uk" => "gb".into(),
        other => other.into(),
    }
}

/// Console keymap for a desktop (XKB) layout: "gb" -> "uk".
pub fn console_from_xkb(layout: &str) -> String {
    match layout {
        "gb" => "uk".into(),
        other => other.into(),
    }
}

/// The value of `kb_layout = "..."` in Hyprland's Lua config, if there is one.
pub fn lua_kb_layout(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("kb_layout")?.trim_start().strip_prefix('=')?;
        let value = rest.trim().trim_end_matches(',').trim();
        Some(value.strip_prefix('"')?.strip_suffix('"')?.to_string())
    })
}

/// Rewrite the `kb_layout = "..."` line, keeping its indent and trailing comma.
pub fn set_lua_kb_layout(text: &str, layout: &str) -> Option<String> {
    let mut found = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            if !found && lua_kb_layout(line).is_some() {
                found = true;
                let indent = &line[..line.len() - line.trim_start().len()];
                let comma = if line.trim_end().ends_with(',') { "," } else { "" };
                format!("{indent}kb_layout = \"{layout}\"{comma}")
            } else {
                line.to_string()
            }
        })
        .collect();
    found.then(|| lines.join("\n") + if text.ends_with('\n') { "\n" } else { "" })
}

/// Set the desktop layout in the user's Hyprland config. A missing file or
/// line is a warning, not a failure: the system still boots and works.
pub fn set_desktop_layout(path: &str, layout: &str) -> Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        run::say(&format!("warning: {path} not found, desktop keyboard layout left alone"));
        run::log(&format!("no {path}, desktop layout not set"));
        return Ok(());
    };
    match set_lua_kb_layout(&text, layout) {
        Some(new) => {
            if new != text {
                fs::write(path, new).with_context(|| format!("writing {path}"))?;
            }
            run::log(&format!("desktop layout {layout} in {path}"));
        }
        None => {
            run::say(&format!("warning: no kb_layout line in {path}, desktop keyboard layout left alone"));
            run::log(&format!("no kb_layout line in {path}"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- password

/// Ask for the password: typed twice on the terminal with echo off, or one
/// line from stdin with `--password-stdin` (for scripted installs).
pub fn ask_password(user: &str, from_stdin: bool) -> Result<String> {
    if from_stdin {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).context("reading the password from stdin")?;
        let pw = line.trim_end_matches(['\n', '\r']).to_string();
        validate_password(&pw)?;
        return Ok(pw);
    }

    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("no terminal to ask for the password on (use --password-stdin)")?;
    for _ in 0..3 {
        let first = prompt_hidden(&tty, &format!("  Password for {user}: "))?;
        if let Err(e) = validate_password(&first) {
            writeln!(&tty, "  {e}, try again")?;
            continue;
        }
        let second = prompt_hidden(&tty, "  Same again: ")?;
        if first == second {
            return Ok(first);
        }
        writeln!(&tty, "  They did not match, try again")?;
    }
    bail!("no password set after 3 tries, nothing was touched")
}

fn validate_password(pw: &str) -> Result<()> {
    if pw.is_empty() {
        bail!("the password cannot be empty");
    }
    if pw.contains(['\n', '\r', '\0']) {
        bail!("the password cannot contain line breaks");
    }
    Ok(())
}

/// Turns terminal echo off and back on when dropped.
struct EchoOff;

impl EchoOff {
    fn new() -> Result<Self> {
        stty("-echo")?;
        Ok(Self)
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        let _ = stty("echo");
    }
}

fn stty(setting: &str) -> Result<()> {
    let tty = File::open("/dev/tty")?;
    let status = std::process::Command::new("stty").arg(setting).stdin(tty).status()?;
    if !status.success() {
        bail!("stty {setting} failed");
    }
    Ok(())
}

fn prompt_hidden(tty: &File, prompt: &str) -> Result<String> {
    let mut out = tty;
    write!(out, "{prompt}")?;
    out.flush()?;
    let line = {
        let _echo = EchoOff::new()?;
        let mut line = String::new();
        BufReader::new(tty).read_line(&mut line)?;
        line
    };
    writeln!(out)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// Feed `user:password` to chpasswd inside the target, on stdin only.
pub fn set_password(user: &str, password: &str) -> Result<()> {
    run::capture_with_stdin("arch-chroot", &[TARGET, "chpasswd"], format!("{user}:{password}\n").as_bytes())
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arctix-cfg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn uncomments_the_locale() {
        let locale_gen = "# comment\n#en_GB.UTF-8 UTF-8\n#en_US.UTF-8 UTF-8  \n#en_US ISO-8859-1\n";
        let out = uncomment(locale_gen, "en_US.UTF-8 UTF-8");
        assert!(out.contains("\nen_US.UTF-8 UTF-8\n"));
        assert!(out.contains("#en_GB.UTF-8 UTF-8"));
        assert!(out.contains("#en_US ISO-8859-1"));
        assert_eq!(uncomment("x\n", "da_DK.UTF-8 UTF-8"), "x\nda_DK.UTF-8 UTF-8\n");
    }

    #[test]
    fn sets_grub_distributor() {
        let grub = "GRUB_DEFAULT=0\nGRUB_DISTRIBUTOR=\"Arch\"\nGRUB_CMDLINE_LINUX=\"\"\n";
        let out = set_key_value(grub, "GRUB_DISTRIBUTOR", "\"Arctix\"");
        assert_eq!(out, "GRUB_DEFAULT=0\nGRUB_DISTRIBUTOR=\"Arctix\"\nGRUB_CMDLINE_LINUX=\"\"\n");
        assert_eq!(set_key_value("#FOO=1\n", "FOO", "2"), "FOO=2\n");
        assert_eq!(set_key_value("A=1\n", "FOO", "2"), "A=1\nFOO=2\n");
    }

    #[test]
    fn write_file_replaces_symlinks_instead_of_following_them() {
        let dir = scratch("symlink");
        let outside = dir.join("outside");
        fs::write(&outside, "live system file").unwrap();
        let link = dir.join("etc/hostname");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(&outside, &link).unwrap();

        write_file(link.to_str().unwrap(), "arctix\n", 0o644).unwrap();
        assert_eq!(fs::read_to_string(&outside).unwrap(), "live system file");
        assert_eq!(fs::read_to_string(&link).unwrap(), "arctix\n");
        assert!(!fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_file_sets_the_mode() {
        let dir = scratch("mode");
        let path = dir.join("sudoers.d/10-wheel");
        write_file(path.to_str().unwrap(), "x\n", 0o440).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o440);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replaces_home_paths_in_text_and_links_only() {
        let dir = scratch("replace");
        let cfg = dir.join("ii/config.json");
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs::write(&cfg, r#"{"wallpaperPath": "/home/arctix/Pictures/bg.jpg", "savePath": "/home/arctix/Videos"}"#).unwrap();
        fs::set_permissions(&cfg, fs::Permissions::from_mode(0o600)).unwrap();
        let bin = dir.join("blob.bin");
        fs::write(&bin, b"\xff\xfe/home/arctix\x00").unwrap();
        let untouched = dir.join("other.txt");
        fs::write(&untouched, "nothing here").unwrap();
        symlink("/home/arctix/Pictures", dir.join("pics")).unwrap();
        symlink("relative-target", dir.join("rel")).unwrap();

        let mut changed = replace_in_tree(&dir, "/home/arctix", "/home/cammy").unwrap();
        changed.sort();
        assert_eq!(changed, [cfg.clone(), dir.join("pics")]);
        assert_eq!(
            fs::read_to_string(&cfg).unwrap(),
            r#"{"wallpaperPath": "/home/cammy/Pictures/bg.jpg", "savePath": "/home/cammy/Videos"}"#
        );
        assert_eq!(fs::metadata(&cfg).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read(&bin).unwrap(), b"\xff\xfe/home/arctix\x00");
        assert_eq!(fs::read_link(dir.join("pics")).unwrap(), Path::new("/home/cammy/Pictures"));
        assert_eq!(fs::read_link(dir.join("rel")).unwrap(), Path::new("relative-target"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replace_in_missing_dir_is_a_no_op() {
        assert!(replace_in_tree(Path::new("/no/such/dir"), "a", "b").unwrap().is_empty());
    }

    #[test]
    fn checks_system_values_against_a_root() {
        let root = scratch("values");
        for f in [
            "usr/share/zoneinfo/Europe/Copenhagen",
            "usr/share/i18n/locales/en_US",
            "usr/share/i18n/charmaps/UTF-8.gz",
            "usr/share/kbd/keymaps/i386/qwerty/us.map.gz",
        ] {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "").unwrap();
        }
        assert!(check_system_values(&root, "Europe/Copenhagen", "en_US.UTF-8", "us").is_ok());
        let err = check_system_values(&root, "Europe/Kobenhavn", "en_XX.UTF-8", "zz").unwrap_err().to_string();
        assert!(err.contains("timezone") && err.contains("locale") && err.contains("keymap"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn console_and_desktop_layout_names() {
        assert_eq!(xkb_from_console("dk"), "dk");
        assert_eq!(xkb_from_console("de-latin1"), "de");
        assert_eq!(xkb_from_console("uk"), "gb");
        assert_eq!(xkb_from_console("se-lat6"), "se");
        assert_eq!(console_from_xkb("gb"), "uk");
        assert_eq!(console_from_xkb("dk"), "dk");
    }

    #[test]
    fn rewrites_the_hyprland_layout_line_only() {
        let lua = "hl.config({\n    input = {\n        kb_layout = \"dk\",\n        numlock_by_default = true,\n    },\n})\n";
        assert_eq!(lua_kb_layout(lua).as_deref(), Some("dk"));
        let out = set_lua_kb_layout(lua, "de").unwrap();
        assert_eq!(out, lua.replace("\"dk\"", "\"de\""));
        assert_eq!(lua_kb_layout(&out).as_deref(), Some("de"));
        assert!(set_lua_kb_layout("input = {}\n", "de").is_none());
        // kb_layout_variant and friends are not the layout line
        assert!(lua_kb_layout("    kb_layoutx = \"us\",\n").is_none());
    }

    #[test]
    fn passwords_are_validated() {
        assert!(validate_password("hunter2").is_ok());
        assert!(validate_password("with:colon").is_ok());
        assert!(validate_password("").is_err());
        assert!(validate_password("a\nb").is_err());
    }

    #[test]
    fn expect_absent_ignores_comments() {
        let conf = "# no archiso here please\nHOOKS=(base udev archiso block)\n";
        assert_eq!(matching_lines(conf, "archiso"), ["HOOKS=(base udev archiso block)"]);
        assert!(matching_lines(MKINITCPIO_CONF, "archiso").is_empty());
    }

    #[test]
    fn initramfs_config_has_no_archiso() {
        let hooks = MKINITCPIO_CONF.lines().find(|l| l.starts_with("HOOKS=")).unwrap();
        assert!(!hooks.contains("archiso"));
        assert!(hooks.contains("autodetect") && hooks.contains("fsck"));
        assert!(LINUX_LTS_PRESET.contains("ALL_config=\"/etc/mkinitcpio.conf\""));
    }
}
