// SPDX-License-Identifier: Apache-2.0
//! A short sweep of the needles and the lamps, to show the panel works. The
//! needles stay at full scale for well under a second, to spare the
//! movement.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ux2_panel::{Clip, FULL_SCALE, Reading};

const STEP: Duration = Duration::from_millis(20);
const RISE_STEPS: u8 = 25;
const FALL_STEPS: u8 = 35;
const LAMP: Duration = Duration::from_millis(200);

/// A position along the needle's travel, from 0 at rest to 1 at full scale.
fn position(fraction: f32) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped into 0..=140 first"
    )]
    let position = (fraction.clamp(0.0, 1.0) * f32::from(FULL_SCALE)).round() as u8;
    position
}

fn both(needle: u8, clip: Clip) -> Reading {
    Reading {
        left: needle,
        right: needle,
        clip,
    }
}

/// The sweep: each reading with how long it stays. The needles rise, light
/// the left lamp, the right, then both, and fall back to rest.
#[must_use]
pub fn frames() -> Vec<(Reading, Duration)> {
    let rise = (1..=RISE_STEPS).map(|step| {
        let left = 1.0 - f32::from(step) / f32::from(RISE_STEPS);
        (both(position(1.0 - left * left), Clip::None), STEP)
    });
    let lamps = [Clip::Left, Clip::Right, Clip::Both].map(|clip| (both(FULL_SCALE, clip), LAMP));
    let fall = (1..=FALL_STEPS).map(|step| {
        let left = 1.0 - f32::from(step) / f32::from(FALL_STEPS);
        (both(position(left * left), Clip::None), STEP)
    });
    rise.chain(lamps).chain(fall).collect()
}

/// Plays the sweep through `show`, stopping early once `stop` is set.
///
/// # Errors
///
/// Returns the first error from `show`.
pub fn play<E>(mut show: impl FnMut(Reading) -> Result<(), E>, stop: &AtomicBool) -> Result<(), E> {
    for (reading, hold) in frames() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        show(reading)?;
        std::thread::sleep(hold);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_starts_moving_and_ends_at_rest() {
        let frames = frames();
        assert!(frames[0].0.left > 0);
        assert_eq!(
            frames.last().map(|(reading, _)| *reading),
            Some(Reading::REST)
        );
    }

    #[test]
    fn the_needles_stay_at_full_scale_under_a_second() {
        let mut at_full_scale = Duration::ZERO;
        let mut longest = Duration::ZERO;
        for (reading, hold) in frames() {
            let reading = reading.with_full_scale_rule();
            if reading.left == FULL_SCALE || reading.right == FULL_SCALE {
                at_full_scale += hold;
                longest = longest.max(at_full_scale);
            } else {
                at_full_scale = Duration::ZERO;
            }
        }
        assert!(longest > Duration::ZERO);
        assert!(longest < Duration::from_secs(1), "{longest:?}");
    }

    #[test]
    fn the_lamps_light_only_at_full_scale() {
        for (reading, _) in frames() {
            assert_eq!(reading, reading.with_full_scale_rule());
        }
    }

    #[test]
    fn the_whole_sweep_is_short() {
        let total: Duration = frames().iter().map(|(_, hold)| *hold).sum();
        assert!(total < Duration::from_secs(2), "{total:?}");
    }
}
