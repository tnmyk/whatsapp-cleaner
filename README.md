# WhatsApp Cleaner

A desktop app for finding and deleting business chats from your WhatsApp account.

![WhatsApp Cleaner](./assets/screenshot.png)

## Install

### Recommended: cargo install

```sh
cargo install --git https://github.com/tnmyk/whatsapp-cleaner
whatsapp-cleaner
```

Installs into `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). macOS needs Xcode Command Line Tools; Windows needs the MSVC Rust toolchain and Visual Studio C++ Build Tools.

### Prebuilt downloads

Both builds are unsigned, so your OS may show a security prompt.

**macOS (Apple Silicon):** download `WhatsApp-Cleaner-<version>-macos-arm64.zip` from [Releases](https://github.com/tnmyk/whatsapp-cleaner/releases), unzip, and drag `WhatsApp Cleaner.app` into Applications.

**Windows (x64):** download `whatsapp-cleaner-<version>-windows-x86_64.zip` from [Releases](https://github.com/tnmyk/whatsapp-cleaner/releases), unzip, and run `whatsapp-cleaner.exe`. If SmartScreen appears, choose **More info → Run anyway**.

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

- [GPUI](https://gpui.rs) — gpui-pre
- [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust) — WhatsApp Web client
- `qrcode` + `image` — QR pairing and avatars

## Notes

- Sessions are intentionally ephemeral: the app starts unpaired every launch and stores nothing across runs.
- Only chats delivered in WhatsApp history sync can appear.
- Delete sends a `deleteChat` app-state mutation with the chat's last-message range, matching WhatsApp Web behavior. It removes the chat and its media from your account.
