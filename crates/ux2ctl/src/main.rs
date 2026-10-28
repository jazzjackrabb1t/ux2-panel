// SPDX-License-Identifier: Apache-2.0
//! `ux2ctl`: the Line 6 POD Studio UX2's front panel from the command line,
//! and a meter daemon that drives it from any program's levels.

mod config;
mod meter;
mod sweep;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use ux2_panel::{Channels, Clip, Combine, FULL_SCALE, Panel, Route, Source, find};

use crate::config::{Config, KEYS, Origin};
use crate::meter::Input;

/// Where the meter service listens.
const SERVICE_SOCKET: &str = "/run/ux2-panel/meter.sock";

/// Drive the needles and clip lamps of a Line 6 POD Studio UX2.
#[derive(Debug, Parser)]
#[command(name = "ux2ctl", version)]
struct Cli {
    /// The settings file.
    #[arg(long, global = true, value_name = "FILE", default_value = config::DEFAULT_PATH)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Say whether a UX2 is plugged in and whether this user may drive it.
    Status,
    /// Move the needles, each from 0 at rest to 140 at full scale.
    Needles {
        /// The left needle.
        #[arg(value_parser = clap::value_parser!(u8).range(0..=i64::from(FULL_SCALE)))]
        left: u8,
        /// The right needle.
        #[arg(value_parser = clap::value_parser!(u8).range(0..=i64::from(FULL_SCALE)))]
        right: u8,
    },
    /// Set the clip lamps. A lamp lights only while its needle is at full
    /// scale (`ux2ctl needles 140 140`).
    Clip {
        /// Which lamps are lit.
        #[arg(value_enum)]
        lamps: LampsArg,
    },
    /// Put both needles at rest and both lamps out.
    Rest,
    /// Sweep the needles and the lamps once, to see that the panel works.
    Sweep,
    /// Drive the needles from levels in dBFS on stdin or a Unix socket.
    ///
    /// Each line names channels, `in1=-12.3 in2=-40 out1=-6 out2=-6`, which
    /// go through the routing (`ux2ctl route`); or gives the needles directly,
    /// `LEFT RIGHT`, or one level for both. Stops at the end of stdin, or on
    /// SIGINT or SIGTERM, with the needles at rest; SIGHUP reads the settings
    /// again.
    Meter {
        /// Listen on a Unix socket instead of reading stdin.
        #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = SERVICE_SOCKET)]
        socket: Option<PathBuf>,
        /// The socket's permission bits, in octal.
        #[arg(long, value_name = "MODE", default_value = "660", value_parser = octal)]
        socket_mode: u32,
    },
    /// Show what each needle shows, or set it.
    ///
    /// `ux2ctl route left input 1,2 --combine max` shows the loudest of both
    /// input channels on the left needle; `ux2ctl route right output 1` shows
    /// output channel 1 on the right; `ux2ctl route right off` stills it.
    Route {
        /// The needle to set; without one, the routing is shown.
        #[arg(value_enum, requires = "source")]
        needle: Option<NeedleArg>,
        /// What it shows: input, output or off.
        source: Option<Source>,
        /// Which channels feed it, as 1, 2 or 1,2; unchanged if left out.
        channels: Option<Channels>,
        /// How several channels make one level: max (the loudest), rms (the
        /// average power) or sum (the amplitudes added).
        #[arg(long)]
        combine: Option<Combine>,
    },
    /// Show or change the settings.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Show the settings in effect, as the settings file.
    Show,
    /// Set one setting in the settings file.
    Set {
        /// The setting.
        key: String,
        /// Its new value.
        value: String,
    },
    /// List the settings and what each means.
    Keys,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LampsArg {
    None,
    Left,
    Right,
    Both,
}

impl From<LampsArg> for Clip {
    fn from(lamps: LampsArg) -> Self {
        match lamps {
            LampsArg::None => Self::None,
            LampsArg::Left => Self::Left,
            LampsArg::Right => Self::Right,
            LampsArg::Both => Self::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum NeedleArg {
    Left,
    Right,
    Both,
}

fn octal(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text, 8)
        .ok()
        .filter(|mode| *mode <= 0o777)
        .ok_or_else(|| format!("`{text}` is not a file mode such as 660"))
}

/// Writes a line to standard error, where the journal picks it up.
fn say(text: &str) {
    eprintln!("ux2ctl: {text}");
}

fn keys_help() -> String {
    let width = KEYS.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
    let mut help = String::from("Settings:\n");
    for (key, meaning) in KEYS {
        let _ = writeln!(help, "  {key:width$}  {meaning}");
    }
    help
}

fn main() -> ExitCode {
    let command = Cli::command().mut_subcommand("config", |config| {
        config.mut_subcommand("set", |set| set.after_help(keys_help()))
    });
    let cli = match Cli::from_arg_matches(&command.get_matches()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            say(&error);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Status => return Ok(status(&cli.config)),
        Command::Needles { left, right } => open()?
            .set_needles(left, right)
            .map_err(|error| error.to_string())?,
        Command::Clip { lamps } => {
            let clip = Clip::from(lamps);
            open()?.set_clip(clip).map_err(|error| error.to_string())?;
            if clip != Clip::None {
                say(
                    "a lamp lights only while its needle is at full scale (ux2ctl needles 140 140)",
                );
            }
        }
        Command::Rest => open()?.rest().map_err(|error| error.to_string())?,
        Command::Sweep => {
            let mut panel = open()?;
            let stop = signals()?.0;
            let swept = sweep::play(|reading| panel.show(reading), &stop);
            let rested = panel.rest();
            swept.and(rested).map_err(|error| error.to_string())?;
        }
        Command::Meter {
            socket,
            socket_mode,
        } => {
            let input = socket.map_or(Input::Stdin, |path| Input::Socket {
                path,
                mode: socket_mode,
            });
            let (stop, reload) = signals()?;
            meter::run(&cli.config, &input, &stop, &reload)?;
        }
        Command::Route {
            needle,
            source,
            channels,
            combine,
        } => route(&cli.config, needle, source, channels, combine)?,
        Command::Config { action } => configure(&cli.config, action)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn open() -> Result<Panel, String> {
    Panel::open().map_err(|error| error.to_string())
}

/// Flags set on SIGINT or SIGTERM, and on SIGHUP.
fn signals() -> Result<(Arc<AtomicBool>, Arc<AtomicBool>), String> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let stop = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));
    for (signal, flag) in [(SIGINT, &stop), (SIGTERM, &stop), (SIGHUP, &reload)] {
        signal_hook::flag::register(signal, Arc::clone(flag))
            .map_err(|error| format!("cannot handle signal {signal}: {error}"))?;
    }
    Ok((stop, reload))
}

fn status(config_path: &Path) -> ExitCode {
    let found = match find() {
        Ok(Some(found)) => found,
        Ok(None) => {
            println!("UX2:      not found");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            println!("UX2:      {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("UX2:      found, {}", found.name());
    println!(
        "USB:      {:04x}:{:04x}",
        ux2_panel::VENDOR_ID,
        found.product_id()
    );
    println!("bus:      {}, device {:03}", found.bus(), found.address());
    println!("file:     {}", found.device_file().display());
    let writable = match found.open() {
        Ok(_) => "yes".to_owned(),
        Err(error) if error.is_permission_denied() => {
            "no: permission denied (install the udev rule and join group audio)".to_owned()
        }
        Err(error) => format!("no: {error}"),
    };
    println!("writable: {writable}");
    let settings = match Config::load(config_path) {
        Ok((_, Origin::File)) => config_path.display().to_string(),
        Ok((_, Origin::Defaults)) => format!("defaults ({} not found)", config_path.display()),
        Err(error) => format!("unreadable: {error}"),
    };
    println!("settings: {settings}");
    if writable == "yes" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn route(
    config_path: &Path,
    needle: Option<NeedleArg>,
    source: Option<Source>,
    channels: Option<Channels>,
    combine: Option<Combine>,
) -> Result<(), String> {
    let (mut config, _) = Config::load(config_path)?;
    if let (Some(needle), Some(source)) = (needle, source) {
        let set = |route: &mut Route| {
            route.source = source;
            route.channels = channels.unwrap_or(route.channels);
            route.combine = combine.unwrap_or(route.combine);
        };
        if needle != NeedleArg::Right {
            set(&mut config.routing.left);
        }
        if needle != NeedleArg::Left {
            set(&mut config.routing.right);
        }
        config.save(config_path)?;
    }
    println!("left needle:  {}", config.routing.left);
    println!("right needle: {}", config.routing.right);
    if needle.is_some() {
        say("saved; `systemctl reload ux2-panel` puts it in effect");
    }
    Ok(())
}

fn configure(config_path: &Path, action: ConfigAction) -> Result<(), String> {
    match action {
        ConfigAction::Show => {
            let (config, origin) = Config::load(config_path)?;
            if origin == Origin::Defaults {
                println!(
                    "# {} not found: these are the defaults.",
                    config_path.display()
                );
            }
            print!("{}", config.render());
        }
        ConfigAction::Set { key, value } => {
            let (mut config, _) = Config::load(config_path)?;
            config.set(&key, &value)?;
            config.save(config_path)?;
            say(&format!(
                "{key} set in {}; `systemctl reload ux2-panel` puts it in effect",
                config_path.display()
            ));
        }
        ConfigAction::Keys => print!("{}", keys_help()),
    }
    Ok(())
}
