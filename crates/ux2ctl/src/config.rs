// SPDX-License-Identifier: Apache-2.0
//! The settings file, `/etc/ux2-panel.toml`: how the needles move and what
//! each one shows.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use ux2_panel::{BallisticsSettings, Channels, Combine, Route, Routing, Source};

/// Where the settings live unless `--config` says otherwise.
pub const DEFAULT_PATH: &str = "/etc/ux2-panel.toml";

/// Every key `ux2ctl config set` takes, with what it means.
pub const KEYS: &[(&str, &str)] = &[
    (
        "fall_rate",
        "how fast a needle falls back, in dB a second (VU: 26)",
    ),
    (
        "clip_hold",
        "how long a clip lamp stays lit after clipping, in seconds",
    ),
    (
        "clip_threshold",
        "the level in dBFS at or above which a channel clips",
    ),
    (
        "startup_sweep",
        "sweep the needles when the meter starts or the UX2 is plugged in: true or false",
    ),
    (
        "needles",
        "whether the needles move at all: false keeps the panel still",
    ),
    (
        "left.source",
        "what the left needle shows: input, output or off",
    ),
    (
        "left.channels",
        "which channels feed the left needle: 1, 2 or 1,2",
    ),
    (
        "left.combine",
        "how several channels make the left needle's level: max, rms or sum",
    ),
    (
        "right.source",
        "what the right needle shows: input, output or off",
    ),
    (
        "right.channels",
        "which channels feed the right needle: 1, 2 or 1,2",
    ),
    (
        "right.combine",
        "how several channels make the right needle's level: max, rms or sum",
    ),
];

/// The settings, checked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// How the needles move.
    pub ballistics: BallisticsSettings,
    /// Whether the meter sweeps the needles when it starts or the UX2 is
    /// plugged in.
    pub startup_sweep: bool,
    /// Whether the needles move at all.
    pub needles: bool,
    /// What each needle shows.
    pub routing: Routing,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ballistics: BallisticsSettings::default(),
            startup_sweep: true,
            needles: true,
            routing: Routing::default(),
        }
    }
}

/// A settings file as written, before it is checked.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    fall_rate: Option<f32>,
    clip_hold: Option<f32>,
    clip_threshold: Option<f32>,
    startup_sweep: Option<bool>,
    needles: Option<bool>,
    left: Option<RouteFile>,
    right: Option<RouteFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteFile {
    source: Option<String>,
    channels: Option<Vec<u8>>,
    combine: Option<String>,
}

/// Where the settings come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The file.
    File,
    /// The defaults, as there is no file.
    Defaults,
}

