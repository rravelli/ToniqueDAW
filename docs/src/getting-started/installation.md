# Installation

There are no packaged releases of Tonique yet, so you build it from source.
This takes a few minutes the first time.

## 1. Install Rust

Install the Rust toolchain from [rustup.rs](https://rustup.rs). Tonique needs
a recent stable Rust (1.88 or newer).

## 2. Install the audio libraries (Linux only)

On Linux, Tonique plays audio through ALSA, and building it needs the ALSA
development files:

| Distribution           | Command                             |
| ---------------------- | ----------------------------------- |
| Debian, Ubuntu, Mint   | `sudo apt install libasound2-dev`   |
| Fedora, CentOS         | `sudo dnf install alsa-lib-devel`   |
| Arch                   | `sudo pacman -S alsa-lib`           |

On macOS and Windows you don't need anything else.

## 3. Build and run

```bash
git clone https://github.com/rravelli/ToniqueDAW.git
cd ToniqueDAW
cargo run --release
```

Always use `--release` for real work. Debug builds are much slower and can
make the audio stutter.

## Updating

```bash
git pull
cargo run --release
```

Projects, settings and themes are kept when you update. See
[Files and folders](../reference/files.md) for where they are stored.
