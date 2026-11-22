mod bootloader;
mod cleanup;
mod config;
mod configure;
mod network;
mod options;
mod disks;
mod guided;
mod partition;
mod plan;
mod populate;
mod run;

use anyhow::{Context, Result, bail};
use config::Config;
use plan::{Firmware, Stage, Step};
use std::path::PathBuf;

/// The last stage real installs may run. Later stages are planned but not
/// executable yet; this moves forward one phase at a time.
const IMPLEMENTED_UNTIL: Stage = Stage::Finish;

const USAGE: &str = "\
arctix-install: the Arctix installer

usage:
  arctix-install guided                         ask a few questions, then install (live desktop launcher)
  arctix-install list-disks                     print disks as JSON
  arctix-install options                        print disks, defaults and choices as JSON (for the UI)
  arctix-install validate < config.toml         check a config, print ok + errors as JSON
  arctix-install plan <config.toml> [options]   print the install steps, run nothing
  arctix-install install <config.toml> [options]

options:
  --dry-run              (install) print the steps instead of running them
  --yes                  (install) skip typing the device path to confirm
  --password-stdin       (install) read the user's password from stdin, one line
  --offline              (install) skip mirrors and updates even if the network is up
  --progress json        (install) report progress as JSON lines (needs --yes and --password-stdin)
  --stop-after <stage>   stop after a stage: preflight, partition, populate, configure,
                         initramfs, cleanup, user, packages, bootloader, finish
  --firmware uefi|bios   (plan / dry run) override firmware detection
  -h, --help             show this
";

struct Opts {
    config: PathBuf,
    dry_run: bool,
    yes: bool,
    password_stdin: bool,
    offline: bool,
    json_progress: bool,
    firmware: Option<Firmware>,
    stop_after: Option<Stage>,
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut config = None;
    let mut opts = Opts { config: PathBuf::new(), dry_run: false, yes: false, password_stdin: false, offline: false, json_progress: false, firmware: None, stop_after: None };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dry-run" => opts.dry_run = true,
            "--yes" => opts.yes = true,
            "--password-stdin" => opts.password_stdin = true,
            "--offline" => opts.offline = true,
            "--progress" => match it.next().map(String::as_str) {
                Some("json") => opts.json_progress = true,
                Some("text") => opts.json_progress = false,
                _ => bail!("--progress takes json or text"),
            },
            "--firmware" => {
                let value = it.next().context("--firmware needs uefi or bios")?;
                opts.firmware =
                    Some(Firmware::parse(value).with_context(|| format!("--firmware: expected uefi or bios, got {value:?}"))?);
            }
            "--stop-after" => {
                let value = it.next().context("--stop-after needs a stage name")?;
                opts.stop_after = Some(Stage::parse(value).with_context(|| format!("--stop-after: unknown stage {value:?}"))?);
            }
            flag if flag.starts_with('-') => bail!("unknown option {flag}\n\n{USAGE}"),
            path if config.is_none() => config = Some(PathBuf::from(path)),
            extra => bail!("unexpected argument {extra:?}\n\n{USAGE}"),
        }
    }
    opts.config = config.with_context(|| format!("missing config file\n\n{USAGE}"))?;
    Ok(opts)
}

fn until(steps: Vec<Step>, last: Option<Stage>) -> Vec<Step> {
    match last {
        Some(last) => steps.into_iter().filter(|s| s.stage <= last).collect(),
        None => steps,
    }
}

fn cmd_list_disks() -> Result<()> {
    let devices = disks::lsblk()?;
    let live = disks::live_medium_disk()?;
    let infos = disks::disk_infos(&devices, live.as_deref());
    println!("{}", serde_json::to_string_pretty(&infos)?);
    Ok(())
}

fn print_plan(opts: &Opts) -> Result<()> {
    let cfg = Config::load(&opts.config)?;
    let fw = opts.firmware.unwrap_or_else(Firmware::detect);
    let steps = until(plan::build(&cfg, fw), opts.stop_after);
    print!("{}", plan::render(&cfg, fw, &steps));
    Ok(())
}