impl Config {
    /// Reads the settings at `path`, or the defaults when there is no file.
    ///
    /// # Errors
    ///
    /// Returns why the file cannot be read or what is wrong in it.
    pub fn load(path: &Path) -> Result<(Self, Origin), String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text)
                .map(|config| (config, Origin::File))
                .map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok((Self::default(), Origin::Defaults))
            }
            Err(error) => Err(format!("cannot read {}: {error}", path.display())),
        }
    }

    /// Reads settings from TOML; a key left out keeps its default.
    ///
    /// # Errors
    ///
    /// Returns what is wrong in it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = basic_toml::from_str(text).map_err(|error| error.to_string())?;
        let mut config = Self::default();
        let mut apply = |key: &str, value: Option<String>| match value {
            Some(value) => config.set(key, &value),
            None => Ok(()),
        };
        apply("fall_rate", file.fall_rate.map(|value| value.to_string()))?;
        apply("clip_hold", file.clip_hold.map(|value| value.to_string()))?;
        apply(
            "clip_threshold",
            file.clip_threshold.map(|value| value.to_string()),
        )?;
        apply(
            "startup_sweep",
            file.startup_sweep.map(|value| value.to_string()),
        )?;
        apply("needles", file.needles.map(|value| value.to_string()))?;
        for (side, route) in [("left", file.left), ("right", file.right)] {
            let Some(route) = route else { continue };
            apply(&format!("{side}.source"), route.source)?;
            apply(
                &format!("{side}.channels"),
                route.channels.map(|numbers| {
                    numbers
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                }),
            )?;
            apply(&format!("{side}.combine"), route.combine)?;
        }
        Ok(config)
    }

    /// Sets one key, as `ux2ctl config set` does.
    ///
    /// # Errors
    ///
    /// Returns why the key or the value is not taken.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        match key {
            "fall_rate" => {
                self.ballistics.fall_rate = number(key, value, 1.0..=1000.0)?;
            }
            "clip_hold" => {
                self.ballistics.clip_hold = milliseconds(number(key, value, 0.0..=10.0)?);
            }
            "clip_threshold" => {
                self.ballistics.clip_threshold = number(key, value, -60.0..=6.0)?;
            }
            "startup_sweep" => self.startup_sweep = boolean(key, value)?,
            "needles" => self.needles = boolean(key, value)?,
            _ => {
                let (side, field) = key.split_once('.').ok_or_else(|| unknown(key))?;
                let route = match side {
                    "left" => &mut self.routing.left,
                    "right" => &mut self.routing.right,
                    _ => return Err(unknown(key)),
                };
                let invalid = |error: String| format!("{key}: {error}");
                match field {
                    "source" => route.source = value.parse::<Source>().map_err(invalid)?,
                    "channels" => route.channels = value.parse::<Channels>().map_err(invalid)?,
                    "combine" => route.combine = value.parse::<Combine>().map_err(invalid)?,
                    _ => return Err(unknown(key)),
                }
            }
        }
        Ok(())
    }

    /// The settings as a commented TOML file, which [`Config::parse`] reads
    /// back.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::from(
            "# ux2-panel: the Line 6 UX2's needles and clip lamps, as `ux2ctl meter` drives them.\n\
             # Change with `ux2ctl config set KEY VALUE` or `ux2ctl route`, then\n\
             # `systemctl reload ux2-panel`.\n\n",
        );
        let settings = &self.ballistics;
        let mut line = |key: &str, value: String| {
            let help = KEYS
                .iter()
                .find(|(name, _)| *name == key)
                .map_or("", |(_, help)| help);
            let name = key.rsplit('.').next().unwrap_or(key);
            let _ = writeln!(text, "# {help}\n{name} = {value}");
        };
        line("fall_rate", format!("{:?}", settings.fall_rate));
        line(
            "clip_hold",
            format!("{:?}", settings.clip_hold.as_secs_f32()),
        );
        line("clip_threshold", format!("{:?}", settings.clip_threshold));
        line("startup_sweep", self.startup_sweep.to_string());
        line("needles", self.needles.to_string());
        for (side, route) in [("left", self.routing.left), ("right", self.routing.right)] {
            line_route(&mut text, side, route);
        }
        text
    }

    /// Writes the settings to `path`, replacing what was there in one step.
    ///
    /// # Errors
    ///
    /// Returns why it cannot be written, as for want of permission.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut temporary = PathBuf::from(path);
        temporary.as_mut_os_string().push(".new");
        std::fs::write(&temporary, self.render())
            .and_then(|()| std::fs::rename(&temporary, path))
            .map_err(|error| {
                let hint = if error.kind() == io::ErrorKind::PermissionDenied {
                    " (run it with sudo)"
                } else {
                    ""
                };
                format!("cannot write {}: {error}{hint}", path.display())
            })
    }
}

