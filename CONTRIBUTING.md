# Contributing

Thank you for helping the UX2's panel come alive on Linux. Issues and pull
requests are welcome: bug reports, other Line 6 devices with meters, packaging
for distributions, and feeds from audio programs.

## Before a pull request

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
shellcheck install.sh
```

CI runs the same checks. The code is stable Rust, edition 2024, with
`unsafe_code` forbidden and Clippy's pedantic lints on.

## Style

- Names and doc comments say what the code does; inline comments are rare and
  explain why, not what.
- Pure logic (the curve, ballistics, routing, the feed format, the settings
  file) has unit tests. Code that talks to the device stays thin.
- Keep the dependencies few.
- English only, in code and in the command-line output.

## Hardware

Anything that sends new commands to the device needs a report of what it
did on a real UX2: the product id (`ux2ctl status`), the command, and what
the panel showed. Never send the commands the kernel's driver owns (the same
request with values 0x0301 and 0x0801 to 0x0b01): they switch the device on
and choose its input source. And spare the movement: don't hold a needle at
full scale for more than about a second in tests or demos.

If you have a TonePort UX2 (product id 0x4142), a report that the panel works
on it, or does not, is very welcome.

## Licence

By contributing you agree that your contribution is licensed under the
Apache License, Version 2.0, as the rest of the project is. Don't submit
code copied from GPL-licensed projects.
