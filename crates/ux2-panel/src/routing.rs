// SPDX-License-Identifier: Apache-2.0
//! Which channels each needle shows, and how several channels make one
//! needle's level.

use std::fmt;
use std::str::FromStr;

use crate::ballistics::{Levels, NeedleLevel};
use crate::curve::SILENCE;

/// The most channels a direction can have in a level feed. The UX2 records
/// on two and plays on two.
pub const MAX_CHANNELS: u8 = 8;

/// Recording or playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// What the interface records: capture channels.
    Input,
    /// What the interface plays: playback channels.
    Output,
}

/// One channel, numbered from 1, as `in1` or `out2` in a level feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Channel {
    direction: Direction,
    number: u8,
}

impl Channel {
    /// Channel `number` of `direction`, if it is within 1..=[`MAX_CHANNELS`].
    #[must_use]
    pub const fn new(direction: Direction, number: u8) -> Option<Self> {
        if number >= 1 && number <= MAX_CHANNELS {
            Some(Self { direction, number })
        } else {
            None
        }
    }

    /// Whether it records or plays.
    #[must_use]
    pub const fn direction(self) -> Direction {
        self.direction
    }

    /// Its number, from 1.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.number
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.direction {
            Direction::Input => "in",
            Direction::Output => "out",
        };
        write!(formatter, "{prefix}{}", self.number)
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (direction, number) = if let Some(number) = text.strip_prefix("in") {
            (Direction::Input, number)
        } else if let Some(number) = text.strip_prefix("out") {
            (Direction::Output, number)
        } else {
            return Err(format!("`{text}` is not a channel such as in1 or out2"));
        };
        number
            .parse()
            .ok()
            .and_then(|number| Self::new(direction, number))
            .ok_or_else(|| {
                format!(
                    "`{text}` is not a channel in1..in{MAX_CHANNELS} or out1..out{MAX_CHANNELS}"
                )
            })
    }
}

/// The latest level of every channel, in dBFS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelLevels {
    inputs: [f32; MAX_CHANNELS as usize],
    outputs: [f32; MAX_CHANNELS as usize],
}

impl Default for ChannelLevels {
    fn default() -> Self {
        Self::SILENT
    }
}

impl ChannelLevels {
    /// Every channel silent.
    pub const SILENT: Self = Self {
        inputs: [SILENCE; MAX_CHANNELS as usize],
        outputs: [SILENCE; MAX_CHANNELS as usize],
    };

    /// One channel's level.
    #[must_use]
    pub fn get(&self, channel: Channel) -> f32 {
        let slot = usize::from(channel.number - 1);
        match channel.direction {
            Direction::Input => self.inputs[slot],
            Direction::Output => self.outputs[slot],
        }
    }

    /// Sets one channel's level.
    pub fn set(&mut self, channel: Channel, decibels: f32) {
        let slot = usize::from(channel.number - 1);
        match channel.direction {
            Direction::Input => self.inputs[slot] = decibels,
            Direction::Output => self.outputs[slot] = decibels,
        }
    }

    /// Sets input `number`'s level; numbers outside 1..=[`MAX_CHANNELS`] are
    /// ignored.
    pub fn set_input(&mut self, number: u8, decibels: f32) {
        if let Some(channel) = Channel::new(Direction::Input, number) {
            self.set(channel, decibels);
        }
    }

    /// Sets output `number`'s level; numbers outside 1..=[`MAX_CHANNELS`] are
    /// ignored.
    pub fn set_output(&mut self, number: u8, decibels: f32) {
        if let Some(channel) = Channel::new(Direction::Output, number) {
            self.set(channel, decibels);
        }
    }
}

/// What a needle shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// Channels the interface records.
    Input,
    /// Channels the interface plays.
    Output,
    /// Nothing: the needle rests and its lamp stays out.
    Off,
}

impl Source {
    /// The direction of its channels, unless it is off.
    #[must_use]
    pub const fn direction(self) -> Option<Direction> {
        match self {
            Self::Input => Some(Direction::Input),
            Self::Output => Some(Direction::Output),
            Self::Off => None,
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::Off => "off",
        })
    }
}

impl FromStr for Source {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "input" | "in" => Ok(Self::Input),
            "output" | "out" => Ok(Self::Output),
            "off" => Ok(Self::Off),
            _ => Err(format!("`{text}` is not input, output or off")),
        }
    }
}

/// How several channels make one needle's level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Combine {
    /// The loudest channel.
    #[default]
    Max,
    /// The average power of the channels: their RMS.
    Rms,
    /// The channels' amplitudes added, as if mixed in phase.
    Sum,
}

