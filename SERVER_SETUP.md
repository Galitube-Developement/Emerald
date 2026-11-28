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

The server listens on `127.0.0.1:7878` by default. In Emerald Client, open **Settings › Sync** (or click **Sync off** in the bottom bar), enter `http://127.0.0.1:7878` and the server token, press **Test**, then **Connect**. Notes sync automatically from then on.

## 3. Connect over a network

The server binds to loopback by default, so remote devices cannot reach it directly. Put a reverse proxy with HTTPS on the server machine and forward requests to `127.0.0.1:7878`. For example, with Caddy and a DNS name pointing to the server:

```text
notes.example.org {
    reverse_proxy 127.0.0.1:7878
}
```

Allow inbound HTTPS in the host firewall and router if the server is behind NAT. Avoid exposing port 7878 directly to the public internet because the server API uses a bearer token without TLS. In each client, enter `https://notes.example.org` and the same token in **Settings › Sync**.

The client requires HTTPS for remote servers. Plain HTTP is permitted only for localhost testing. Give the server process write access to its vault folder and keep backups independently of sync.

### Without a domain name: Tailscale

If your devices are on a [Tailscale](https://tailscale.com) network, Tailscale can provide the HTTPS address for you. On the server machine, with the server running on `127.0.0.1:7878` and HTTPS certificates enabled for your tailnet:

```sh
tailscale serve --bg 7878
```

Tailscale prints an address such as `https://server-name.your-tailnet.ts.net`. Enter it in each client. Only devices on your tailnet can reach it.

## Run the server unattended (Linux, systemd)

Instead of keeping a terminal open, run the server as a service. Create a dedicated user and folder, and keep the token in a file only root can read:

```sh
sudo useradd --system --home /var/lib/emerald --create-home emerald
sudo install -d -m 700 /etc/emerald
printf 'EMERALD_TOKEN=%s\n' "$(openssl rand -hex 32)" | sudo tee /etc/emerald/server.env >/dev/null
sudo chmod 600 /etc/emerald/server.env
sudo cat /etc/emerald/server.env   # copy the token into each client
```

Copy `packaging/emerald-server.service` to `/etc/systemd/system/`, then:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now emerald-server
systemctl status emerald-server
```

The unit runs `/usr/bin/emerald-server --folder /var/lib/emerald/vault` (the Debian package path) as the `emerald` user. systemd reads the token file as root before starting the service, so the token never appears in the unit or on the command line. Combine it with Caddy or Tailscale above for remote devices.

## Stop and restart

Stop a foreground server with Ctrl+C. Restart it using the same token and folder. Clients store their sync URL and token in the current user's application configuration. The sync data is stored as ordinary files in the chosen server folder.
