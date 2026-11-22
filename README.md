# WhatsApp Cleaner

A desktop app for finding and deleting business chats from your WhatsApp account.

![WhatsApp Cleaner](./assets/screenshot.png)

## Install

Prebuilt releases: macOS (Apple Silicon) and Windows (x64). Both builds are unsigned.

### macOS app

1. Open [Releases](https://github.com/tnmyk/whatsapp-cleaner/releases).
2. Download `WhatsApp-Cleaner-<version>-macos-<arch>.zip` (for example, `WhatsApp-Cleaner-0.1.0-macos-arm64.zip`).
3. Unzip it and drag `WhatsApp Cleaner.app` into Applications.

The build is unsigned, so the first launch may be blocked by Gatekeeper. Either right-click the app and choose **Open**, or remove the quarantine attribute:

```sh
xattr -dr com.apple.quarantine "/Applications/WhatsApp Cleaner.app"
```

Only bypass Gatekeeper for a release you trust.

If macOS says the app is **damaged**, you are using the broken `v0.1.0` macOS build. Download `v0.1.1` or later, which is ad-hoc signed so Gatekeeper shows the normal “unidentified developer” prompt instead.

### Windows

1. Open [Releases](https://github.com/tnmyk/whatsapp-cleaner/releases).
2. Download `whatsapp-cleaner-<version>-windows-x86_64.zip`.
3. Unzip it anywhere and run `whatsapp-cleaner.exe`.

The build is unsigned, so SmartScreen may show “Windows protected your PC”. Choose **More info → Run anyway**. There is no installer yet; the app runs directly from the unzipped folder.

### Standalone binary (macOS)

Each release also ships `whatsapp-cleaner-<version>-macos-<arch>.tar.gz`, containing the binary and this README:

```sh
tar -xzf whatsapp-cleaner-<version>-macos-<arch>.tar.gz
mkdir -p ~/.local/bin
mv whatsapp-cleaner ~/.local/bin/
```

Make sure `~/.local/bin` is on your `PATH`, then run `whatsapp-cleaner`.

### cargo install from git

```sh
cargo install --git https://github.com/tnmyk/whatsapp-cleaner
whatsapp-cleaner
```

This installs the binary into `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). Windows builds need the MSVC Rust toolchain plus Visual Studio C++ Build Tools.

## Build from source

Requires a recent stable Rust toolchain. macOS additionally needs Xcode Command Line Tools; Windows needs the MSVC toolchain and Visual Studio C++ Build Tools.

```sh
git clone https://github.com/tnmyk/whatsapp-cleaner.git
cd whatsapp-cleaner
cargo build --release
```

Run it with:

```sh
./target/release/whatsapp-cleaner         # macOS
.\target\release\whatsapp-cleaner.exe     # Windows
```

`cargo run` works too. To build distributable archives locally:

```sh
./scripts/package-macos.sh          # macOS .app zip + tar.gz
./scripts/package-windows.ps1       # Windows .exe zip
```

Assets are written to `dist/`.

## Pairing

Scan the QR from **WhatsApp → Settings → Linked devices → Link a device**.

## What it does

- Links to WhatsApp by QR code
- Reads chats from history sync and filters direct chats to business accounts
- Shows profile picture, verified name, category, and description
- Search by name, category, or phone number
- Select chats individually or select all filtered results
- Deletes chats (and their media) across linked devices

## Stack

- [GPUI](https://gpui.rs) — GPU-accelerated UI (`gpui-pre` snapshots)
- [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust) — WhatsApp Web client
- `qrcode` + `image` — QR pairing and avatars

## Notes

- Sessions are intentionally ephemeral: the app starts unpaired every launch and stores nothing across runs.
- Only chats delivered in WhatsApp history sync can appear.
- Delete sends a `deleteChat` app-state mutation with the chat's last-message range, matching WhatsApp Web behavior. It removes the chat and its media from your account.
- This project uses an unofficial WhatsApp client. Use at your own risk.
