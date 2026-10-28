// SPDX-License-Identifier: Apache-2.0
//! The front panel's USB protocol: one vendor request with no data stage,
//! whose value says what to set and whose index carries the setting.

use std::fmt;
use std::str::FromStr;

/// Line 6's USB vendor id.
pub const VENDOR_ID: u16 = 0x0e41;

/// The POD Studio UX2, and the `TonePort` UX2 before it (not yet verified).
pub const PRODUCT_IDS: [u16; 2] = [0x4151, 0x4142];

/// `bmRequestType` of every panel command: host to device, vendor, device
/// recipient.
pub const REQUEST_TYPE: u8 = 0x40;

/// `bRequest` of every panel command.
pub const REQUEST: u8 = 0x67;

/// `wValue` that moves both needles: the left needle in the index's low
/// byte, the right in its high byte.
pub const NEEDLES: u16 = 0x0601;

/// `wValue` that sets the clip lamps, by [`Clip::code`] in the index.
pub const CLIP_LAMPS: u16 = 0x0201;

/// A needle at full scale. A needle at 0 is at rest.
pub const FULL_SCALE: u8 = 140;

/// Which clip lamps are lit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Clip {
    /// Both lamps out.
    #[default]
    None,
    /// The left lamp lit.
    Left,
    /// The right lamp lit.
    Right,
    /// Both lamps lit.
    Both,
}

impl Clip {
    /// The lamps for each side lit or not.
    #[must_use]
    pub const fn from_sides(left: bool, right: bool) -> Self {
        match (left, right) {
            (false, false) => Self::None,
            (true, false) => Self::Left,
            (false, true) => Self::Right,
            (true, true) => Self::Both,
        }
    }

    /// Whether the left lamp is lit.
    #[must_use]
    pub const fn left(self) -> bool {
        matches!(self, Self::Left | Self::Both)
    }

    /// Whether the right lamp is lit.
    #[must_use]
    pub const fn right(self) -> bool {
        matches!(self, Self::Right | Self::Both)
    }

    /// The `wIndex` that sets these lamps.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::None => 1,
            Self::Left => 2,
            Self::Right => 3,
            Self::Both => 4,
        }
    }
}

impl fmt::Display for Clip {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::None => "none",
            Self::Left => "left",
            Self::Right => "right",
            Self::Both => "both",
        })
    }
}

impl FromStr for Clip {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "none" => Ok(Self::None),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "both" => Ok(Self::Both),
            _ => Err(format!("`{text}` is not none, left, right or both")),
        }
    }
}

/// Everything the panel shows at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Reading {
    /// The left needle, from 0 at rest to [`FULL_SCALE`].
    pub left: u8,
    /// The right needle, from 0 at rest to [`FULL_SCALE`].
    pub right: u8,
    /// The clip lamps.
    pub clip: Clip,
}

impl Reading {
    /// Both needles at rest and both lamps out.
    pub const REST: Self = Self {
        left: 0,
        right: 0,
        clip: Clip::None,
    };

    /// This reading as the UX2 can show it. A clip lamp lights only while its
    /// needle is at full scale, so a lit lamp takes its needle there, and
    /// needles past full scale are held at it.
    #[must_use]
    pub fn with_full_scale_rule(self) -> Self {
        let needle = |position: u8, lit: bool| {
            if lit {
                FULL_SCALE
            } else {
                position.min(FULL_SCALE)
            }
        };
        Self {
            left: needle(self.left, self.clip.left()),
            right: needle(self.right, self.clip.right()),
            clip: self.clip,
        }
    }
}

/// One panel command: the `wValue` and `wIndex` of a [`REQUEST`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Command {
    /// What the command sets.
    pub value: u16,
    /// The setting.
    pub index: u16,
}

impl Command {
    /// Moves both needles, each held at [`FULL_SCALE`] at most.
    #[must_use]
    pub fn needles(left: u8, right: u8) -> Self {
        Self {
            value: NEEDLES,
            index: (u16::from(right.min(FULL_SCALE)) << 8) | u16::from(left.min(FULL_SCALE)),
        }
    }

    /// Sets the clip lamps.
    #[must_use]
    pub const fn clip(clip: Clip) -> Self {
        Self {
            value: CLIP_LAMPS,
            index: clip.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needles_go_left_in_the_low_byte_and_right_in_the_high() {
        assert_eq!(
            Command::needles(0x12, 0x34),
            Command {
                value: 0x0601,
                index: 0x3412
            }
        );
        assert_eq!(Command::needles(255, 200).index, 0x8c8c);
    }

    #[test]
    fn clip_codes_count_from_one() {
        let codes = [Clip::None, Clip::Left, Clip::Right, Clip::Both].map(Clip::code);
        assert_eq!(codes, [1, 2, 3, 4]);
        assert_eq!(Command::clip(Clip::Both).value, 0x0201);
    }

    #[test]
    fn clip_sides_round_trip() {
        for clip in [Clip::None, Clip::Left, Clip::Right, Clip::Both] {
            assert_eq!(Clip::from_sides(clip.left(), clip.right()), clip);
            assert_eq!(clip.to_string().parse::<Clip>(), Ok(clip));
        }
    }

    #[test]
    fn a_lit_lamp_takes_its_needle_to_full_scale() {
        let reading = Reading {
            left: 30,
            right: 90,
            clip: Clip::Right,
        };
        assert_eq!(
            reading.with_full_scale_rule(),
            Reading {
                left: 30,
                right: FULL_SCALE,
                clip: Clip::Right
            }
        );
        let past = Reading {
            left: 200,
            ..Reading::REST
        };
        assert_eq!(past.with_full_scale_rule().left, FULL_SCALE);
    }
}