impl Combine {
    /// Combines levels in dBFS; no levels at all are silence.
    #[must_use]
    pub fn combine(self, levels: impl IntoIterator<Item = f32>) -> f32 {
        let levels = levels.into_iter().map(|level| {
            if level.is_nan() {
                f32::NEG_INFINITY
            } else {
                level
            }
        });
        let combined = match self {
            Self::Max => levels.fold(f32::NEG_INFINITY, f32::max),
            Self::Rms => {
                let (power, count) = levels.fold((0.0_f32, 0_u16), |(power, count), level| {
                    (power + 10.0_f32.powf(level / 10.0), count + 1)
                });
                if count == 0 {
                    f32::NEG_INFINITY
                } else {
                    10.0 * (power / f32::from(count)).log10()
                }
            }
            Self::Sum => {
                let amplitude: f32 = levels.map(|level| 10.0_f32.powf(level / 20.0)).sum();
                20.0 * amplitude.log10()
            }
        };
        combined.max(SILENCE)
    }
}

impl fmt::Display for Combine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Max => "max",
            Self::Rms => "rms",
            Self::Sum => "sum",
        })
    }
}

impl FromStr for Combine {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "max" => Ok(Self::Max),
            "rms" => Ok(Self::Rms),
            "sum" => Ok(Self::Sum),
            _ => Err(format!("`{text}` is not max, rms or sum")),
        }
    }
}

/// A set of channel numbers, each in 1..=[`MAX_CHANNELS`], written `1,2`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Channels(u8);

impl Channels {
    /// No channels.
    pub const NONE: Self = Self(0);

    /// Channels 1 and 2: both of the UX2's in a direction.
    pub const BOTH: Self = Self(0b11);

    /// The channels numbered in `numbers`, or the first number outside
    /// 1..=[`MAX_CHANNELS`].
    ///
    /// # Errors
    ///
    /// Returns the number that is out of range.
    pub fn from_numbers(numbers: impl IntoIterator<Item = u8>) -> Result<Self, u8> {
        numbers.into_iter().try_fold(Self::NONE, |set, number| {
            if (1..=MAX_CHANNELS).contains(&number) {
                Ok(Self(set.0 | 1 << (number - 1)))
            } else {
                Err(number)
            }
        })
    }

    /// Whether channel `number` is in the set.
    #[must_use]
    pub const fn contains(self, number: u8) -> bool {
        number >= 1 && number <= MAX_CHANNELS && self.0 & (1 << (number - 1)) != 0
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The channel numbers, lowest first.
    pub fn numbers(self) -> impl Iterator<Item = u8> {
        (1..=MAX_CHANNELS).filter(move |&number| self.contains(number))
    }
}

impl fmt::Display for Channels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let numbers: Vec<String> = self.numbers().map(|number| number.to_string()).collect();
        formatter.write_str(&numbers.join(","))
    }
}

impl FromStr for Channels {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let numbers = text
            .split(|c: char| c == ',' || c == '+' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse::<u8>()
                    .map_err(|_| format!("`{part}` is not a channel number"))
            })
            .collect::<Result<Vec<u8>, String>>()?;
        if numbers.is_empty() {
            return Err("name at least one channel, such as 1 or 1,2".to_owned());
        }
        Self::from_numbers(numbers)
            .map_err(|number| format!("channel {number} is not in 1..={MAX_CHANNELS}"))
    }
}

/// What one needle shows: which channels, and how they are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Route {
    /// Input, output or nothing.
    pub source: Source,
    /// The channels of the source that feed the needle.
    pub channels: Channels,
    /// How several channels make one level.
    pub combine: Combine,
}

impl Route {
    /// A needle that rests.
    pub const OFF: Self = Self {
        source: Source::Off,
        channels: Channels::BOTH,
        combine: Combine::Max,
    };

    /// Both channels of `source`, the loudest shown.
    #[must_use]
    pub const fn both(source: Source) -> Self {
        Self {
            source,
            channels: Channels::BOTH,
            combine: Combine::Max,
        }
    }

    /// What the needle is fed from `levels`: the channels combined, and the
    /// loudest of them for clipping.
    #[must_use]
    pub fn level(&self, levels: &ChannelLevels) -> NeedleLevel {
        let Some(direction) = self.source.direction() else {
            return NeedleLevel::SILENT;
        };
        let fed = || {
            self.channels
                .numbers()
                .filter_map(move |number| Channel::new(direction, number))
                .map(|channel| levels.get(channel))
        };
        NeedleLevel {
            level: self.combine.combine(fed()),
            peak: Combine::Max.combine(fed()),
        }
    }
}

impl fmt::Display for Route {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.source == Source::Off {
            return formatter.write_str("off");
        }
        if self.channels.numbers().count() == 1 {
            write!(formatter, "{} channel {}", self.source, self.channels)
        } else {
            write!(
                formatter,
                "{} channels {}, combined by {}",
                self.source, self.channels, self.combine
            )
        }
    }
}

