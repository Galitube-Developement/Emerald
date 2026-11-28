<div align="center">

<img src="assets/emerald-256.png" alt="Emerald logo" width="128" height="128">

# Emerald

**Your notes. Your folder. Your server.**

A fast, native Markdown notebook written in Rust, plus an optional sync server you host yourself.
No account, no cloud lock-in, no telemetry. Just `.md` files you own.

[![Latest release](https://img.shields.io/github/v/release/Galitube-Developement/Emerald?color=2ecc71&label=release)](https://github.com/Galitube-Developement/Emerald/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-2ecc71)](LICENSE.md)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-555)

[**⬇ Download**](#download) · [Features](#features) · [Shortcuts](#shortcuts) · [Self-host sync](#self-host) · [Build from source](#build)

</div>

---

## ⚡ At a glance

| | |
|:--|:--|
| 📦 **~5 MB** | Client installer. The sync server is about 1 MB. |
| 🦀 **~3,200 lines** | of Rust for the client, server and sync engine combined. |
| 🖥️ **5 installers** | MSI, macOS DMG (Apple Silicon and Intel), Debian `.deb` and Flatpak. |
| 📝 **100% plain Markdown** | Every note is an ordinary `.md` file. Open it with any editor. |
| 🔒 **0 accounts, 0 telemetry** | Nothing leaves your machine unless you point it at your own server. |
| 💾 **1-second autosave** | Notes save as soon as you stop typing. |
| 🔄 **5-second sync** | Connected clients sync automatically and pick up server changes every five seconds. |

---

<a id="download"></a>

## ⬇ Download

Grab the latest build for your platform from the [releases page](https://github.com/Galitube-Developement/Emerald/releases/latest):

| Platform | Notebook client | Sync server (optional) |
|:--|:--|:--|
| 🪟 **Windows** x64 | [Emerald-Client_Windows_x64.msi](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Client_Windows_x64.msi) | [Emerald-Server_Windows_x64.msi](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Server_Windows_x64.msi) |
| 🍎 **macOS** Apple Silicon | [Emerald-Client_macOS_Arm64.dmg](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Client_macOS_Arm64.dmg) | [Emerald-Server_macOS_Arm64.dmg](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Server_macOS_Arm64.dmg) |
| 🍎 **macOS** Intel | [Emerald-Client_macOS_Intel_x64.dmg](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Client_macOS_Intel_x64.dmg) | [Emerald-Server_macOS_Intel_x64.dmg](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Server_macOS_Intel_x64.dmg) |
| 🐧 **Linux** Debian / Ubuntu | [Emerald-Client_Linux_x64.deb](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Client_Linux_x64.deb) | [Emerald-Server_Linux_x64.deb](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Server_Linux_x64.deb) |
| 🐧 **Linux** Flatpak | [Emerald-Client_Linux_x64.flatpak](https://github.com/Galitube-Developement/Emerald/releases/latest/download/Emerald-Client_Linux_x64.flatpak) | — |

Checksums are in [`SHA256SUMS`](https://github.com/Galitube-Developement/Emerald/releases/latest/download/SHA256SUMS). Packages are currently unsigned, so your OS may ask you to confirm the first launch.

Prefer to build it yourself? `cargo run --locked --bin emerald` works too.

---

<a id="features"></a>

## ✨ Features

- 🚀 **Native and lightweight.** A real Rust desktop window (egui), not a browser in disguise. It starts fast and stays small.
- 📂 **Your folder is the database.** Notes live in `Documents/Emerald` by default, or any folder you choose with `--vault`. Use Git, Dropbox or another editor alongside Emerald.
- 🔗 **Linked thinking.** Type `[[` to link notes. Follow links from the links row, from Preview, or from the **Linked from** backlinks panel.
- 🕸️ **Graph view.** See how your notes connect with Ctrl+G.
- 🔍 **Instant search.** Matches paths, text and tags. Press Enter to open the first hit.
- 🎯 **Command palette.** Ctrl+P / Ctrl+K jumps to any note or runs any command.
- 📅 **Daily notes.** One click opens today's dated entry.
- 👁️ **Markdown preview.** Rendered right inside the app.
- 🗑️ **Safe deletes.** Deleted notes go to the Trash, with a ten-second undo toast.
- 🎨 **Light, dark or system theme.** Plus zoom controls and restored tabs every time you reopen a vault.
- 🔄 **Self-hosted sync with real conflict handling.** Offline edits on two machines never overwrite each other. Emerald keeps both and lets you compare them side by side.
- 🤖 **AI memory ready.** Agents can use your Emerald server as a persistent, human-readable memory. See [AGENTS.md](AGENTS.md).

---

## 🚀 Open a notebook

Install the Emerald client package or run `cargo run --locked --bin emerald`. It opens a native Rust desktop window and stores notes in Documents/Emerald by default. Version 1.1 and later open existing 1.0 vaults and reuse their sync settings. To work in another folder, run:

```bash
emerald --vault "/path/to/notes"
```

<a id="shortcuts"></a>

### ⌨️ Shortcuts

| Key | Action |
| --- | --- |
| Ctrl+N | New note (the title is selected so you can name it right away) |
| Ctrl+O or Ctrl+Shift+F | Search notes (Enter opens the first hit, Esc clears) |
| Ctrl+P or Ctrl+K | Jump to a note or run a command (arrow keys to choose) |
| Ctrl+G | Toggle graph view |
| Ctrl+W | Close the current tab |
| Ctrl+S | Save now (notes also autosave one second after you stop typing) |
| Ctrl+= / Ctrl+− / Ctrl+0 | Zoom in, out, reset |

<details>
<summary><b>Getting around the interface</b></summary>

The search box is always at the top of the sidebar, with **New note** and **New folder** below it. Right-click a note for Rename, Move to folder, Duplicate, Copy link, Reveal and Delete; right-click a folder to add a note to it or rename it. Click a note's title to rename it; its folder is shown above as a breadcrumb. Type `[[` to link a note; links can be followed from the links row, from Preview and from the **Linked from** panel on the right. Deleted notes go to the Trash (vault name menu or Ctrl+P) and can be undone from the toast for ten seconds.

The bottom bar holds the vault switcher (click the vault name for recent vaults), the note count, and the sync status. **Settings** (gear icon) has sections for Sync, Appearance (light, dark or match system), Vault and Shortcuts. Open tabs are restored the next time you open the vault.

Create notes using paths such as `Projects/Ideas`. Save with Ctrl+S or Cmd+S. Preview renders Markdown. Follow a `[[Projects/Ideas]]` link using the links above the editor, or inspect the graph and backlinks. Search matches paths, text and tags. Daily note opens today's dated entry. You can put binary attachments in the vault using your file manager.

</details>

### 📁 Where things are stored

- **Vault:** the folder passed with `--vault` (default Documents/Emerald). Notes are plain `.md` files.
- **Sync connection:** `%APPDATA%/Emerald/<hash>.json` on Windows (the platform config dir elsewhere), or the `EMERALD_SERVER` and `EMERALD_TOKEN` environment variables.
- **Sync state:** `.emerald-sync-*.json` inside the vault.

---

<a id="self-host"></a>

## 🌐 Host your own sync server

> Keep every device in sync without handing your notes to anyone. The server stores them as ordinary files in a folder you pick.

See [SERVER_SETUP.md](SERVER_SETUP.md) for platform-specific installation and token setup.

Install the separate Emerald Server package on a machine with persistent storage. Pick a dedicated folder and generate a random shared secret (for example, with a password manager). The service account needs write access to that folder. In a Linux or macOS terminal:

```bash
read -rs EMERALD_TOKEN
export EMERALD_TOKEN
emerald-server --folder "/path/to/server-vault"
```

On Windows, the MSI installs the executable under Program Files. In PowerShell 7:

```powershell
$env:EMERALD_TOKEN = Read-Host -MaskInput "Server token"
& "$env:ProgramFiles\Emerald Server\emerald-server.exe" --folder "D:\EmeraldVault"
```

Both commands start a foreground process on 127.0.0.1:7878. On macOS, open the server DMG and run its PKG first. For remote clients, configure an HTTPS reverse proxy on the server. For example, with Caddy and a DNS name pointing to the machine:

```caddy
notes.example.org {
    reverse_proxy 127.0.0.1:7878
}
```

In each client, open **Settings › Sync** (or click **Sync off** in the bottom bar), enter the HTTPS address and the same token, press **Test**, then **Connect**. Connected clients sync automatically after five seconds of inactivity and check for server updates every five seconds; the bottom bar shows when the last sync happened. For testing on one machine use http://127.0.0.1:7878. The client saves the connection, including the token, in the current user's configuration directory outside the vault. Alternatively supply both EMERALD_SERVER and EMERALD_TOKEN in the client's environment.

<details>
<summary><b>How sync handles conflicts, deletions and limits</b></summary>

Sync compares file contents against the last sync, including renames (delete and create), attachments and deletions. A concurrent change rejected by the server can be retried. If both clients edited a note offline, the remote version takes its original name and the local edit gets a conflict filename. The bottom bar then shows a conflict count; **Settings › Sync** lists each one with **Compare…**, which shows both versions side by side and lets you keep either (the other moves to the Trash) or both. A concurrent deletion and edit preserves the edited content.

- File transfers support up to 64 MiB per upload.
- Dot-prefixed paths and symlinks are excluded.
- Use separate directories for client and server, and a single server process per server vault.
- The server token grants access to the whole vault.
- Maintain backups independently of sync.

</details>

---

<a id="build"></a>

## 🛠 Build packages

Install stable Rust and run:

```bash
cargo build --locked --release
python3 packaging/package.py
```

On Windows use Python and WiX 5 on PATH. On macOS the script uses pkgbuild and hdiutil. On Debian/Ubuntu it uses dpkg-deb. Output goes to dist/. The client packages are MSI, DMG and Debian. A separate Flatpak build uses `bash packaging/flatpak.sh` with the Freedesktop 25.08 SDK and runtime. The server packages are MSI, a PKG inside a DMG, and Debian. They install the server executable for manual startup. The GitHub Actions workflow builds unsigned packages on their respective platforms.

---

## 🧭 Current scope

**Available now:** the native desktop editor, local folders, Markdown preview, note links, backlinks, graph, search, daily notes and automatic self-hosted sync. The client currently loads the Markdown text of the vault to search and build links.

**Not in this release:** canvas, plugin compatibility, rich embeds, version history, mobile clients and end-to-end encryption.

---

<div align="center">

Made with 💚 by **Galitube Productions** · [MIT licensed](LICENSE.md)

If Emerald makes your notes feel like *yours* again, consider giving the repo a ⭐

</div>
