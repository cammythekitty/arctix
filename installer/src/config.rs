//! The install config: one TOML file, shared by the CLI and (later) the UI.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub disk: DiskConfig,
    #[serde(default)]
    pub install: InstallConfig,
    #[serde(default)]
    pub system: SystemConfig,
    pub user: UserConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskConfig {
    /// Required on purpose: the target disk is never guessed.
    pub device: String,
    #[serde(default)]
    pub filesystem: Filesystem,
    #[serde(default)]
    pub luks: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    #[default]
    Ext4,
    Btrfs,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InstallConfig {
    pub mode: Mode,
    pub package_set: Option<String>,
    pub update: bool,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self { mode: Mode::Copy, package_set: None, update: true }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Copy,
    Pacstrap,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemConfig {
    pub hostname: String,
    pub timezone: String,
    pub locale: String,
    pub keymap: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            hostname: "arctix".into(),
            timezone: "UTC".into(),
            locale: "en_US.UTF-8".into(),
            keymap: "us".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub name: String,
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default = "yes")]
    pub sudo: bool,
}

fn default_shell() -> String {
    "/usr/bin/fish".into()
}

fn yes() -> bool {
    true
}

/// System accounts a user must never be named after.
pub const RESERVED_USERS: &[&str] = &[
    "root", "bin", "daemon", "mail", "ftp", "http", "nobody", "dbus", "polkitd", "git", "alpm",
];

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read config {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let cfg = Self::parse_unvalidated(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Parse the TOML and check its shape, but not the values.
    pub fn parse_unvalidated(text: &str) -> Result<Self> {
        let cfg: Config = toml::from_str(text).map_err(|e| {
            if e.message().contains("`password`") {
                anyhow!("passwords never go in the config file, the installer asks for it")
            } else {
                anyhow!("invalid config: {e}")
            }
        })?;
        Ok(cfg)
    }

    /// Collects every problem at once so the user can fix them in one go.
    pub fn validate(&self) -> Result<()> {
        let errs = self.problems();
        if errs.is_empty() {
            Ok(())
        } else {
            bail!("config has problems:\n  - {}", errs.join("\n  - "))
        }
    }

    /// Every problem with the values, one message each (empty means valid).
    pub fn problems(&self) -> Vec<String> {
        let mut errs: Vec<String> = Vec::new();

        let dev = &self.disk.device;
        if !dev.starts_with("/dev/") || dev.len() <= "/dev/".len() {
            errs.push(format!("disk.device must be a path under /dev, got {dev:?}"));
        } else if dev.contains("..") || dev.chars().any(char::is_whitespace) {
            errs.push(format!("disk.device looks wrong: {dev:?}"));
        }
        if self.disk.filesystem == Filesystem::Btrfs {
            errs.push("disk.filesystem = \"btrfs\" is not supported yet, use \"ext4\"".into());
        }
        if self.disk.luks {
            errs.push("disk.luks = true is not supported yet".into());
        }

        if self.install.mode == Mode::Pacstrap {
            match &self.install.package_set {
                None => errs.push("install.package_set is required in pacstrap mode".into()),
                Some(set) if !is_simple_name(set) => {
                    errs.push(format!("install.package_set {set:?} should be a plain name like \"desktop\""))
                }
                Some(_) => {}
            }
        }

        let sys = &self.system;
        if !valid_hostname(&sys.hostname) {
            errs.push(format!(
                "system.hostname {:?} must be 1-63 letters, digits or '-', not starting or ending with '-'",
                sys.hostname
            ));
        }
        if !valid_timezone(&sys.timezone) {
            errs.push(format!("system.timezone {:?} should look like \"Europe/Copenhagen\"", sys.timezone));
        }
        if !valid_locale(&sys.locale) {
            errs.push(format!("system.locale {:?} should look like \"en_US.UTF-8\"", sys.locale));
        }
        if sys.keymap.is_empty()
            || !sys.keymap.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
        {
            errs.push(format!("system.keymap {:?} looks wrong", sys.keymap));
        }

        let user = &self.user;
        if !valid_username(&user.name) {
            errs.push(format!(
                "user.name {:?} must start with a lowercase letter or '_', then up to 31 lowercase letters, digits, '_' or '-'",
                user.name
            ));
        } else if RESERVED_USERS.contains(&user.name.as_str()) || user.name.starts_with("systemd-") {
            errs.push(format!("user.name {:?} is a system account name", user.name));
        }
        if !user.shell.starts_with('/') || user.shell.chars().any(char::is_whitespace) {
            errs.push(format!("user.shell must be an absolute path, got {:?}", user.shell));
        }

        errs
    }

    /// The line to uncomment in /etc/locale.gen, e.g. "en_US.UTF-8 UTF-8".
    pub fn locale_gen_line(&self) -> String {
        let locale = &self.system.locale;
        let charset = locale.split_once('.').map_or("UTF-8", |(_, c)| c);
        format!("{locale} {charset}")
    }
}

fn is_simple_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
}

