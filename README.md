# Tonique DAW [WIP]

A rust based Digital Audio Workstation (DAW).

## Setup

For linux, install required dependencies from [alsa-sys](https://github.com/diwic/alsa-sys)

### Debian/Ubuntu/Mint

```bash
sudo apt install libasound2-dev
```

### Fedora/Centos

```bash
dnf install alsa-lib-devel
```

## Build and run in dev mode

```bash
cargo run
```

## Build and run in release

```bash
cargo run --release
```

## Tests and coverage

Install required tools:

```bash
cargo install cargo-tarpaulin
```

Tests only

```bash
cargo test
```

Tests and coverage

```bash
cargo tarpaulin --out Html
```

Then coverage report can be found in `tarpaulin-report.html`

[![codecov](https://codecov.io/github/rravelli/ToniqueDAW/graph/badge.svg?token=R3Y36TXT7D)](https://codecov.io/github/rravelli/ToniqueDAW)