fn line_route(text: &mut String, side: &str, route: Route) {
    let help = |field: &str| {
        KEYS.iter()
            .find(|(name, _)| *name == format!("{side}.{field}"))
            .map_or("", |(_, help)| help)
    };
    let channels = route
        .channels
        .numbers()
        .map(|number| number.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let _ = write!(
        text,
        "\n[{side}]\n# {}\nsource = \"{}\"\n# {}\nchannels = [{channels}]\n# {}\ncombine = \"{}\"\n",
        help("source"),
        route.source,
        help("channels"),
        help("combine"),
        route.combine,
    );
}

fn unknown(key: &str) -> String {
    let keys: Vec<&str> = KEYS.iter().map(|(name, _)| *name).collect();
    format!(
        "`{key}` is not a setting; the settings are {}",
        keys.join(", ")
    )
}

fn number(key: &str, value: &str, range: std::ops::RangeInclusive<f32>) -> Result<f32, String> {
    value
        .parse::<f32>()
        .ok()
        .filter(|number| range.contains(number))
        .ok_or_else(|| {
            format!(
                "{key}: `{value}` is not a number from {} to {}",
                range.start(),
                range.end()
            )
        })
}

/// Seconds, from 0 to 10, to the millisecond.
fn milliseconds(seconds: f32) -> Duration {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked to be in 0..=10 s first"
    )]
    let milliseconds = (seconds * 1000.0).round() as u64;
    Duration::from_millis(milliseconds)
}

fn boolean(key: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        _ => Err(format!("{key}: `{value}` is not true or false")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rendered_defaults_read_back() {
        let config = Config::default();
        assert_eq!(Config::parse(&config.render()), Ok(config));
        assert_eq!(Config::parse(""), Ok(config));
    }

    #[test]
    fn the_shipped_file_is_the_defaults() {
        let shipped = include_str!("../../../packaging/ux2-panel.toml");
        assert_eq!(shipped, Config::default().render());
    }

    #[test]
    fn every_key_can_be_set_and_read_back() {
        let mut config = Config::default();
        for (key, value) in [
            ("fall_rate", "13.5"),
            ("clip_hold", "1.5"),
            ("clip_threshold", "-1"),
            ("startup_sweep", "false"),
            ("needles", "off"),
            ("left.source", "output"),
            ("left.channels", "2"),
            ("left.combine", "rms"),
            ("right.source", "off"),
            ("right.channels", "1"),
            ("right.combine", "sum"),
        ] {
            config.set(key, value).unwrap();
        }
        assert!((config.ballistics.fall_rate - 13.5).abs() < f32::EPSILON);
        assert_eq!(config.ballistics.clip_hold, Duration::from_millis(1500));
        assert!(!config.startup_sweep && !config.needles);
        assert_eq!(config.routing.left.source, Source::Output);
        assert_eq!(config.routing.left.channels.to_string(), "2");
        assert_eq!(config.routing.right.combine, Combine::Sum);
        assert_eq!(Config::parse(&config.render()), Ok(config));
    }

    #[test]
    fn every_key_has_help() {
        let mut config = Config::default();
        for (key, help) in KEYS {
            assert!(!help.is_empty());
            assert!(
                !config
                    .set(key, "?")
                    .unwrap_err()
                    .contains("is not a setting")
            );
        }
    }

    #[test]
    fn bad_settings_are_refused() {
        let mut config = Config::default();
        assert!(config.set("volume", "11").is_err());
        assert!(config.set("left.colour", "red").is_err());
        assert!(config.set("fall_rate", "0").is_err());
        assert!(config.set("left.channels", "3,9").is_err());
        assert!(config.set("left.combine", "average").is_err());
        assert!(Config::parse("loudness = 11").is_err());
        assert!(Config::parse("[left]\nsource = \"mic\"").is_err());
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let config = Config::parse("fall_rate = 20\n[right]\nchannels = [1]\n").unwrap();
        assert!((config.ballistics.fall_rate - 20.0).abs() < f32::EPSILON);
        assert_eq!(config.routing.right.source, Source::Output);
        assert_eq!(config.routing.right.channels.to_string(), "1");
        assert_eq!(config.routing.left, Routing::default().left);
    }
}
