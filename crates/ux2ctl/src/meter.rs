// SPDX-License-Identifier: Apache-2.0
//! `ux2ctl meter`: the needles driven by a level feed on standard input or a
//! Unix socket, moving like a VU meter's.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ux2_panel::feed::{Line, parse_line};
use ux2_panel::{
    AutoPanel, Ballistics, Channel, ChannelLevels, Error, Levels, NeedleLevel, Routing, SILENCE,
    Source,
};

use crate::config::Config;
use crate::{say, sweep};

/// How often the needles move.
const FRAME: Duration = Duration::from_millis(20);

/// How long a level stands for without a newer one; after that its channel
/// is taken as silent.
const STALE: Duration = Duration::from_millis(500);

/// Where the meter reads its feed.
#[derive(Debug, Clone)]
pub enum Input {
    /// Standard input: the meter stops when it closes.
    Stdin,
    /// A Unix socket that any number of programs may connect to.
    Socket {
        /// The socket's path.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
}

/// One level and when it came.
#[derive(Debug, Clone, Copy)]
struct Held {
    loudest_since_frame: Option<f32>,
    latest: f32,
    at: Instant,
}

impl Held {
    fn new(level: f32, now: Instant) -> Self {
        Self {
            loudest_since_frame: Some(level),
            latest: level,
            at: now,
        }
    }

    fn feed(&mut self, level: f32, now: Instant) {
        self.loudest_since_frame = Some(
            self.loudest_since_frame
                .map_or(level, |loudest| loudest.max(level)),
        );
        self.latest = level;
        self.at = now;
    }

    /// The level for this frame: the loudest that came since the last frame,
    /// or the latest while it is fresh.
    fn take(&mut self, now: Instant) -> Option<f32> {
        self.loudest_since_frame
            .take()
            .or_else(|| (now.duration_since(self.at) < STALE).then_some(self.latest))
    }
}

/// The levels fed so far, each channel as it last came.
#[derive(Debug, Default)]
pub struct Feed {
    channels: HashMap<Channel, Held>,
    needles: Option<[Held; 2]>,
}

impl Feed {
    /// Takes in one line of the feed.
    pub fn apply(&mut self, line: Line, now: Instant) {
        match line {
            Line::Channels(levels) => {
                for (channel, level) in levels {
                    self.channels
                        .entry(channel)
                        .and_modify(|held| held.feed(level, now))
                        .or_insert_with(|| Held::new(level, now));
                }
            }
            Line::Needles { left, right } => match self.needles.as_mut() {
                Some([held_left, held_right]) => {
                    held_left.feed(left, now);
                    held_right.feed(right, now);
                }
                None => self.needles = Some([Held::new(left, now), Held::new(right, now)]),
            },
        }
    }

    /// What the needles are fed this frame. Channels go through `routing`;
    /// needle levels fed directly stand in for it while they are fresh,
    /// except on a needle that is off.
    pub fn levels(&mut self, routing: Routing, now: Instant) -> Levels {
        let mut channels = ChannelLevels::SILENT;
        self.channels.retain(|channel, held| {
            let level = held.take(now);
            channels.set(*channel, level.unwrap_or(SILENCE));
            level.is_some()
        });
        let mut levels = routing.levels(&channels);
        if let Some([left, right]) = self.needles.as_mut() {
            let direct = [left.take(now), right.take(now)];
            if direct.iter().all(Option::is_none) {
                self.needles = None;
            }
            for (needle, route, level) in [
                (&mut levels.left, routing.left, direct[0]),
                (&mut levels.right, routing.right, direct[1]),
            ] {
                if let Some(level) = level.filter(|_| route.source != Source::Off) {
                    *needle = NeedleLevel::new(level);
                }
            }
        }
        levels
    }
}

enum Message {
    Line(Line),
    Closed,
}

fn read_lines(reader: impl BufRead, from: &str, messages: &Sender<Message>) {
    for (number, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                say(&format!("{from}: line {} is not text", number + 1));
                continue;
            }
            Err(_) => break,
        };
        match parse_line(&line) {
            Ok(Some(line)) => {
                if messages.send(Message::Line(line)).is_err() {
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => say(&format!("{from}: line {} ignored: {error}", number + 1)),
        }
    }
}

fn listen(path: &Path, mode: u32) -> Result<UnixListener, String> {
    let bound = match UnixListener::bind(path) {
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            let is_socket = std::fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.file_type().is_socket());
            if !is_socket {
                return Err(format!(
                    "{} is in the way and is not a socket",
                    path.display()
                ));
            }
            if UnixStream::connect(path).is_ok() {
                return Err(format!("another meter is listening on {}", path.display()));
            }
            std::fs::remove_file(path)
                .and_then(|()| UnixListener::bind(path))
                .map_err(|error| format!("cannot listen on {}: {error}", path.display()))?
        }
        bound => bound.map_err(|error| format!("cannot listen on {}: {error}", path.display()))?,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set the mode of {}: {error}", path.display()))?;
    Ok(bound)
}

fn start(input: &Input, messages: Sender<Message>) -> Result<(), String> {
    match input {
        Input::Stdin => {
            std::thread::spawn(move || {
                read_lines(io::stdin().lock(), "stdin", &messages);
                let _ = messages.send(Message::Closed);
            });
        }
        Input::Socket { path, mode } => {
            let listener = listen(path, *mode)?;
            say(&format!("listening on {}", path.display()));
            std::thread::spawn(move || {
                for (number, client) in listener.incoming().enumerate() {
                    let Ok(client) = client else { continue };
                    let messages = messages.clone();
                    std::thread::spawn(move || {
                        let from = format!("client {}", number + 1);
                        read_lines(BufReader::new(client), &from, &messages);
                    });
                }
            });
        }
    }
    Ok(())
}