pub fn valid_hostname(s: &str) -> bool {
    (1..=63).contains(&s.len())
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn valid_timezone(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.ends_with('/')
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_+-/".contains(c))
}

/// Only plain `xx_YY.CHARSET` locales for now, no `@modifier` variants.
pub fn valid_locale(s: &str) -> bool {
    match s.split_once('.') {
        Some((lang, charset)) => {
            !lang.is_empty()
                && !charset.is_empty()
                && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && charset.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        }
        None => false,
    }
}

pub fn valid_username(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    s.len() <= 32
        && (first.is_ascii_lowercase() || first == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../examples/arctix.toml");

    fn with(find: &str, replace: &str) -> String {
        assert!(SAMPLE.contains(find), "sample has no {find:?}");
        SAMPLE.replacen(find, replace, 1)
    }

    fn err_of(text: &str) -> String {
        format!("{:#}", Config::parse(text).expect_err("config should be rejected"))
    }

    #[test]
    fn sample_config_is_valid() {
        let cfg = Config::parse(SAMPLE).unwrap();
        assert_eq!(cfg.disk.device, "/dev/vda");
        assert_eq!(cfg.install.mode, Mode::Copy);
        assert_eq!(cfg.user.name, "cammy");
        assert_eq!(cfg.locale_gen_line(), "en_US.UTF-8 UTF-8");
    }

    #[test]
    fn minimal_config_gets_defaults() {
        let cfg = Config::parse("[disk]\ndevice = \"/dev/sda\"\n[user]\nname = \"a\"\n").unwrap();
        assert_eq!(cfg.install.mode, Mode::Copy);
        assert!(cfg.install.update);
        assert_eq!(cfg.system.timezone, "UTC");
        assert_eq!(cfg.user.shell, "/usr/bin/fish");
        assert!(cfg.user.sudo);
    }

    #[test]
    fn device_is_required() {
        assert!(err_of("[disk]\n[user]\nname = \"a\"\n").contains("device"));
    }

    #[test]
    fn device_must_be_under_dev() {
        assert!(err_of(&with("/dev/vda", "vda")).contains("disk.device"));
        assert!(err_of(&with("/dev/vda", "/dev/")).contains("disk.device"));
        assert!(err_of(&with("/dev/vda", "/dev/../etc/passwd")).contains("disk.device"));
    }

    #[test]
    fn unsupported_features_are_rejected() {
        assert!(err_of(&with("\"ext4\"", "\"btrfs\"")).contains("btrfs"));
        assert!(err_of(&with("luks = false", "luks = true")).contains("luks"));
        assert!(err_of(&with("\"ext4\"", "\"zfs\"")).contains("invalid config"));
    }

    #[test]
    fn pacstrap_needs_a_package_set() {
        let text = with("mode = \"copy\"", "mode = \"pacstrap\"")
            .replace("package_set = \"desktop\"      # pacstrap mode only\n", "");
        assert!(err_of(&text).contains("package_set is required"));
        let ok = with("mode = \"copy\"", "mode = \"pacstrap\"");
        assert!(Config::parse(&ok).is_ok());
    }

    #[test]
    fn bad_usernames_are_rejected() {
        for bad in ["Cammy", "1cammy", "root", "systemd-foo", "", "cam my"] {
            let text = with("name = \"cammy\"", &format!("name = {bad:?}"));
            assert!(err_of(&text).contains("user.name"), "{bad:?} should be rejected");
        }
        for good in ["cammy", "_svc", "c-1"] {
            let text = with("name = \"cammy\"", &format!("name = {good:?}"));
            assert!(Config::parse(&text).is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn bad_system_values_are_rejected() {
        assert!(err_of(&with("hostname = \"arctix\"", "hostname = \"-bad\"")).contains("hostname"));
        assert!(err_of(&with("Europe/Copenhagen", "../etc/shadow")).contains("timezone"));
        assert!(err_of(&with("en_US.UTF-8", "en_US")).contains("locale"));
        assert!(err_of(&with("keymap = \"us\"", "keymap = \"u s\"")).contains("keymap"));
    }

    #[test]
    fn all_problems_are_reported_together() {
        let text = with("hostname = \"arctix\"", "hostname = \"-bad\"").replace("cammy", "Root");
        let err = err_of(&text);
        assert!(err.contains("hostname") && err.contains("user.name"));
    }

    #[test]
    fn password_in_config_is_refused() {
        let text = with("sudo = true", "sudo = true\npassword = \"hunter2\"");
        assert!(err_of(&text).contains("passwords never go in the config"));
    }

    #[test]
    fn unknown_keys_are_refused() {
        assert!(err_of(&with("sudo = true", "sudo = true\nsudoo = true")).contains("sudoo"));
    }
}
