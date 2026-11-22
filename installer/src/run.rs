//! Every external command goes through here: output is captured, logged,
//! and a failure turns into an error that names the command and its stderr.
//! Also home to the executor that walks a plan's Steps.

use crate::{bootloader, cleanup, configure, network, partition, populate};
use crate::plan::{Action, Stage, Step, When};
use anyhow::{Context, Result, bail};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const LOG_PATH: &str = "/tmp/arctix-install.log";

/// `--progress json`: progress goes to stdout as one JSON object per line,
/// for the graphical installer. Otherwise it is plain text for people.
static JSON_PROGRESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_json_progress(on: bool) {
    JSON_PROGRESS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn json_progress() -> bool {
    JSON_PROGRESS.load(std::sync::atomic::Ordering::Relaxed)
}

fn emit(value: serde_json::Value) {
    println!("{value}");
}

/// A note about the current step ("kernel from ...", "skipped (offline)").
pub fn say(text: &str) {
    if json_progress() {
        emit(serde_json::json!({"type": "note", "text": text}));
    } else {
        println!("  {text}");
    }
}

fn report_step(n: usize, total: usize, step: &Step) {
    if json_progress() {
        emit(serde_json::json!({
            "type": "step", "n": n, "total": total, "stage": step.stage.key(), "desc": step.desc,
        }));
    } else {
        report_step(n, total, step);
    }
}

/// The end of a run: success or the error that stopped it.
pub fn report_end(result: &Result<()>, done_message: &str) {
    if json_progress() {
        match result {
            Ok(()) => emit(serde_json::json!({"type": "done", "text": done_message})),
            Err(e) => emit(serde_json::json!({"type": "error", "text": format!("{e:#}")})),
        }
    } else if result.is_ok() {
        println!();
        println!("{done_message}");
    }
}

/// How long to wait for partition device nodes after udevadm settle.
const DEVICE_TIMEOUT: Duration = Duration::from_secs(15);

/// Append a line to the install log. Logging never fails the install, but if
/// it stops working we say so once instead of losing the log silently.
pub fn log(line: &str) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    if let Err(e) = open_log().and_then(|mut f| writeln!(f, "{line}")) {
        WARNED.call_once(|| eprintln!("warning: cannot write {LOG_PATH} ({e}), continuing without a log"));
    }
}

/// Opens the log without O_CREAT first: with fs.protected_regular (the default),
/// root may not O_CREAT-open a file in sticky /tmp that another user owns, which
/// happens when an earlier non-root run created the log.
fn open_log() -> std::io::Result<std::fs::File> {
    match OpenOptions::new().append(true).open(LOG_PATH) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new().create(true).append(true).open(LOG_PATH)
        }
        other => other,
    }
}

/// Run a command and return its stdout. A non-zero exit becomes an error.
pub fn capture(program: &str, args: &[&str]) -> Result<String> {
    run_cmd(program, args, None)
}

/// Like `capture`, but feeds `input` on stdin. The input is never logged,
/// which is how passwords reach chpasswd.
pub fn capture_with_stdin(program: &str, args: &[&str], input: &[u8]) -> Result<String> {
    run_cmd(program, args, Some(input))
}

