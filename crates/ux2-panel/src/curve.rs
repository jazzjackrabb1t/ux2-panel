// SPDX-License-Identifier: Apache-2.0
//! Where the needle points for a level, fitted to the markings on the UX2's
//! meter face.

use crate::protocol::FULL_SCALE;

/// The level taken as silence, in dBFS: far below the needle's travel.
pub const SILENCE: f32 = -120.0;

/// Below this level, in dBFS, the needle rests.
const BOTTOM: f32 = -67.0;

/// Where a needle points for a level in dBFS: at rest below -67 dBFS, near
/// full scale at 0 dBFS, following
/// `49.28 · 10^(dB/20) + 1.252 · dB + 87.59` in between.
#[must_use]
pub fn needle(decibels: f32) -> u8 {
    if decibels.is_nan() || decibels < BOTTOM {
        return 0;
    }
    let linear = 10.0_f32.powf(decibels / 20.0);
    let position = 49.28_f32.mul_add(linear, 1.252_f32.mul_add(decibels, 87.59));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped into 0..=140 first"
    )]
    let position = position.round().clamp(0.0, f32::from(FULL_SCALE)) as u8;
    position
}

/// A linear sample peak (1.0 at full scale) in dBFS, with silence at
/// [`SILENCE`].
#[must_use]
pub fn dbfs(peak: f32) -> f32 {
    let peak = peak.abs();
    if peak > 0.0 {
        (20.0 * peak.log10()).max(SILENCE)
    } else {
        SILENCE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_needle_rests_below_the_face_and_nears_full_scale_at_zero() {
        assert_eq!(needle(-90.0), 0);
        assert_eq!(needle(f32::NEG_INFINITY), 0);
        assert_eq!(needle(f32::NAN), 0);
        assert_eq!(needle(-20.0), 67);
        assert_eq!(needle(0.0), 137);
        assert_eq!(needle(6.0), FULL_SCALE);
    }

    #[test]
    fn the_needle_climbs_with_the_level() {
        let positions: Vec<u8> = (-66_i8..=0).map(|db| needle(f32::from(db))).collect();
        assert!(positions.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn peaks_become_decibels() {
        assert!((dbfs(1.0)).abs() < 1e-6);
        assert!((dbfs(0.5) + 6.0206).abs() < 1e-3);
        assert!((dbfs(-0.5) + 6.0206).abs() < 1e-3);
        assert!((dbfs(0.0) - SILENCE).abs() < f32::EPSILON);
    }
}
