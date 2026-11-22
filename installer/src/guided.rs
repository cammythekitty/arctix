//! Guided mode for the live desktop launcher: asks a few questions with
//! sensible defaults, writes the same TOML the CLI reads, and hands over to
//! the normal install. Nothing here touches a disk.

use crate::config::{self, Config};
use crate::configure::{check_keymap, check_locale, check_timezone};
use crate::disks;
use crate::partition;
use anyhow::{Context, Result, bail};
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Where the guided answers are saved. Re-run them later with
/// `sudo arctix-install install /tmp/arctix-install.toml`.
pub const GUIDED_CONFIG: &str = "/tmp/arctix-install.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    pub device: String,
    pub username: String,
    pub hostname: String,
    pub timezone: String,
    pub locale: String,
    pub keymap: String,
    pub update: bool,
}

/// The config file for these answers. Values are validated before they get
/// here and only use characters that need no escaping inside TOML strings.
pub fn render_toml(a: &Answers) -> String {
    format!(
        "# Written by arctix-install guided mode.\n\
         \n\
         [disk]\n\
         device = \"{}\"\n\
         filesystem = \"ext4\"\n\
         \n\
         [install]\n\
         mode = \"copy\"\n\
         update = {}\n\
         \n\
         [system]\n\
         hostname = \"{}\"\n\
         timezone = \"{}\"\n\
         locale = \"{}\"\n\
         keymap = \"{}\"\n\
         \n\
         [user]\n\
         name = \"{}\"\n\
         shell = \"/usr/bin/fish\"\n\
         sudo = true\n",
        a.device, a.update, a.hostname, a.timezone, a.locale, a.keymap, a.username
    )
}

/// Ask the questions, save the config, return its path.
pub fn run() -> Result<PathBuf> {
    if !Path::new(partition::LIVE_MARKER).exists() {
        bail!("the installer only runs from the Arctix live ISO");
    }
    if !partition::is_root()? {
        bail!("the installer needs root, start it with sudo");
    }

    println!();
    println!("  Install Arctix");
    println!("  --------------");
    println!("  A few questions first. Press Enter to take the default in [brackets].");
    println!("  Nothing on any disk changes until you type the disk's name to confirm.");
    println!();

    let device = pick_disk()?;
    let root = Path::new("/");
    let username = ask("Username", None, |v| {
        if !config::valid_username(v) {
            Err("lowercase letters, digits, '_' or '-', starting with a letter".into())
        } else if config::RESERVED_USERS.contains(&v) || v.starts_with("systemd-") {
            Err("that name belongs to a system account".into())
        } else {
            Ok(())
        }
    })?;
    let hostname = ask("Computer name", Some("arctix"), |v| {
        if config::valid_hostname(v) { Ok(()) } else { Err("letters, digits and '-', up to 63".into()) }
    })?;
    let timezone = ask("Timezone (like Europe/Copenhagen)", Some(&crate::options::live_timezone()), |v| {
        if !config::valid_timezone(v) {
            return Err("should look like Europe/Copenhagen".into());
        }
        check_timezone(root, v).map_err(|e| e.to_string())
    })?;
    let keymap = ask("Keyboard layout, for console and desktop", Some(&crate::options::live_keymap()), |v| {
        check_keymap(root, v).map_err(|e| e.to_string())
    })?;
    let locale = ask("Language", Some("en_US.UTF-8"), |v| {
        if !config::valid_locale(v) {
            return Err("should look like en_US.UTF-8".into());
        }
        check_locale(root, v).map_err(|e| e.to_string())
    })?;
    let update = ask("Update to the newest packages if online? (y/n)", Some("y"), |v| match v {
        "y" | "n" | "yes" | "no" => Ok(()),
        _ => Err("y or n".into()),
    })?
    .starts_with('y');

    let answers = Answers { device, username, hostname, timezone, locale, keymap, update };
    let toml = render_toml(&answers);
    // Same validation the CLI does, so guided and CLI installs cannot drift.
    Config::parse(&toml).context("the guided answers did not make a valid config")?;

    std::fs::write(GUIDED_CONFIG, &toml).with_context(|| format!("writing {GUIDED_CONFIG}"))?;
    std::fs::set_permissions(GUIDED_CONFIG, std::fs::Permissions::from_mode(0o600))?;

    println!();
    println!("  Disk:      {} (everything on it will be erased)", answers.device);
    println!("  User:      {} (with sudo)", answers.username);
    println!("  Computer:  {}", answers.hostname);
    println!("  Timezone:  {}   Keyboard: {}   Language: {}", answers.timezone, answers.keymap, answers.locale);
    println!("  Update:    {}", if answers.update { "yes, if online" } else { "no" });
    println!("  Saved to {GUIDED_CONFIG}");
    Ok(PathBuf::from(GUIDED_CONFIG))
}

