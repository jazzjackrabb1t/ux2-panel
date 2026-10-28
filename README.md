# ux2-panel

A Linux userspace driver for the front panel of the **Line 6 POD Studio UX2**:
its two needles (VU meters) and its two clip lamps.

On Linux the kernel's `snd-usb-toneport` driver plays and records through the
UX2, but the needles never move
([line6linux bug #10](https://sourceforge.net/p/line6linux/bugs/10/)). The
panel is driven by Line 6's own vendor request, which the kernel never sends
for the meters. ux2-panel sends it from userspace, beside the kernel's driver,
while audio plays.

It is two things, like Line 6's own control panel:

- **`ux2-panel`**, a Rust library: find the UX2, move the needles, light the
  lamps, map levels onto the meter face, move the needles like a VU meter's,
  route channels to needles, and find the UX2 again after it is unplugged.
- **`ux2ctl`**, a command-line tool and meter daemon: set the needles and
  lamps by hand, sweep them, and drive them from levels that any audio
  program writes to its standard input or a Unix socket, with routing and
  ballistics set in `/etc/ux2-panel.toml`.

## The protocol

Every command is one USB control transfer on endpoint 0, with no data stage:

| Field           | Value                                      |
| --------------- | ------------------------------------------ |
| `bmRequestType` | `0x40` (host to device, vendor, device)    |
| `bRequest`      | `0x67`                                     |
| `wLength`       | `0`                                        |

What it does is chosen by `wValue`:

| `wValue` | What           | `wIndex`                                                        |
| -------- | -------------- | --------------------------------------------------------------- |
| `0x0601` | Both needles   | `(right << 8) \| left`, each `0..=140` (0 at rest, 140 full scale) |
| `0x0201` | Clip lamps     | `1` none, `2` left, `3` right, `4` both                         |
| `0x0301` | *Device on*    | *sent by the kernel driver; not sent by ux2-panel*             |
| `0x0801`..`0x0b01` | *Input source* | *sent by the kernel driver; not sent by ux2-panel*  |

- A lamp lights **only while its needle is at full scale (140)**. So the lamp
  command is sent after the needle command, with the needle at 140.
  `Panel::show` does this for you.
- A request addressed to the device needs no claimed interface, so it works
  beside `snd-usb-toneport` without detaching it.
- USB ids: vendor `0x0e41`; product `0x4151` (POD Studio UX2, verified) and
  `0x4142` (TonePort UX2, probably the same, not yet verified).

Where the needle points for a level was fitted to the markings on the meter
face:

```text
needle = 49.28 · 10^(dB/20) + 1.252 · dB + 87.59     clamped to 0..=140, 0 below -67 dBFS
```

## Inputs: what the UX2 records, and on which channels

The UX2 can record the instrument, the microphones, the line inputs, or
the instrument and a microphone at once, each on its own channel. The
kernel's driver offers the choice as the card's `PCM Capture Source`
control but nothing on Linux shows it; `ux2ctl input` does:

```sh
ux2ctl input                  # what it records now
ux2ctl input instrument       # the instrument on channels 1 and 2
ux2ctl input mic              # the microphones on channels 1 and 2
ux2ctl input line             # line left on channel 1, right on channel 2
ux2ctl input instrument+mic   # the instrument on 1, a microphone on 2
```

The choice goes through the kernel's own control (with `amixer`), so the
kernel keeps it and sends it again after a suspend.

| Input | ALSA item | Vendor request value |
|---|---|---|
| Microphone | 0 | `0x0a01` |
| Line | 1 | `0x0801` |
| Instrument | 2 | `0x0b01` |
| Instrument and mic | 3 | `0x0901` |

Values from `0x0c01` to `0x1001` were tried on a POD Studio UX2: each left
both input channels silent. If you find another pairing the UX2 can
record, please open an issue with what you plugged where.

## Install

You need a Rust toolchain (stable) to build.

```sh
git clone https://github.com/jazzjackrabb1t/ux2-panel
cd ux2-panel
./install.sh
sudo usermod -aG audio "$USER"     # then log in again
ux2ctl status
```

`install.sh` builds `ux2ctl` and installs:

- `/usr/local/bin/ux2ctl`;
- `/etc/udev/rules.d/70-ux2-panel.rules`, which gives group `audio` read and
  write on the UX2's device file;
- `/etc/ux2-panel.toml`, the settings, unless it is there already;
- a system user `ux2-panel` (in group `audio`) and the `ux2-panel` service,
  which runs `ux2ctl meter --socket /run/ux2-panel/meter.sock`.

Options: `--binary PATH` installs a binary built elsewhere (for example
cross-built for a Raspberry Pi), `--prefix /usr` installs under another
prefix, `--no-service` leaves the service off.

Cross-building for a 64-bit Raspberry Pi from another machine:

```sh
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.36 -p ux2ctl
scp target/aarch64-unknown-linux-gnu/release/ux2ctl pi@raspberrypi.local:/tmp/
```

To remove it: `sudo systemctl disable --now ux2-panel`, then delete the files
listed above and `sudo userdel ux2-panel`.

## Usage

```sh
ux2ctl status                 # found or not, bus and device, writable or not
ux2ctl needles 70 35          # left needle at 70, right at 35 (0..=140)
ux2ctl clip left              # none | left | right | both
ux2ctl rest                   # needles at rest, lamps out
ux2ctl sweep                  # a short demo of the needles and lamps
ux2ctl meter                  # drive the needles from levels on stdin
ux2ctl meter --socket         # ... or on /run/ux2-panel/meter.sock
ux2ctl route                  # show what each needle shows
ux2ctl config show            # show the settings
ux2ctl config set KEY VALUE   # change one
ux2ctl config keys            # list the settings
```

`ux2ctl status` exits with 0 only when a UX2 is found and writable.
Remember that a lamp lights only while its needle is at full scale, so
`ux2ctl clip left` alone shows nothing until `ux2ctl needles 140 0`. The
sweep never holds the needles at full scale for more than about 0.6 s, to
spare the movement.

`ux2ctl meter` stops at the end of stdin, or on SIGINT or SIGTERM, and puts
the needles at rest. SIGHUP (`systemctl reload ux2-panel`) reads the settings
again. When the UX2 is unplugged it waits and drives it again once it is back.

## Routing

Like Line 6's control panel, each needle can show the **input** (what the UX2
records), the **output** (what it plays), or be **off**, chosen separately for
the left and the right needle. Each needle is fed by any set of the
direction's channels: the UX2 records on two (instrument or mic on one, the
other on the second, by input source) and plays on two. Several channels are
combined into one needle's level by:

