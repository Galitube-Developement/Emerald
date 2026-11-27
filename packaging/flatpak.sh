#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
flatpak build-init "$stage/app" dev.emerald.Vault org.freedesktop.Sdk org.freedesktop.Platform 25.08
install -Dm755 target/release/emerald "$stage/app/files/bin/emerald"
install -Dm644 assets/emerald-256.png "$stage/app/files/share/icons/hicolor/256x256/apps/emerald.png"
install -Dm644 packaging/dev.emerald.Vault.desktop "$stage/app/files/share/applications/dev.emerald.Vault.desktop"
flatpak build-finish --command=emerald --share=network --socket=wayland --socket=fallback-x11 --share=ipc --device=dri --filesystem=xdg-documents "$stage/app"
flatpak build-export "$stage/repo" "$stage/app"
mkdir -p dist
flatpak build-bundle "$stage/repo" dist/emerald.flatpak dev.emerald.Vault