fn cmd_install(opts: &Opts) -> Result<()> {
    if opts.dry_run {
        return print_plan(opts);
    }
    if opts.firmware.is_some() {
        bail!("--firmware is only for plan and --dry-run, a real install always detects it");
    }

    if opts.json_progress && !(opts.yes && opts.password_stdin) {
        bail!("--progress json cannot prompt, so it needs --yes and --password-stdin");
    }
    run::set_json_progress(opts.json_progress);

    let mut cfg = Config::load(&opts.config)?;
    // Refuse before the disk is wiped, not halfway through.
    if cfg.install.mode == config::Mode::Pacstrap {
        bail!("pacstrap mode is not wired up yet (phase 9), use mode = \"copy\"");
    }
    // Resolve symlinks like /dev/disk/by-id/... to the real node, so the
    // safety checks compare against what lsblk reports.
    let real = std::fs::canonicalize(&cfg.disk.device)
        .with_context(|| format!("{} does not exist", cfg.disk.device))?;
    cfg.disk.device = real.to_string_lossy().into_owned();
    if !cfg.disk.device.starts_with("/dev/") {
        bail!("disk.device resolves to {}, which is not under /dev", cfg.disk.device);
    }

    let fw = Firmware::detect();
    let last = opts.stop_after.unwrap_or(IMPLEMENTED_UNTIL).min(IMPLEMENTED_UNTIL);
    let steps = until(plan::build(&cfg, fw), Some(last));

    run::log(&format!("---- arctix-install {} ----", env!("CARGO_PKG_VERSION")));
    run::log(&format!("target {} ({fw}), running up to stage {}", cfg.disk.device, last.key()));
    if !opts.json_progress {
        println!("Installing to {} ({fw}), up to stage \"{}\". Log: {}", cfg.disk.device, last.key(), run::LOG_PATH);
    }

    let mut ctx = run::ExecCtx {
        yes: opts.yes,
        password_stdin: opts.password_stdin,
        password: None,
        online: if opts.offline { Some(false) } else { None },
    };
    let result = run::execute(&steps, &mut ctx);
    let done = if last == Stage::Finish {
        format!("Arctix is installed on {}. Remove the install medium and reboot.", cfg.disk.device)
    } else {
        format!("Stopped after \"{}\".", last.key())
    };
    run::report_end(&result, &done);
    result?;

    if last < Stage::Finish {
        if last >= Stage::Partition {
            println!("{} is still mounted so you can look around. When done: sudo umount -R {}", plan::TARGET, plan::TARGET);
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("guided") => {
            let result = guided::run().and_then(|config| {
                cmd_install(&Opts {
                    config,
                    dry_run: false,
                    yes: false,
                    password_stdin: false,
                    offline: false,
                    json_progress: false,
                    firmware: None,
                    stop_after: None,
                })
            });
            if let Err(e) = &result {
                eprintln!("error: {e:#}");
                run::log(&format!("error: {e:#}"));
            }
            guided::finish(&result);
            result.map_err(|_| anyhow::anyhow!("install did not complete"))
        }
        Some("list-disks") => cmd_list_disks(),
        Some("options") => options::gather().and_then(|o| Ok(println!("{}", serde_json::to_string(&o)?)) ),
        Some("validate") => {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                .map_err(anyhow::Error::from)
                .and_then(|_| {
                    let v = options::validate(&text, std::path::Path::new("/"));
                    println!("{}", serde_json::to_string(&v)?);
                    Ok(())
                })
        }
        Some("plan") => parse_opts(&args[1..]).and_then(|o| print_plan(&o)),
        Some("install") => parse_opts(&args[1..]).and_then(|o| cmd_install(&o)),
        Some(other) => Err(anyhow::anyhow!("unknown command {other:?}\n\n{USAGE}")),
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        run::log(&format!("error: {e:#}"));
        std::process::exit(1);
    }
}
