// SPDX-License-Identifier: Apache-2.0
//! The level feed: plain text lines of levels in dBFS, which `ux2ctl meter`
//! reads on its standard input or its Unix socket.
//!
//! A line names channels, in any order and any subset:
//!
//! ```text
//! in1=-12.3 in2=-40 out1=-6 out2=-6
//! ```
//!
//! or, as a shortcut that bypasses the channel routing, gives the needles'
//! levels directly: two numbers for the left and the right needle, or one
//! for both.
//!
//! ```text
//! -12.3 -6
//! -18
//! ```
//!
//! Levels are numbers in dBFS; `-inf` is silence. Blank lines and lines
//! starting with `#` say nothing.

use std::fmt;

use crate::routing::Channel;

/// What one line of the feed says.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// Named channels' levels.
    Channels(Vec<(Channel, f32)>),
    /// The needles' levels, bypassing the routing.
    Needles {
        /// The left needle's level.
        left: f32,
        /// The right needle's level.
        right: f32,
    },
}

/// A line that is not a level feed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn level(text: &str) -> Result<f32, ParseError> {
    text.parse::<f32>()
        .ok()
        .filter(|level| !level.is_nan())
        .ok_or_else(|| ParseError(format!("`{text}` is not a level in dBFS")))
}

/// Reads one line of the feed: `None` for a line that says nothing.
///
/// # Errors
///
/// Returns [`ParseError`] for a line that is neither form, or that mixes
/// them.
pub fn parse_line(line: &str) -> Result<Option<Line>, ParseError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.iter().any(|word| word.contains('=')) {
        let channels = words
            .iter()
            .map(|word| {
                let (name, value) = word.split_once('=').ok_or_else(|| {
                    ParseError(format!("`{word}` is not a channel's level, as in1=-12"))
                })?;
                let channel = name.parse::<Channel>().map_err(ParseError)?;
                Ok((channel, level(value)?))
            })
            .collect::<Result<Vec<_>, ParseError>>()?;
        return Ok(Some(Line::Channels(channels)));
    }
    match words.as_slice() {
        [both] => {
            let both = level(both)?;
            Ok(Some(Line::Needles {
                left: both,
                right: both,
            }))
        }
        [left, right] => Ok(Some(Line::Needles {
            left: level(left)?,
            right: level(right)?,
        })),
        _ => Err(ParseError(
            "a line is `in1=… out1=…`, `LEFT RIGHT` or one level for both".to_owned(),
        )),
    }
}

/// Writes channels' levels as one feed line, ending in a newline.
#[must_use]
pub fn format_line(levels: &[(Channel, f32)]) -> String {
    let mut line = levels
        .iter()
        .map(|(channel, level)| format!("{channel}={level:.1}"))
        .collect::<Vec<_>>()
        .join(" ");
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Direction;

    fn channel(direction: Direction, number: u8) -> Channel {
        Channel::new(direction, number).unwrap()
    }

    #[test]
    fn named_channels_are_read_in_any_order() {
        let line = parse_line("out2=-6 in1=-12.3  in2=-inf").unwrap().unwrap();
        assert_eq!(
            line,
            Line::Channels(vec![
                (channel(Direction::Output, 2), -6.0),
                (channel(Direction::Input, 1), -12.3),
                (channel(Direction::Input, 2), f32::NEG_INFINITY),
            ])
        );
    }

    #[test]
    fn the_shortcut_gives_the_needles_directly() {
        assert_eq!(
            parse_line("-12 -6\n").unwrap(),
            Some(Line::Needles {
                left: -12.0,
                right: -6.0
            })
        );
        assert_eq!(
            parse_line("-18").unwrap(),
            Some(Line::Needles {
                left: -18.0,
                right: -18.0
            })
        );
    }

    #[test]
    fn blank_lines_and_comments_say_nothing() {
        assert_eq!(parse_line("   ").unwrap(), None);
        assert_eq!(parse_line("# levels").unwrap(), None);
    }

    #[test]
    fn bad_lines_are_refused() {
        for line in [
            "loud",
            "-1 -2 -3",
            "in1=-3 -6",
            "in3x=-1",
            "in1=",
            "in1=nan",
            "mic=-3",
        ] {
            assert!(parse_line(line).is_err(), "{line}");
        }
    }

    #[test]
    fn formatted_lines_read_back() {
        let levels = [
            (channel(Direction::Input, 1), -12.3),
            (channel(Direction::Output, 2), -3.0),
        ];
        let line = format_line(&levels);
        assert_eq!(line, "in1=-12.3 out2=-3.0\n");
        assert!(matches!(parse_line(&line), Ok(Some(Line::Channels(read))) if read.len() == 2));
    }
}
