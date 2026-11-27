# Emerald

Emerald is a Rust Markdown notebook and an optional self-hosted sync server. Each client keeps a local folder you can open with other editors. The server keeps another copy of the notes and attachments as ordinary files in the directory you select. One server instance serves one shared vault.

## Open a notebook

Install the Emerald client package or run `cargo run --locked --bin emerald`. It opens a native Rust desktop window and stores notes in Documents/Emerald by default. Version 1.1 opens existing 1.0 vaults and reuses their sync settings. To work in another folder, run:

    emerald --vault "/path/to/notes"

Create notes using paths such as `Projects/Ideas`. Save with Ctrl+S or Cmd+S. Preview renders Markdown. Follow a `[[Projects/Ideas]]` link using the links above the editor, or inspect the graph and backlinks. Search matches paths, text and tags. Daily note opens today's dated entry. You can put binary attachments in the vault using your file manager.

## Host your own sync server

See [SERVER_SETUP.md](SERVER_SETUP.md) for platform-specific installation and token setup.

Install the separate Emerald Server package on a machine with persistent storage. Pick a dedicated folder and generate a random shared secret (for example, with a password manager). The service account needs write access to that folder. In a Linux or macOS terminal:

    read -rs EMERALD_TOKEN
    export EMERALD_TOKEN
    emerald-server --folder "/path/to/server-vault"

On Windows, the MSI installs the executable under Program Files. In PowerShell 7:

    $env:EMERALD_TOKEN = Read-Host -MaskInput "Server token"
    & "$env:ProgramFiles\Emerald Server\emerald-server.exe" --folder "D:\EmeraldVault"

Both commands start a foreground process on 127.0.0.1:7878. On macOS, open the server DMG and run its PKG first. For remote clients, configure an HTTPS reverse proxy on the server. For example, with Caddy and a DNS name pointing to the machine:

    notes.example.org {
        reverse_proxy 127.0.0.1:7878
    }

In each client, open **Sync settings**, enter the HTTPS address and the same token, save the connection and choose **Sync**. For testing on one machine use http://127.0.0.1:7878. The client saves the connection, including the token, in the current user's configuration directory outside the vault. Alternatively supply both EMERALD_SERVER and EMERALD_TOKEN in the client's environment.

Sync compares file contents against the last sync, including renames (delete and create), attachments and deletions. A concurrent change rejected by the server can be retried. If both clients edited a note offline, the remote version takes its original name and the local edit gets a conflict filename. Review it in the vault. A subsequent sync transfers the conflict copy. A concurrent deletion and edit preserves the edited content. File transfers support up to 64 MiB per upload. Dot-prefixed paths and symlinks are excluded. Use separate directories for client and server, and a single server process per server vault. The server token grants access to the whole vault. Maintain backups independently of sync.

## Build packages

Install stable Rust and run:

    cargo build --locked --release
    python3 packaging/package.py

On Windows use Python and WiX 5 on PATH. On macOS the script uses pkgbuild and hdiutil. On Debian/Ubuntu it uses dpkg-deb. Output goes to dist/. The client packages are MSI, DMG and Debian. A separate Flatpak build uses `bash packaging/flatpak.sh` with the Freedesktop 25.08 SDK and runtime. The server packages are MSI, a PKG inside a DMG, and Debian. They install the server executable for manual startup. The GitHub Actions workflow builds unsigned packages on their respective platforms.

## Current scope

The native desktop editor, local folders, Markdown preview, note links, backlinks, graph, search, daily notes and manual self-hosted sync are implemented. The client currently loads the Markdown text of the vault to search and build links. Canvas, plugin compatibility, rich embeds, version history, automatic background sync, mobile clients and end-to-end encryption are outside the current implementation.
