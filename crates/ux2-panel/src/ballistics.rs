// SPDX-License-Identifier: Apache-2.0
//! Needle movement like a VU meter's: up at once, back down at a steady
//! pace in decibels a second, and a clip lamp held lit for a moment.

use std::time::Duration;

use crate::curve::{SILENCE, needle};
use crate::protocol::{Clip, FULL_SCALE, Reading};

/// What one needle is fed at one moment, in dBFS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeedleLevel {
    /// The level the needle shows.
    pub level: f32,
    /// The loudest channel behind it, which decides whether it clips.
    pub peak: f32,
}

impl NeedleLevel {
    /// Silence.
    pub const SILENT: Self = Self::new(SILENCE);

    /// A needle fed by one channel, whose level is its peak.
    #[must_use]
    pub const fn new(level: f32) -> Self {
        Self { level, peak: level }
    }
}

/// What both needles are fed at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    /// The left needle.
    pub left: NeedleLevel,
    /// The right needle.
    pub right: NeedleLevel,
}

impl Levels {
    /// Silence on both needles.
    pub const SILENT: Self = Self::both(SILENCE);

    /// One level in dBFS for each needle.
    #[must_use]
    pub const fn new(left: f32, right: f32) -> Self {
        Self {
            left: NeedleLevel::new(left),
            right: NeedleLevel::new(right),
        }
    }

    /// The same level in dBFS on both needles.
    #[must_use]
    pub const fn both(level: f32) -> Self {
        Self::new(level, level)
    }
}

/// How the needles move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallisticsSettings {
    /// How fast a needle falls back, in decibels a second.
    pub fall_rate: f32,
    /// How long a clip lamp stays lit after the last level at or above
    /// [`clip_threshold`](Self::clip_threshold).
    pub clip_hold: Duration,
    /// The level in dBFS at or above which a needle's channels clip.
    pub clip_threshold: f32,
}

impl Default for BallisticsSettings {
    /// A VU meter's 26 dB a second, a 0.7 s clip hold, and clipping from
    /// -0.2 dBFS.
    fn default() -> Self {
        Self {
            fall_rate: 26.0,
            clip_hold: Duration::from_millis(700),
            clip_threshold: -0.2,
        }
    }
}

/// One needle's state.
#[derive(Debug, Clone, Copy)]
struct Needle {
    decibels: f32,
    clip_left_for: Duration,
}

impl Default for Needle {
    fn default() -> Self {
        Self {
            decibels: SILENCE,
            clip_left_for: Duration::ZERO,
        }
    }
}

impl Needle {
    fn follow(&mut self, fed: NeedleLevel, elapsed: Duration, settings: &BallisticsSettings) {
        let level = if fed.level.is_nan() {
            SILENCE
        } else {
            fed.level
        };
        let fallen = settings
            .fall_rate
            .mul_add(-elapsed.as_secs_f32(), self.decibels);
        self.decibels = level.max(fallen).max(SILENCE);
        self.clip_left_for = if fed.peak >= settings.clip_threshold {
            settings.clip_hold
        } else {
            self.clip_left_for.saturating_sub(elapsed)
        };
    }

    fn clipping(&self) -> bool {
        !self.clip_left_for.is_zero()
    }

    fn position(&self) -> u8 {
        if self.clipping() {
            FULL_SCALE
        } else {
            needle(self.decibels)
        }
    }
}

/// Both needles' movement. Feed it the levels every frame with the time
/// since the last frame; it answers with what the panel should show.
#[derive(Debug, Clone, Default)]
pub struct Ballistics {
    settings: BallisticsSettings,
    left: Needle,
    right: Needle,
}

impl Ballistics {
    /// Needles at rest that move by `settings`.
    #[must_use]
    pub fn new(settings: BallisticsSettings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    /// How the needles move.
    #[must_use]
    pub const fn settings(&self) -> &BallisticsSettings {
        &self.settings
    }

    /// Moves by `settings` from now on, from where the needles are.
    pub fn set_settings(&mut self, settings: BallisticsSettings) {
        self.settings = settings;
    }

    /// Follows `levels` for `elapsed`, and answers with the reading to show.
    /// A clipping needle is held at full scale with its lamp lit, since the
    /// UX2 lights a lamp only there.
    pub fn update(&mut self, levels: Levels, elapsed: Duration) -> Reading {
        self.left.follow(levels.left, elapsed, &self.settings);
        self.right.follow(levels.right, elapsed, &self.settings);
        self.reading()
    }

    /// What the panel shows now.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading {
            left: self.left.position(),
            right: self.right.position(),
            clip: Clip::from_sides(self.left.clipping(), self.right.clipping()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(100);

    #[test]
    fn a_needle_rises_at_once() {
        let mut meter = Ballistics::default();
        let reading = meter.update(Levels::new(-20.0, f32::NEG_INFINITY), FRAME);
        assert_eq!(reading.left, needle(-20.0));
        assert_eq!(reading.right, 0);
    }

    #[test]
    fn a_needle_falls_at_the_fall_rate() {
        let mut meter = Ballistics::new(BallisticsSettings {
            fall_rate: 10.0,
            ..BallisticsSettings::default()
        });
        meter.update(Levels::both(-10.0), FRAME);
        let reading = meter.update(Levels::SILENT, Duration::from_secs(1));
        assert_eq!(reading.left, needle(-20.0));
        let reading = meter.update(Levels::SILENT, Duration::from_secs(60));
        assert_eq!(reading, Reading::REST);
    }

    #[test]
    fn a_clip_lamp_holds_its_needle_at_full_scale_for_the_hold() {
        let mut meter = Ballistics::default();
        let reading = meter.update(Levels::new(0.0, -30.0), FRAME);
        assert_eq!(reading.clip, Clip::Left);
        assert_eq!(reading.left, FULL_SCALE);
        for _ in 0..6 {
            assert!(meter.update(Levels::SILENT, FRAME).clip.left());
        }
        let reading = meter.update(Levels::SILENT, FRAME);
        assert_eq!(reading.clip, Clip::None);
        assert!(reading.left < FULL_SCALE);
    }

    #[test]
    fn the_peak_decides_clipping_not_the_level() {
        let mut meter = Ballistics::default();
        let quiet_but_clipping = NeedleLevel {
            level: -12.0,
            peak: 0.0,
        };
        let reading = meter.update(
            Levels {
                left: NeedleLevel::SILENT,
                right: quiet_but_clipping,
            },
            FRAME,
        );
        assert_eq!(reading.clip, Clip::Right);
    }

    #[test]
    fn nan_is_silence() {
        let mut meter = Ballistics::default();
        assert_eq!(meter.update(Levels::both(f32::NAN), FRAME), Reading::REST);
    }
}