/// What both needles show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Routing {
    /// The left needle.
    pub left: Route,
    /// The right needle.
    pub right: Route,
}

impl Default for Routing {
    /// The input on the left and the output on the right, both channels of
    /// each, the loudest shown.
    fn default() -> Self {
        Self {
            left: Route::both(Source::Input),
            right: Route::both(Source::Output),
        }
    }
}

impl Routing {
    /// What both needles are fed from `levels`.
    #[must_use]
    pub fn levels(&self, levels: &ChannelLevels) -> Levels {
        Levels {
            left: self.left.level(levels),
            right: self.right.level(levels),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    fn feed() -> ChannelLevels {
        let mut levels = ChannelLevels::SILENT;
        levels.set_input(1, -12.0);
        levels.set_input(2, -40.0);
        levels.set_output(1, -6.0);
        levels.set_output(2, -6.0);
        levels
    }

    #[test]
    fn channels_parse_and_print() {
        assert_eq!(
            "in1".parse(),
            Ok(Channel::new(Direction::Input, 1).unwrap())
        );
        assert_eq!("out8".parse::<Channel>().unwrap().to_string(), "out8");
        assert!("in0".parse::<Channel>().is_err());
        assert!("in9".parse::<Channel>().is_err());
        assert!("left".parse::<Channel>().is_err());
        assert_eq!("1,2".parse(), Ok(Channels::BOTH));
        assert_eq!("2".parse::<Channels>().unwrap().to_string(), "2");
        assert_eq!("1+2".parse(), Ok(Channels::BOTH));
        assert!("".parse::<Channels>().is_err());
        assert!("0".parse::<Channels>().is_err());
        assert!("9".parse::<Channels>().is_err());
    }

    #[test]
    fn max_shows_the_loudest() {
        assert!(close(Combine::Max.combine([-12.0, -40.0]), -12.0));
    }

    #[test]
    fn rms_is_the_average_power() {
        assert!(close(Combine::Rms.combine([-6.0, -6.0]), -6.0));
        assert!(close(Combine::Rms.combine([0.0, f32::NEG_INFINITY]), -3.01));
    }

    #[test]
    fn sum_adds_amplitudes() {
        assert!(close(Combine::Sum.combine([-6.0, -6.0]), -6.0 + 6.02));
        assert!(close(Combine::Sum.combine([-20.0]), -20.0));
    }

    #[test]
    fn nothing_combined_is_silence() {
        for combine in [Combine::Max, Combine::Rms, Combine::Sum] {
            assert!(close(combine.combine([]), SILENCE));
            assert!(close(combine.combine([f32::NAN]), SILENCE));
        }
    }

    #[test]
    fn the_default_shows_the_input_left_and_the_output_right() {
        let levels = Routing::default().levels(&feed());
        assert!(close(levels.left.level, -12.0));
        assert!(close(levels.right.level, -6.0));
    }

    #[test]
    fn a_needle_can_show_a_single_channel() {
        let routing = Routing {
            left: Route {
                channels: "2".parse().unwrap(),
                ..Route::both(Source::Input)
            },
            right: Route {
                source: Source::Output,
                channels: "1".parse().unwrap(),
                combine: Combine::Sum,
            },
        };
        let levels = routing.levels(&feed());
        assert!(close(levels.left.level, -40.0));
        assert!(close(levels.right.level, -6.0));
    }

    #[test]
    fn both_needles_can_show_the_same_source() {
        let routing = Routing {
            left: Route::both(Source::Output),
            right: Route {
                combine: Combine::Sum,
                ..Route::both(Source::Output)
            },
        };
        let levels = routing.levels(&feed());
        assert!(close(levels.left.level, -6.0));
        assert!(close(levels.right.level, 0.02));
    }

    #[test]
    fn routes_read_plainly() {
        assert_eq!(
            Route::both(Source::Input).to_string(),
            "input channels 1,2, combined by max"
        );
        let single = Route {
            channels: "2".parse().unwrap(),
            ..Route::both(Source::Output)
        };
        assert_eq!(single.to_string(), "output channel 2");
        assert_eq!(Route::OFF.to_string(), "off");
    }

    #[test]
    fn an_off_needle_is_silent() {
        let routing = Routing {
            left: Route::OFF,
            ..Routing::default()
        };
        assert_eq!(routing.levels(&feed()).left, NeedleLevel::SILENT);
    }

    #[test]
    fn clipping_follows_the_loudest_channel_whatever_the_combination() {
        let mut levels = ChannelLevels::SILENT;
        levels.set_input(1, 0.0);
        let route = Route {
            combine: Combine::Rms,
            ..Route::both(Source::Input)
        };
        let fed = route.level(&levels);
        assert!(close(fed.level, -3.01));
        assert!(close(fed.peak, 0.0));
    }
}