/// Drives the needles from `input` by the settings at `config_path` until
/// `stop` is set or standard input closes, then puts them at rest. The
/// settings are read again whenever `reload` is set.
///
/// # Errors
///
/// Returns why the settings or the socket cannot be used.
pub fn run(
    config_path: &Path,
    input: &Input,
    stop: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
) -> Result<(), String> {
    let (mut config, _) = Config::load(config_path)?;
    let (sender, messages) = mpsc::channel();
    start(input, sender)?;

    let mut panel = AutoPanel::new();
    let mut ballistics = Ballistics::new(config.ballistics);
    let mut feed = Feed::default();
    let mut reported: Option<String> = None;
    let mut last = Instant::now();
    let mut next = last + FRAME;
    let mut closed = false;

    while !closed && !stop.load(Ordering::Relaxed) {
        loop {
            let wait = next.saturating_duration_since(Instant::now());
            match messages.recv_timeout(wait) {
                Ok(Message::Line(line)) => feed.apply(line, Instant::now()),
                Ok(Message::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    closed = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        let now = Instant::now();
        next = (next + FRAME).max(now);
        let elapsed = now.duration_since(last);
        last = now;

        if reload.swap(false, Ordering::Relaxed) {
            match Config::load(config_path) {
                Ok((loaded, _)) => {
                    config = loaded;
                    ballistics.set_settings(config.ballistics);
                    say("settings read again");
                }
                Err(error) => say(&format!("settings kept: {error}")),
            }
        }

        if !panel.is_connected() && !connect(&mut panel, &config, stop, &mut reported) {
            continue;
        }
        let levels = feed.levels(config.routing, now);
        let reading = ballistics.update(
            if config.needles {
                levels
            } else {
                Levels::SILENT
            },
            elapsed,
        );
        if let Err(error) = panel.show(reading) {
            report(&mut reported, &error);
        }
    }
    let _ = panel.rest();
    if let Input::Socket { path, .. } = input {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Opens the panel if it is time to try, sweeping it when the settings ask;
/// answers whether it is open.
fn connect(
    panel: &mut AutoPanel,
    config: &Config,
    stop: &AtomicBool,
    reported: &mut Option<String>,
) -> bool {
    match panel.panel() {
        Ok(opened) => {
            say(&format!("driving the {}", opened.found()));
            *reported = None;
        }
        Err(error) => {
            report(reported, &error);
            return false;
        }
    }
    if config.startup_sweep {
        let _ = sweep::play(|reading| panel.show(reading), stop);
    }
    true
}

/// Says what went wrong, once until something else does.
fn report(reported: &mut Option<String>, error: &Error) {
    if matches!(error, Error::NotFound) && reported.is_some() {
        return;
    }
    let text = match error {
        Error::NotFound => "no UX2 is plugged in; waiting for one".to_owned(),
        error => error.to_string(),
    };
    if reported.as_deref() != Some(text.as_str()) {
        say(&text);
        *reported = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ux2_panel::{Combine, Route};

    fn line(text: &str) -> Line {
        parse_line(text).unwrap().unwrap()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn named_channels_go_through_the_routing() {
        let mut feed = Feed::default();
        let now = Instant::now();
        feed.apply(line("in1=-12 in2=-40 out1=-6 out2=-9"), now);
        let routing = Routing {
            left: Route {
                channels: "2".parse().unwrap(),
                ..Route::both(Source::Input)
            },
            right: Route {
                combine: Combine::Rms,
                ..Route::both(Source::Output)
            },
        };
        let levels = feed.levels(routing, now);
        assert!(close(levels.left.level, -40.0));
        assert!(close(levels.right.level, -7.25));
        assert!(close(levels.right.peak, -6.0));
    }

    #[test]
    fn channels_fed_apart_are_heard_together() {
        let mut feed = Feed::default();
        let now = Instant::now();
        feed.apply(line("in1=-30"), now);
        feed.apply(line("out1=-3"), now);
        let levels = feed.levels(Routing::default(), now);
        assert!(close(levels.left.level, -30.0));
        assert!(close(levels.right.level, -3.0));
    }

    #[test]
    fn the_loudest_level_of_a_frame_is_kept() {
        let mut feed = Feed::default();
        let now = Instant::now();
        feed.apply(line("in1=-3"), now);
        feed.apply(line("in1=-30"), now);
        assert!(close(feed.levels(Routing::default(), now).left.level, -3.0));
        assert!(close(
            feed.levels(Routing::default(), now).left.level,
            -30.0
        ));
    }

    #[test]
    fn a_level_goes_silent_once_stale() {
        let mut feed = Feed::default();
        let now = Instant::now();
        feed.apply(line("in1=-10"), now);
        feed.levels(Routing::default(), now);
        let later = now + STALE;
        assert!(close(
            feed.levels(Routing::default(), later).left.level,
            SILENCE
        ));
        assert!(feed.channels.is_empty());
    }

    #[test]
    fn needle_levels_bypass_the_routing_but_not_an_off_needle() {
        let mut feed = Feed::default();
        let now = Instant::now();
        feed.apply(line("-12 -6"), now);
        let routing = Routing {
            right: Route::OFF,
            ..Routing::default()
        };
        let levels = feed.levels(routing, now);
        assert!(close(levels.left.level, -12.0));
        assert!(close(levels.right.level, SILENCE));
    }
}
