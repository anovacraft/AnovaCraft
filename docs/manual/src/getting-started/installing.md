# Installing and running

## Downloading the app

Ready-built versions are on the project's
[Releases page](https://github.com/anovacraft/AnovaCraft/releases). For
macOS (11 or later, Apple silicon or Intel), download the `macos-universal`
`.zip`, open it and drag **AnovaCraft.app** to your Applications folder. The app is not signed with an Apple Developer ID, so
macOS refuses to open it the first time: try to open it once, then choose
**Open Anyway** under System Settings ▸ Privacy & Security (on macOS 14 and
earlier you can instead right-click the app and choose **Open**). macOS
remembers the choice.

For Windows (64-bit), download the `windows-x64` `.zip`, unzip it anywhere
and run `AnovaCraft.exe`; there is no installer. Windows SmartScreen may warn
about an unrecognized app the first time: choose **More info ▸ Run anyway**.

## Building from source

AnovaCraft can also be built from source with Rust. You need the Rust toolchain
(install it from <https://rustup.rs>) and a copy of the repository.

```sh
git clone https://github.com/anovacraft/AnovaCraft.git
cd AnovaCraft
cargo run --release -p app-gui
```

The first build takes several minutes; later runs start in seconds.

### A standalone app

To build an application you can keep in your Applications folder or copy to
another computer:

- **macOS:** `./scripts/build-macos-app.sh` writes `AnovaCraft.app` to
  `dist/` (add `universal` for one app that runs on Apple silicon and Intel).
  It is built with whole-program optimization (the `dist` build profile),
  which takes longer than `cargo build --release` but makes the analyses run
  faster. It also writes a `.zip` of the app for sharing
  (`AnovaCraft-<version>-macos-universal.zip`). The app is not code-signed,
  so the first time allow it as described under Downloading the app.
- **Windows:** `./scripts/build-windows-exe.sh`, run on macOS or Linux with
  the MinGW-w64 cross compiler, writes `dist/AnovaCraft-windows-x64/` and a
  `.zip` of it (`AnovaCraft-<version>-windows-x64.zip`). Copy the folder to a Windows PC and run `AnovaCraft.exe`; no
  installer is needed.

## Running scripts without the window

Scripts can also run headless, for batch jobs and scheduled reports:

```sh
cargo run --release -p stat-script --bin anovacraft-run -- script.rhai
```

See [Scripting and recording](../reference/scripting.md).