fn pick_disk() -> Result<String> {
    let devices = disks::lsblk()?;
    let live = disks::live_medium_disk()?;
    let infos = disks::disk_infos(&devices, live.as_deref());
    let usable: Vec<&disks::DiskInfo> = infos.iter().filter(|d| d.usable).collect();

    println!("  Disks:");
    for (i, d) in usable.iter().enumerate() {
        let model = d.model.as_deref().unwrap_or("");
        println!("    {}) {:<14} {:>10}  {}", i + 1, d.path, d.size_human, model);
    }
    for d in infos.iter().filter(|d| !d.usable) {
        println!("    -) {:<14} {:>10}  not usable: {}", d.path, d.size_human, d.reason.as_deref().unwrap_or("?"));
    }
    println!();
    if usable.is_empty() {
        bail!("no disk can be installed to");
    }

    let default = if usable.len() == 1 { Some("1") } else { None };
    let answer = ask("Install to disk number", default, |v| {
        match v.parse::<usize>() {
            Ok(n) if (1..=usable.len()).contains(&n) => Ok(()),
            _ if usable.iter().any(|d| d.path == v) => Ok(()),
            _ => Err(format!("pick a number from 1 to {}", usable.len())),
        }
    })?;
    Ok(match answer.parse::<usize>() {
        Ok(n) => usable[n - 1].path.clone(),
        Err(_) => answer,
    })
}

/// Ask until `check` accepts the answer. An empty answer takes the default.
fn ask(prompt: &str, default: Option<&str>, check: impl Fn(&str) -> Result<(), String>) -> Result<String> {
    let stdin = std::io::stdin();
    loop {
        match default {
            Some(d) => print!("  {prompt} [{d}]: "),
            None => print!("  {prompt}: "),
        }
        std::io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            bail!("input closed, nothing was changed");
        }
        let value = match (line.trim(), default) {
            ("", Some(d)) => d.to_string(),
            ("", None) => {
                println!("    this one needs an answer");
                continue;
            }
            (v, _) => v.to_string(),
        };
        match check(&value) {
            Ok(()) => return Ok(value),
            Err(why) => println!("    {why}, try again"),
        }
    }
}

/// After the install: offer a reboot, or keep the window open until Enter.
pub fn finish(result: &Result<()>) {
    println!();
    let reboot = result.is_ok()
        && matches!(prompt_line("  Reboot now? Remove the install medium first. [y/N]: ").as_deref(), Some("y" | "yes"));
    if reboot {
        let _ = std::process::Command::new("systemctl").arg("reboot").status();
    } else if result.is_err() {
        prompt_line("  Press Enter to close this window.");
    }
}

fn prompt_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    std::io::stdout().flush().ok()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    Some(line.trim().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers() -> Answers {
        Answers {
            device: "/dev/nvme0n1".into(),
            username: "cammy".into(),
            hostname: "arctix".into(),
            timezone: "Europe/Copenhagen".into(),
            locale: "en_US.UTF-8".into(),
            keymap: "dk".into(),
            update: false,
        }
    }

    #[test]
    fn guided_toml_parses_into_the_same_config() {
        let cfg = Config::parse(&render_toml(&answers())).unwrap();
        assert_eq!(cfg.disk.device, "/dev/nvme0n1");
        assert_eq!(cfg.user.name, "cammy");
        assert!(cfg.user.sudo);
        assert_eq!(cfg.system.timezone, "Europe/Copenhagen");
        assert_eq!(cfg.system.keymap, "dk");
        assert!(!cfg.install.update);
        assert_eq!(cfg.install.mode, config::Mode::Copy);
    }

    #[test]
    fn guided_toml_never_contains_a_password() {
        assert!(!render_toml(&answers()).to_lowercase().contains("password"));
    }
}
