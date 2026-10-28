// SPDX-License-Identifier: Apache-2.0
//! Finding a UX2 and sending it panel commands.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use nusb::MaybeFuture;
use nusb::transfer::{ControlOut, ControlType, Recipient, TransferError};

use crate::protocol::{Clip, Command, PRODUCT_IDS, REQUEST, Reading, VENDOR_ID};

/// How long a command may take before it is given up.
const TIMEOUT: Duration = Duration::from_millis(100);

/// Why the panel could not be reached.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// No UX2 is plugged in.
    NotFound,
    /// The USB devices could not be listed.
    List(nusb::Error),
    /// The UX2 is there but could not be opened, most often for want of
    /// permission on its device file.
    Open(nusb::Error),
    /// The UX2 did not take a command, as when it was unplugged.
    Transfer(TransferError),
}

impl Error {
    /// Whether the UX2 is there but this user may not write to it.
    #[must_use]
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::Open(error) if error.kind() == nusb::ErrorKind::PermissionDenied)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("no UX2 is plugged in"),
            Self::List(error) => write!(formatter, "cannot list USB devices: {error}"),
            Self::Open(error) if self.is_permission_denied() => write!(
                formatter,
                "the UX2 cannot be opened: {error} (install the udev rule and join group audio)"
            ),
            Self::Open(error) => write!(formatter, "the UX2 cannot be opened: {error}"),
            Self::Transfer(error) => write!(formatter, "the UX2 did not take a command: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotFound => None,
            Self::List(error) | Self::Open(error) => Some(error),
            Self::Transfer(error) => Some(error),
        }
    }
}

/// A UX2 that is plugged in, not yet opened.
#[derive(Debug, Clone)]
pub struct Found {
    info: nusb::DeviceInfo,
}

impl Found {
    /// The bus it is on, as `001`.
    #[must_use]
    pub fn bus(&self) -> &str {
        self.info.bus_id()
    }

    /// Its address on the bus.
    #[must_use]
    pub fn address(&self) -> u8 {
        self.info.device_address()
    }

    /// Its USB product id.
    #[must_use]
    pub fn product_id(&self) -> u16 {
        self.info.product_id()
    }

    /// Line 6's name for it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self.info.product_id() {
            0x4142 => "TonePort UX2",
            _ => "POD Studio UX2",
        }
    }

    /// Its device file, where udev sets who may write to it.
    #[must_use]
    pub fn device_file(&self) -> PathBuf {
        let bus: u16 = self.bus().parse().unwrap_or_default();
        PathBuf::from(format!("/dev/bus/usb/{bus:03}/{:03}", self.address()))
    }

    /// Opens it for panel commands. No command is sent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Open`] when it cannot be opened, as when this user
    /// may not write to its device file.
    pub fn open(&self) -> Result<Panel, Error> {
        let device = self.info.open().wait().map_err(Error::Open)?;
        Ok(Panel {
            device,
            found: self.clone(),
            lamps: None,
        })
    }
}

impl fmt::Display for Found {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} ({:04x}:{:04x}) on bus {} device {:03}",
            self.name(),
            VENDOR_ID,
            self.product_id(),
            self.bus(),
            self.address()
        )
    }
}

/// The first UX2 plugged in, if there is one.
///
/// # Errors
///
/// Returns [`Error::List`] when the USB devices cannot be listed.
pub fn find() -> Result<Option<Found>, Error> {
    let found = nusb::list_devices()
        .wait()
        .map_err(Error::List)?
        .find(|device| {
            device.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&device.product_id())
        })
        .map(|info| Found { info });
    Ok(found)
}

/// An opened UX2's front panel.
///
/// Commands go to the device itself on the control endpoint, so no interface
/// is claimed and the kernel's audio driver keeps working. Dropping a panel
/// leaves the needles where they are; call [`Panel::rest`] first to put them
/// back.
#[derive(Debug)]
pub struct Panel {
    device: nusb::Device,
    found: Found,
    lamps: Option<Clip>,
}

impl Panel {
    /// Opens the first UX2 plugged in.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] when none is, or why it cannot be opened.
    pub fn open() -> Result<Self, Error> {
        find()?.ok_or(Error::NotFound)?.open()
    }

    /// The UX2 this panel belongs to.
    #[must_use]
    pub const fn found(&self) -> &Found {
        &self.found
    }

    /// Sends one command.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transfer`] when the UX2 does not take it.
    pub fn send(&self, command: Command) -> Result<(), Error> {
        self.device
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: REQUEST,
                    value: command.value,
                    index: command.index,
                    data: &[],
                },
                TIMEOUT,
            )
            .wait()
            .map_err(Error::Transfer)
    }

    /// Moves the needles, each from 0 at rest to
    /// [`FULL_SCALE`](crate::FULL_SCALE); higher values stop at full scale.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transfer`] when the UX2 does not take it.
    pub fn set_needles(&mut self, left: u8, right: u8) -> Result<(), Error> {
        self.send(Command::needles(left, right))
    }

    /// Sets the clip lamps. A lamp lights only while its needle is at full
    /// scale; [`Panel::show`] takes care of that.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transfer`] when the UX2 does not take it.
    pub fn set_clip(&mut self, clip: Clip) -> Result<(), Error> {
        self.lamps = None;
        self.send(Command::clip(clip))?;
        self.lamps = Some(clip);
        Ok(())
    }

    /// Shows a reading: the needles first, a lit lamp's needle at full
    /// scale, then the lamps, which are sent again while any is lit and
    /// otherwise only when they change.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transfer`] when the UX2 does not take it.
    pub fn show(&mut self, reading: Reading) -> Result<(), Error> {
        let reading = reading.with_full_scale_rule();
        self.set_needles(reading.left, reading.right)?;
        if reading.clip != Clip::None || self.lamps != Some(reading.clip) {
            self.set_clip(reading.clip)?;
        }
        Ok(())
    }

    /// Puts both needles at rest and both lamps out.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transfer`] when the UX2 does not take it.
    pub fn rest(&mut self) -> Result<(), Error> {
        self.set_needles(0, 0)?;
        self.set_clip(Clip::None)
    }
}
