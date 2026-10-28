// SPDX-License-Identifier: Apache-2.0
//! A panel that is found when first shown, and found again after the UX2 is
//! unplugged and plugged back in.

use std::time::{Duration, Instant};

use crate::panel::{Error, Panel, find};
use crate::protocol::Reading;

/// How long a missing or failing UX2 is left before it is looked for again.
const RETRY: Duration = Duration::from_secs(2);

/// A UX2's front panel that survives being unplugged. Each
/// [`show`](Self::show) opens the UX2 if it is not open, at most once every
/// retry interval, and a reading the panel already shows is not sent again.
#[derive(Debug)]
pub struct AutoPanel {
    panel: Option<Panel>,
    shown: Option<Reading>,
    retry: Duration,
    next_attempt: Option<Instant>,
}

impl Default for AutoPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoPanel {
    /// A panel looked for again every two seconds while it is missing.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_retry(RETRY)
    }

    /// A panel looked for again every `retry` while it is missing.
    #[must_use]
    pub const fn with_retry(retry: Duration) -> Self {
        Self {
            panel: None,
            shown: None,
            retry,
            next_attempt: None,
        }
    }

    /// Whether the UX2 is open.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        self.panel.is_some()
    }

    /// The open panel, opened now if it is time to try.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] while waiting to try again, or why the
    /// UX2 could not be opened.
    pub fn panel(&mut self) -> Result<&mut Panel, Error> {
        if self.panel.is_none() {
            let now = Instant::now();
            if self.next_attempt.is_some_and(|next| now < next) {
                return Err(Error::NotFound);
            }
            match find().and_then(|found| found.ok_or(Error::NotFound)?.open()) {
                Ok(panel) => {
                    self.panel = Some(panel);
                    self.shown = None;
                    self.next_attempt = None;
                }
                Err(error) => {
                    self.next_attempt = Some(now + self.retry);
                    return Err(error);
                }
            }
        }
        self.panel.as_mut().ok_or(Error::NotFound)
    }

    /// Shows a reading unless the panel already shows it.
    ///
    /// # Errors
    ///
    /// Returns why the UX2 cannot be reached; it is looked for again later.
    pub fn show(&mut self, reading: Reading) -> Result<(), Error> {
        if self.panel.is_some() && self.shown == Some(reading) {
            return Ok(());
        }
        let sent = self.panel()?.show(reading);
        match sent {
            Ok(()) => self.shown = Some(reading),
            Err(_) => self.disconnect(),
        }
        sent
    }

    /// Puts the needles at rest and the lamps out, if the UX2 is open.
    ///
    /// # Errors
    ///
    /// Returns why the UX2 did not take it.
    pub fn rest(&mut self) -> Result<(), Error> {
        let Some(panel) = self.panel.as_mut() else {
            return Ok(());
        };
        let rested = panel.rest();
        match rested {
            Ok(()) => self.shown = Some(Reading::REST),
            Err(_) => self.disconnect(),
        }
        rested
    }

    fn disconnect(&mut self) {
        self.panel = None;
        self.shown = None;
        self.next_attempt = Some(Instant::now() + self.retry);
    }
}
