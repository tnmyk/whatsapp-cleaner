#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

if [[ -z "${VERSION:-}" ]]; then
  VERSION="$(sed -n 's/^version = "\([^"]*\)"[[:space:]]*$/\1/p' Cargo.toml | head -n 1)"
fi

if [[ -z "${VERSION}" ]]; then
  echo "error: could not determine version; set VERSION or add a version to Cargo.toml" >&2
  exit 1
fi

ARCH="$(uname -m)"
BINARY="${REPO_ROOT}/target/release/whatsapp-cleaner"

if [[ ! -f "${BINARY}" ]]; then
  echo "error: missing release binary at ${BINARY}" >&2
  echo "Build it first with: cargo build --release" >&2
  exit 1
fi

DIST_DIR="${REPO_ROOT}/dist"
APP_DIR="${DIST_DIR}/WhatsApp Cleaner.app"
APP_BINARY="${APP_DIR}/Contents/MacOS/whatsapp-cleaner"
INFO_PLIST="${APP_DIR}/Contents/Info.plist"
ZIP_PATH="${DIST_DIR}/WhatsApp-Cleaner-${VERSION}-macos-${ARCH}.zip"
TAR_PATH="${DIST_DIR}/whatsapp-cleaner-${VERSION}-macos-${ARCH}.tar.gz"

mkdir -p "${DIST_DIR}"
rm -rf "${APP_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS"

cp "${BINARY}" "${APP_BINARY}"
chmod +x "${APP_BINARY}"

cat > "${INFO_PLIST}" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>WhatsApp Cleaner</string>
  <key>CFBundleDisplayName</key>
  <string>WhatsApp Cleaner</string>
  <key>CFBundleIdentifier</key>
  <string>com.tnmyk.whatsapp-cleaner</string>
  <key>CFBundleExecutable</key>
  <string>whatsapp-cleaner</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

rm -f "${ZIP_PATH}" "${TAR_PATH}"
ditto -c -k --norsrc --keepParent "${APP_DIR}" "${ZIP_PATH}"

STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/whatsapp-cleaner-package.XXXXXX")"
cleanup() {
  rm -rf "${STAGING_DIR}"
}
trap cleanup EXIT

cp "${BINARY}" "${STAGING_DIR}/whatsapp-cleaner"
cp "${REPO_ROOT}/README.md" "${STAGING_DIR}/README.md"
tar -czf "${TAR_PATH}" -C "${STAGING_DIR}" whatsapp-cleaner README.md

printf 'Created release assets:\n'
printf '  %s\n' "${ZIP_PATH}"
printf '  %s\n' "${TAR_PATH}"
