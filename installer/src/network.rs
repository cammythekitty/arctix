//! "Are we online?" for the steps that need the internet (mirrors, update).
//! Online means the Arch mirrors answer over HTTPS, not just that a link is up:
//! a cable with no internet behind it must count as offline, and fast.

use crate::run;

/// Tried in order; any one answering is enough.
const PROBES: &[&str] = &["https://geo.mirror.pkgbuild.com/", "https://archlinux.org/"];

pub fn is_online() -> bool {
    PROBES.iter().any(|url| probe(url))
}

fn probe(url: &str) -> bool {
    run::capture(
        "curl",
        &["--fail", "--silent", "--show-error", "--output", "/dev/null", "--connect-timeout", "5", "--max-time", "10", url],
    )
    .is_ok()
}
