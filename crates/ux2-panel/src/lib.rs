// SPDX-License-Identifier: Apache-2.0
//! A userspace driver for the front panel of the Line 6 POD Studio UX2: its
//! two needles and two clip lamps.
//!
//! Linux's `snd-usb-toneport` plays and records through the UX2 but leaves
//! the panel still. Every panel command is a single vendor request on the
//! control endpoint addressed to the device, so it needs no interface and
//! works beside the kernel's driver while audio plays.
//!
//! The crate has a few layers, each usable on its own:
//!
//! - [`Panel`] sends [`Reading`]s to an opened UX2, and [`AutoPanel`] finds it
//!   again after it is unplugged.
//! - [`needle`] maps a level in dBFS onto the meter face.
//! - [`Ballistics`] turns levels into needle movement the way a VU meter
//!   moves: up at once, back down at a steady pace, the clip lamp held.
//! - [`Routing`] picks the channels each needle shows and combines them.
//! - [`feed`] reads the text level feed that `ux2ctl meter` listens to.
//!
//! ```no_run
//! use std::time::Duration;
//! use ux2_panel::{AutoPanel, Ballistics, ChannelLevels, Routing};
//!
//! let mut panel = AutoPanel::new();
//! let mut ballistics = Ballistics::default();
//! let routing = Routing::default();
//! let mut channels = ChannelLevels::SILENT;
//! channels.set_input(1, -12.0);
//! channels.set_output(1, -6.0);
//! let reading = ballistics.update(routing.levels(&channels), Duration::from_millis(20));
//! panel.show(reading)?;
//! # Ok::<(), ux2_panel::Error>(())
//! ```

mod ballistics;
mod curve;
pub mod feed;
mod panel;
mod protocol;
mod reconnect;
mod routing;

pub use ballistics::{Ballistics, BallisticsSettings, Levels, NeedleLevel};
pub use curve::{SILENCE, dbfs, needle};
pub use panel::{Error, Found, Panel, find};
pub use protocol::{
    CLIP_LAMPS, Clip, Command, FULL_SCALE, NEEDLES, PRODUCT_IDS, REQUEST, REQUEST_TYPE, Reading,
    VENDOR_ID,
};
pub use reconnect::AutoPanel;
pub use routing::{
    Channel, ChannelLevels, Channels, Combine, Direction, MAX_CHANNELS, Route, Routing, Source,
};