| `combine` | Level shown                                           |
| --------- | ----------------------------------------------------- |
| `max`     | the loudest channel (the default)                     |
| `rms`     | the average power: `10·log10(mean(10^(dB/10)))`       |
| `sum`     | the amplitudes added: `20·log10(Σ 10^(dB/20))`        |

Whatever the combination, a needle's clip lamp lights when **any** of its
channels reaches the clip threshold.

The default is the input on the left needle and the output on the right,
both channels of each, the loudest shown. Examples:

```sh
ux2ctl route left input 1,2 --combine max   # left: the louder of both inputs
ux2ctl route right output 1                 # right: output channel 1 only
ux2ctl route both output 1,2 --combine rms  # both needles: the output's power
ux2ctl route right off                      # right needle stays at rest
sudo systemctl reload ux2-panel             # put it in effect
```

Leaving out the channels keeps the needle's current ones; leaving out
`--combine` keeps its current combination.

## Settings

`/etc/ux2-panel.toml` (or another file with `--config FILE`). Every key can be
set with `ux2ctl config set KEY VALUE`; a key left out of the file keeps its
default.

```toml
# how fast a needle falls back, in dB a second (VU: 26)
fall_rate = 26.0
# how long a clip lamp stays lit after clipping, in seconds
clip_hold = 0.7
# the level in dBFS at or above which a channel clips
clip_threshold = -0.2
# sweep the needles when the meter starts or the UX2 is plugged in: true or false
startup_sweep = true
# whether the needles move at all: false keeps the panel still
needles = true

[left]
# what the left needle shows: input, output or off
source = "input"
# which channels feed the left needle: 1, 2 or 1,2
channels = [1, 2]
# how several channels make the left needle's level: max, rms or sum
combine = "max"

[right]
source = "output"
channels = [1, 2]
combine = "max"
```

