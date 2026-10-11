# Releasing

Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml)
when a version tag is pushed.

## Cutting a release

1. Bump `version` in `Cargo.toml` (e.g. `0.2.0`) and run `cargo build` so
   `Cargo.lock` follows.
2. Commit on `master`: `git commit -am "chore: release 0.2.0"`.
3. Tag and push. The tag must be the version prefixed with `v`, or the
   workflow stops:

   ```bash
   git tag v0.2.0
   git push origin master v0.2.0
   ```

4. Wait for the workflow, then open the **draft** release on GitHub, edit the
   generated notes and publish it.

## What gets built

| Platform | Package | Contents |
|---|---|---|
| Linux x86_64 | `tonique-<version>-linux-x86_64.tar.gz` | `tonique` binary, `install.sh`, desktop entry and icon |
| macOS (Apple Silicon and Intel) | `tonique-<version>-macos-universal.dmg` | `Tonique.app` |
| Windows x86_64 | `tonique-<version>-windows-x86_64.zip` | `Tonique.exe` |

Each package also contains the license files.

`SHA256SUMS.txt` lists the checksums of all three.

To build the packages without releasing (e.g. to test a branch), run the
**Release** workflow by hand from the Actions tab: the packages are attached
to the run, versioned `<version>-dev.<commit>`.

## Packaging locally

- Linux: `cargo build --release && packaging/linux/package.sh <version>`
- macOS: build both `aarch64-apple-darwin` and `x86_64-apple-darwin`
  targets in release, then `packaging/macos/package.sh <version>`
- Windows (cross-compiled from Linux, needs `mingw-w64-gcc`):
  `cargo build --release --target x86_64-pc-windows-gnu`

Packages land in `dist/`.

## Not signed

The packages aren't signed with a certificate, so the systems warn on first
launch:

- **macOS**: "Tonique can't be opened" / "is damaged". Right-click the app →
  **Open**, or run `xattr -dr com.apple.quarantine /Applications/Tonique.app`.
  Removing this needs an Apple Developer ID and notarization.
- **Windows**: SmartScreen's "Windows protected your PC". Click **More info →
  Run anyway**. Removing this needs a code-signing certificate.