fn run_cmd(program: &str, args: &[&str], input: Option<&[u8]>) -> Result<String> {
    let shown = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    log(&format!("$ {shown}{}", if input.is_some() { "  (stdin not logged)" } else { "" }));

    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start `{shown}` (is {program} installed?)"))?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().context("child has no stdin")?;
        stdin.write_all(input).with_context(|| format!("writing to `{shown}`"))?;
        // stdin is dropped here so the child sees EOF.
    }
    let out = child.wait_with_output().with_context(|| format!("waiting for `{shown}`"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stdout.trim().is_empty() {
        log(stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        log(&format!("stderr: {}", stderr.trim_end()));
    }
    if !out.status.success() {
        log(&format!("exit: {}", out.status));
        bail!("`{shown}` failed ({})\n{}", out.status, stderr.trim_end());
    }
    Ok(stdout)
}

pub struct ExecCtx {
    /// Skip the type-the-device confirmation.
    pub yes: bool,
    /// Read the password from stdin instead of the terminal.
    pub password_stdin: bool,
    /// Collected by AskPassword in preflight, used by SetPassword. Never logged.
    pub password: Option<String>,
    /// Network state, checked once at the first step that needs it.
    /// `Some(false)` up front means `--offline`.
    pub online: Option<bool>,
}

/// Run the steps in order. If anything fails after the disk was touched,
/// unmount the target before returning the error.
pub fn execute(steps: &[Step], ctx: &mut ExecCtx) -> Result<()> {
    let total = steps.len();
    let mut touched_disk = false;

    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        report_step(n, total, step);
        log(&format!("== [{n}/{total}] {} :: {} :: {}", step.stage, step.desc, step.action));

        touched_disk |= step.stage > Stage::Preflight;
        if let Err(e) = exec_step(step, ctx) {
            log(&format!("FAILED: {e:#}"));
            if touched_disk {
                partition::unmount_target();
            }
            return Err(e.context(format!("step {n} ({}) failed", step.desc)));
        }
    }
    Ok(())
}

fn exec_step(step: &Step, ctx: &mut ExecCtx) -> Result<()> {
    if !step.when.needs_network() {
        return exec_action(&step.action, ctx);
    }
    let online = *ctx.online.get_or_insert_with(|| {
        say("checking the network...");
        let up = network::is_online();
        log(&format!("network check: {}", if up { "online" } else { "offline" }));
        if !up {
            say("no internet, skipping mirrors and updates (the install still completes)");
        }
        up
    });
    if !online {
        say("skipped (offline)");
        log("skipped: offline");
        return Ok(());
    }
    match exec_action(&step.action, ctx) {
        Err(e) if step.when == When::OnlineBestEffort => {
            say("warning: this did not work, carrying on without it");
            say(&format!("({})", e.to_string().lines().next().unwrap_or_default()));
            log(&format!("best-effort step failed, continuing: {e:#}"));
            Ok(())
        }
        other => other,
    }
}

fn exec_action(action: &Action, ctx: &mut ExecCtx) -> Result<()> {
    match action {
        Action::CheckTarget { device } => partition::check_ready(device),
        Action::Confirm { device } => {
            if ctx.yes {
                // With JSON progress a front end already had the user confirm.
                say(if json_progress() { "confirmed in the installer window" } else { "--yes given, not asking" });
                log("confirmation skipped with --yes");
                Ok(())
            } else {
                partition::confirm(device)
            }
        }
        Action::Run(cmd) => {
            let argv = cmd.argv();
            let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
            capture(&argv[0], &args).map(drop)
        }
        Action::RunToFile { cmd, path } => {
            let argv = cmd.argv();
            let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
            let out = capture(&argv[0], &args)?;
            configure::write_file(path, &out, 0o644)
        }
        Action::WriteFile { path, contents, mode } => configure::write_file(path, contents, *mode),
        Action::Remove { path, recursive } => cleanup::remove_path(path, *recursive).map(drop),
        Action::RemoveMatching { dir, prefix } => cleanup::remove_matching(dir, prefix).map(drop),
        Action::RemoveUnit { dir, name } => cleanup::remove_unit(dir, name).map(drop),
        Action::RemoveNopasswdSudoers { dir } => {
            for path in cleanup::remove_nopasswd_sudoers(dir)? {
                say(&format!("removed {}", path.display()));
            }
            Ok(())
        }
        Action::UncommentLine { path, line } => configure::edit_file(path, |text| configure::uncomment(text, line)),
        Action::SetKeyValue { path, key, value } => {
            configure::edit_file(path, |text| configure::set_key_value(text, key, value))
        }
        Action::ReplaceInTree { dir, from, to } => {
            let changed = configure::replace_in_tree(Path::new(dir), from, to)?;
            for path in &changed {
                say(&format!("updated {}", path.display()));
                log(&format!("replaced {from} -> {to} in {}", path.display()));
            }
            Ok(())
        }
        Action::SetDesktopLayout { path, layout } => configure::set_desktop_layout(path, layout),
        Action::RemoveLines { path, line } => cleanup::remove_lines(path, line).map(drop),
        Action::CheckMbr { device } => bootloader::check_mbr(device),
        Action::FileContains { path, needles } => bootloader::file_contains(path, needles),
        Action::ExpectAbsent { cmd, needle } => configure::expect_absent(cmd, needle),
        Action::CheckSystemValues { timezone, locale, keymap } => {
            configure::check_system_values(Path::new("/"), timezone, locale, keymap)
        }
        Action::AskPassword { user } => {
            ctx.password = Some(configure::ask_password(user, ctx.password_stdin)?);
            Ok(())
        }
        Action::SetPassword { user } => {
            let pw = ctx.password.as_deref().context("no password was collected in preflight")?;
            configure::set_password(user, pw)
        }
        Action::WaitForPaths { paths } => wait_for_paths(paths, DEVICE_TIMEOUT),
        Action::EnsureFile { path } => populate::ensure_file(path),
        Action::InstallKernel { root, pkgbase } => {
            let src = populate::install_kernel(Path::new(root), pkgbase, Path::new(populate::LIVE_KERNEL))?;
            say(&format!("kernel from {}", src.display()));
            Ok(())
        }
        other => bail!("not implemented yet: {other}"),
    }
}

fn wait_for_paths(paths: &[String], timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        let missing: Vec<&str> = paths.iter().map(String::as_str).filter(|p| !Path::new(p).exists()).collect();
        if missing.is_empty() {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!("still missing after {}s: {}", timeout.as_secs(), missing.join(", "));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::plan::{Cmd, Stage, Step, When};

    fn step(when: When, argv: &[&str]) -> Step {
        Step { stage: Stage::Packages, desc: "test".into(), when, action: Action::Run(Cmd::new(argv, false)) }
    }

    fn ctx(online: Option<bool>) -> ExecCtx {
        ExecCtx { yes: true, password_stdin: false, password: None, online }
    }

    #[test]
    fn offline_skips_network_steps_without_running_them() {
        let mut c = ctx(Some(false));
        assert!(exec_step(&step(When::Online, &["false"]), &mut c).is_ok());
        assert!(exec_step(&step(When::OnlineBestEffort, &["false"]), &mut c).is_ok());
        // steps that do not need the network still run (and can still fail)
        assert!(exec_step(&step(When::Always, &["false"]), &mut c).is_err());
    }

    #[test]
    fn online_failures_are_fatal_unless_best_effort() {
        let mut c = ctx(Some(true));
        assert!(exec_step(&step(When::OnlineBestEffort, &["false"]), &mut c).is_ok());
        assert!(exec_step(&step(When::Online, &["false"]), &mut c).is_err());
        assert!(exec_step(&step(When::Online, &["true"]), &mut c).is_ok());
    }

    #[test]
    fn wait_for_paths_succeeds_and_times_out() {
        assert!(wait_for_paths(&["/".into()], Duration::from_millis(10)).is_ok());
        let err = wait_for_paths(&["/no/such/node".into()], Duration::from_millis(10)).unwrap_err();
        assert!(err.to_string().contains("/no/such/node"));
    }

    #[test]
    fn capture_reports_failures_with_the_command() {
        assert_eq!(capture("echo", &["hi"]).unwrap(), "hi\n");
        let err = capture("sh", &["-c", "echo boom >&2; exit 3"]).unwrap_err().to_string();
        assert!(err.contains("sh -c") && err.contains("boom"));
        assert!(capture("definitely-not-a-real-program", &[]).is_err());
    }
}
