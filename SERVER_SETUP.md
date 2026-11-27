# Emerald Server setup

Emerald Server stores one shared vault as ordinary files in a folder you choose. Install the server package on a computer with persistent storage. Install Emerald Client on each device that will edit the vault.

## 1. Choose a server folder and make a token

Use a dedicated folder for the shared vault. Create a long random token on the server. Keep it private: anyone who has the token can read and change every file served by this server.

### Linux

Generate a 256-bit token and keep it in a shell variable:

```sh
export EMERALD_TOKEN="$(openssl rand -hex 32)"
mkdir -p "$HOME/EmeraldVault"
```

Install the Emerald Server Debian package, then start the server:

```sh
sudo dpkg -i ./emerald-server-amd64.deb
emerald-server --folder "$HOME/EmeraldVault"
```

### macOS

Open `emerald-server.dmg`, then run the included `Emerald Server.pkg`. In Terminal:

```sh
export EMERALD_TOKEN="$(openssl rand -hex 32)"
mkdir -p "$HOME/EmeraldVault"
emerald-server --folder "$HOME/EmeraldVault"
```

The package installs the executable under `/usr/local/bin`.

### Windows

Install `emerald-server.msi`. In PowerShell, create and retain a random token in the current session:

```powershell
$bytes = New-Object byte[] 32
$rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
$rng.GetBytes($bytes)
$env:EMERALD_TOKEN = [Convert]::ToHexString($bytes).ToLower()
$rng.Dispose()
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\EmeraldVault"
```

Then start the server:

```powershell
& "$env:ProgramFiles\Emerald Server\emerald-server.exe" --folder "$env:USERPROFILE\EmeraldVault"
```

The token exists only in that PowerShell session. Keep the window open while the server runs. For unattended startup, store the token in a protected service-manager secret or credential store. Do not put it in a script or share it.

## 2. Connect clients on the same computer

The server listens on `127.0.0.1:7878` by default. In Emerald Client, open **Sync settings**, enter `http://127.0.0.1:7878` and the server token, and save. Click **Sync** to transfer notes.

## 3. Connect over a network

The server binds to loopback by default, so remote devices cannot reach it directly. Put a reverse proxy with HTTPS on the server machine and forward requests to `127.0.0.1:7878`. For example, with Caddy and a DNS name pointing to the server:

```text
notes.example.org {
    reverse_proxy 127.0.0.1:7878
}
```

Allow inbound HTTPS in the host firewall and router if the server is behind NAT. Avoid exposing port 7878 directly to the public internet because the server API uses a bearer token without TLS. In each client, enter `https://notes.example.org` and the same token in **Sync settings**.

The client requires HTTPS for remote servers. Plain HTTP is permitted only for localhost testing. Give the server process write access to its vault folder and keep backups independently of sync.

## Stop and restart

Stop a foreground server with Ctrl+C. Restart it using the same token and folder. Clients store their sync URL and token in the current user's application configuration. The sync data is stored as ordinary files in the chosen server folder.
