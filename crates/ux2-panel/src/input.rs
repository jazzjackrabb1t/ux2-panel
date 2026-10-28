// SPDX-License-Identifier: Apache-2.0
//! Which of the UX2's inputs it records, and on which channels.
//!
//! The kernel's driver owns this choice: it sends the UX2 the matching
//! vendor request and sends it again after a suspend, and offers it as the
//! card's "PCM Capture Source" control. So the choice goes through that
//! control, with `amixer`, rather than to the device behind the kernel's
//! back.

use std::fmt;
use std::io;
use std::process::Command;
use std::str::FromStr;

/// The card's ALSA name for each UX2.
pub const CARDS: [&str; 2] = ["PODStudioUX2", "TonePortUX2"];

/// The ALSA control that chooses the input.
const CONTROL: &str = "PCM Capture Source";

/// What the UX2 records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InputSource {
    /// The XLR microphone inputs.
    Microphone,
    /// The line inputs, left and right.
    Line,
    /// The instrument input, on both channels.
    #[default]
    Instrument,
    /// The instrument and a microphone at once, each on its own channel:
    /// the instrument on channel 1, the microphone on channel 2.
    InstrumentAndMic,
}

impl InputSource {
    /// Every source, in the order the card's control lists them.
    pub const ALL: [Self; 4] = [
        Self::Microphone,
        Self::Line,
        Self::Instrument,
        Self::InstrumentAndMic,
    ];

    /// The item of the card's control that chooses it.
    #[must_use]
    pub const fn item(self) -> usize {
        match self {
            Self::Microphone => 0,
            Self::Line => 1,
            Self::Instrument => 2,
            Self::InstrumentAndMic => 3,
        }
    }

    /// The vendor request value the kernel sends for it.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::Microphone => 0x0a01,
            Self::Line => 0x0801,
            Self::Instrument => 0x0b01,
            Self::InstrumentAndMic => 0x0901,
        }
    }

    /// What comes in on each channel.
    #[must_use]
    pub const fn channels(self) -> &'static str {
        match self {
            Self::Microphone => "the microphones on channels 1 and 2",
            Self::Line => "line left on channel 1, line right on channel 2",
            Self::Instrument => "the instrument on channels 1 and 2",
            Self::InstrumentAndMic => "the instrument on channel 1, a microphone on channel 2",
        }
    }
}

impl fmt::Display for InputSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Microphone => "mic",
            Self::Line => "line",
            Self::Instrument => "instrument",
            Self::InstrumentAndMic => "instrument+mic",
        })
    }
}

impl FromStr for InputSource {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.to_ascii_lowercase().as_str() {
            "mic" | "microphone" => Ok(Self::Microphone),
            "line" => Ok(Self::Line),
            "instrument" | "inst" | "guitar" => Ok(Self::Instrument),
            "instrument+mic" | "inst+mic" | "inst&mic" | "instrument-and-mic" => {
                Ok(Self::InstrumentAndMic)
            }
            other => Err(format!(
                "no input called {other:?}: say mic, line, instrument or instrument+mic"
            )),
        }
    }
}

fn amixer(card: &str, arguments: &[&str]) -> io::Result<String> {
    let output = Command::new("amixer")
        .arg("-c")
        .arg(card)
        .args(arguments)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

/// The first UX2 card ALSA has.
#[must_use]
pub fn card() -> Option<&'static str> {
    let cards = std::fs::read_to_string("/proc/asound/cards").unwrap_or_default();
    CARDS
        .into_iter()
        .find(|name| cards.contains(&format!("[{name}")))
}

/// The input `card` records now.
///
/// # Errors
///
/// Returns why `amixer` could not tell.
pub fn input(card: &str) -> io::Result<InputSource> {
    let answer = amixer(card, &["cget", &format!("name={CONTROL}")])?;
    let item = answer
        .lines()
        .find_map(|line| line.trim().strip_prefix(": values="))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .ok_or_else(|| io::Error::other(format!("{card} has no {CONTROL} control")))?;
    InputSource::ALL
        .into_iter()
        .find(|source| source.item() == item)
        .ok_or_else(|| io::Error::other(format!("{card} chose input {item}, which is unknown")))
}

/// Has `card` record `source`.
///
/// # Errors
///
/// Returns why `amixer` could not set it, as when this user may not.
pub fn set_input(card: &str, source: InputSource) -> io::Result<()> {
    amixer(
        card,
        &[
            "cset",
            &format!("name={CONTROL}"),
            &source.item().to_string(),
        ],
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_reads_back_from_its_name_and_lists_in_the_controls_order() {
        for (item, source) in InputSource::ALL.into_iter().enumerate() {
            assert_eq!(source.item(), item);
            assert_eq!(source.to_string().parse::<InputSource>(), Ok(source));
        }
        assert_eq!("inst&mic".parse(), Ok(InputSource::InstrumentAndMic));
        assert!("bass".parse::<InputSource>().is_err());
    }
}