The keys for `config set` are `fall_rate`, `clip_hold`, `clip_threshold`,
`startup_sweep`, `needles`, `left.source`, `left.channels`, `left.combine`,
`right.source`, `right.channels` and `right.combine`, for example
`sudo ux2ctl config set left.channels 1` or
`sudo ux2ctl config set fall_rate 20`.

The needles rise at once, fall back at `fall_rate`, and a clipping needle is
held at full scale with its lamp lit for `clip_hold`.

## Feeding levels: the API for audio programs

`ux2ctl meter` reads plain text lines of levels in **dBFS**, on its standard
input or on its Unix socket (`/run/ux2-panel/meter.sock` for the service,
writable by group `audio`). Any number of programs can connect to the socket
at once.

A line names channels, `in1`..`in8` and `out1`..`out8`, in any order and any
subset:

```text
in1=-12.3 in2=-40 out1=-6 out2=-6
```

The routing decides which of them each needle shows. Channels not named in a
line keep their last level, so one program can feed the inputs and another
the outputs.

As a shortcut, a line can give the needles' levels directly, bypassing the
channel routing (a needle that is `off` still stays off): two numbers for the
left and the right needle, or one for both.

```text
-12.3 -6
-18
```

Rules:

- Send the **peak** level of each window, about 20 to 60 times a second. The
  meter moves 50 times a second, keeps the loudest level that came in each
  frame, and does its own ballistics, so don't smooth the levels first.
- `-inf` is silence. A channel not fed for 0.5 s is taken as silent.
- Blank lines and lines starting with `#` are ignored; bad lines are reported
  on stderr and ignored.

From a shell:

```sh
printf 'in1=-12 out1=-6\n' | socat - UNIX-CONNECT:/run/ux2-panel/meter.sock
```

From Python:

```python
import socket
meter = socket.socket(socket.AF_UNIX)
meter.connect("/run/ux2-panel/meter.sock")
meter.sendall(b"in1=-12.3 in2=-40 out1=-6 out2=-6\n")
```

### From Rust

A Rust program that would rather drive the panel itself (as
[Kabin](https://github.com/kabin-audio/kabin) does) depends on the library:

```toml
[dependencies]
ux2-panel = { git = "https://github.com/jazzjackrabb1t/ux2-panel", tag = "v0.1.0" }
```

```rust
use std::time::Duration;
use ux2_panel::{AutoPanel, Ballistics, ChannelLevels, Routing, dbfs};

let mut panel = AutoPanel::new();          // finds the UX2, again after an unplug
let mut ballistics = Ballistics::default(); // 26 dB/s fall, 0.7 s clip hold
let routing = Routing::default();           // input left, output right

// Every frame:
let mut channels = ChannelLevels::SILENT;
channels.set_input(1, dbfs(input_peak));    // dbfs() turns a linear peak into dBFS
channels.set_output(1, dbfs(output_peak));
let reading = ballistics.update(routing.levels(&channels), Duration::from_millis(20));
panel.show(reading)?;                       // needles first, lamps at full scale
```

`Panel` is the plain device (`Panel::open()`, `set_needles`, `set_clip`,
`show`, `rest`), `needle(db)` is the curve, and `feed::parse_line` and
`feed::format_line` read and write the text feed. The library needs only
[`nusb`](https://crates.io/crates/nusb), a pure-Rust USB library: no libusb.

## Credits

The needle curve and the first public description of the panel's commands
are from [UX2-VU-util](https://github.com/Leocraft1/UX2-VU-util) by Leocraft1
(MIT). ux2-panel is a separate implementation and contains none of its code.

## Licence

Apache License, Version 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).
