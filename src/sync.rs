use super::*;
use reqwest::blocking::Client;
use std::collections::BTreeSet;
#[derive(serde::Serialize, Deserialize)]
#[derive(Clone)]
pub struct Connection {
    pub server: String,
    pub token: String,
    /// Named vault on the server (`Vaults/<name>/`). `None` syncs with the server's main folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
}
fn config_file(v: &Vault) -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or_else(|| anyhow!("Missing configuration directory"))?
        .join("Emerald")
        .join(format!(
            "{}.json",
            digest(v.root.to_string_lossy().as_bytes())
        )))
}
fn server_url(server: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(server)?;
    ensure!(
        url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))),
        "Use HTTPS for a remote server"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Enter a server URL without credentials or query"
    );
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}
/// The address that one vault's `files` and `file` endpoints hang off.
fn vault_url(server: &str, vault: Option<&str>) -> Result<reqwest::Url> {
    let mut url = server_url(server)?;
    if let Some(name) = vault {
        let name = vault_name(name)?;
        url.path_segments_mut()
            .map_err(|_| anyhow!("Invalid server URL"))?
            .pop_if_empty()
            .extend(["vaults", name, ""]);
    }
    Ok(url)
}
fn client(timeout: bool) -> Result<Client> {
    let b = Client::builder().redirect(reqwest::redirect::Policy::none());
    Ok(if timeout { b.timeout(std::time::Duration::from_secs(10)) } else { b }.build()?)
}
pub fn configure(v: &Vault, mut c: Connection) -> Result<()> {
    server_url(&c.server)?;
    ensure!(c.token.len() >= 32, "Enter the server token");
    c.vault = match c.vault.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(n) => Some(vault_name(n)?.to_owned()),
    };
    atomic(&config_file(v)?, &serde_json::to_vec(&c)?)
}
/// The connection this vault syncs with, from the environment or its saved settings.
pub fn connection(v: &Vault) -> Option<Connection> {
    env_connection().or_else(|| serde_json::from_slice(&fs::read(config_file(v).ok()?).ok()?).ok())
}
/// Named vaults on a server, sorted by name.
pub fn vaults(server: &str, token: &str) -> Result<Vec<String>> {
    let r = client(true)?.get(server_url(server)?.join("vaults")?).bearer_auth(token).send()?;
    ensure!(
        r.status() != reqwest::StatusCode::NOT_FOUND,
        "This server has no named vaults. Update emerald-server to version 2.1 or later"
    );
    Ok(r.error_for_status()?.json()?)
}
/// Create an empty named vault on the server. Succeeds if it already exists.
pub fn create_vault(server: &str, token: &str, name: &str) -> Result<()> {
    let mut url = server_url(server)?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("Invalid server URL"))?
        .pop_if_empty()
        .extend(["vaults", vault_name(name)?]);
    let r = client(true)?.put(url).bearer_auth(token).send()?;
    ensure!(
        r.status() != reqwest::StatusCode::NOT_FOUND && r.status() != reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "This server has no named vaults. Update emerald-server to version 2.1 or later"
    );
    r.error_for_status()?;
    Ok(())
}
/// What one sync pass did.
#[derive(Default, Clone, Copy, Debug)]
pub struct Summary {
    pub sent: usize,
    pub received: usize,
    pub conflicts: usize,
}
impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.sent + self.received == 0 && self.conflicts == 0 {
            return write!(f, "Up to date");
        }
        write!(f, "Sent {}, received {}", self.sent, self.received)?;
        if self.conflicts > 0 {
            write!(f, ", {} conflict{} saved as copies", self.conflicts, if self.conflicts == 1 { "" } else { "s" })?;
        }
        Ok(())
    }
}
fn env_connection() -> Option<Connection> {
    match (std::env::var("EMERALD_SERVER"), std::env::var("EMERALD_TOKEN")) {
        (Ok(server), Ok(token)) => Some(Connection {
            server,
            token,
            vault: std::env::var("EMERALD_VAULT").ok().filter(|n| !n.trim().is_empty()),
        }),
        _ => None,
    }
}
/// The configured server address, the named server vault (if any), and whether they come
/// from environment variables.
pub fn current(v: &Vault) -> Option<(String, Option<String>, bool)> {
    if let Some(c) = env_connection() {
        return Some((c.server, c.vault, true));
    }
    let c: Connection = serde_json::from_slice(&fs::read(config_file(v).ok()?).ok()?).ok()?;
    Some((c.server, c.vault, false))
}
/// Forget the saved connection for this vault. Environment variables are left alone.
pub fn disconnect(v: &Vault) -> Result<()> {
    match fs::remove_file(config_file(v)?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
pub fn run(v: Vault) -> Result<Summary> {
    let c = match env_connection() {
        Some(c) => c,
        None => serde_json::from_slice(
            &fs::read(config_file(&v)?)
                .map_err(|_| anyhow!("Configure a server in Sync settings"))?,
        )?,
    };
    run_with(v, c)
}
fn run_with(v: Vault, c: Connection) -> Result<Summary> {
    let _guard = v.lock.lock().unwrap();
    let url = vault_url(&c.server, c.vault.as_deref())?;
    let token = c.token;
    let http = client(false)?;
    let remote: Index = http
        .get(url.join("files")?)
        .bearer_auth(&token)
        .send()?
        .error_for_status()?
        .json()?;
    let local = index(&v.root)?;
    let state = v.root.join(format!(
        ".emerald-sync-{}.json",
        digest(url.as_str().as_bytes())
    ));
    let mut base: Index = match fs::read(&state) {
        Ok(b) => serde_json::from_slice(&b)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };
    let names: BTreeSet<_> = local
        .keys()
        .chain(remote.keys())
        .chain(base.keys())
        .cloned()
        .collect();
    let mut sum = Summary::default();
    for n in names {
        let (l, r, b) = (local.get(&n), remote.get(&n), base.get(&n));
        let p = target(&v.root, &n)?;
        let mut end = r.cloned();
        if l != r {
            let file = url.join("file")?;
            if r == b {
                let params = [
                    ("name", n.as_str()),
                    ("base", r.map(String::as_str).unwrap_or("")),
                ];
                let req = if let Some(hash) = l {
                    let data = fs::read(&p)?;
                    ensure!(&digest(&data) == hash, "Local file changed during sync");
                    http.put(file).body(data)
                } else {
                    http.delete(file)
                };
                req.query(&params)
                    .bearer_auth(&token)
                    .send()?
                    .error_for_status()?;
                end = l.cloned();
                sum.sent += 1;
            } else {
                sum.received += 1;
                let incoming = if let Some(hash) = r {
                    let data = http
                        .get(file)
                        .query(&[("name", &n)])
                        .bearer_auth(&token)
                        .send()?
                        .error_for_status()?
                        .bytes()?;
                    ensure!(&digest(&data) == hash, "Remote file changed during sync");
                    Some(data)
                } else {
                    None
                };
                if l != b {
                    if l.is_some() {
                        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("bin");
                        let copy = p.with_file_name(format!(
                            "{}-conflict-{:016x}.{}",
                            p.file_stem().unwrap().to_string_lossy(),
                            rand::random::<u64>(),
                            ext
                        ));
                        fs::copy(&p, copy)?;
                    }
                    sum.conflicts += 1;
                }
                if let Some(data) = incoming {
                    atomic(&p, &data)?
                } else if p.exists() {
                    fs::remove_file(p)?
                }
            }
        }
        if let Some(h) = end {
            base.insert(n, h);
        } else {
            base.remove(&n);
        }
        atomic(&state, &serde_json::to_vec(&base)?)?;
    }
    Ok(sum)
}
pub fn configured(v: &Vault) -> bool {
    (std::env::var("EMERALD_SERVER").is_ok() && std::env::var("EMERALD_TOKEN").is_ok())
        || config_file(v).map(|p| p.exists()).unwrap_or(false)
}
pub fn test(server: &str, token: &str, vault: Option<&str>) -> Result<String> {
    let vault = vault.map(str::trim).filter(|n| !n.is_empty());
    let url = vault_url(server, vault)?;
    let r = client(true)?.get(url.join("files")?).bearer_auth(token).send()?;
    ensure!(
        vault.is_none() || r.status() != reqwest::StatusCode::NOT_FOUND,
        "This server has no named vaults. Update emerald-server to version 2.1 or later"
    );
    let n: Index = r.error_for_status()?.json()?;
    Ok(match vault {
        Some(name) => format!("Connected. The server vault \"{name}\" holds {} files.", n.len()),
        None => format!("Connected. The server holds {} files.", n.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn serve(dir: &Path, token: &str) -> String {
        let app = crate::api().with_state(Vault::new(dir.to_owned(), token.to_owned()).unwrap());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let l = tokio::net::TcpListener::from_std(listener).unwrap();
                axum::serve(l, app).await.unwrap();
            })
        });
        format!("http://127.0.0.1:{}", addr.port())
    }
    fn sync_with(local: &Path, url: &str, token: &str, vault: Option<&str>) -> Summary {
        let v = Vault::new(local.to_owned(), String::new()).unwrap();
        // Skip the per-user config file so tests never touch real settings.
        run_with(v, Connection { server: url.into(), token: token.into(), vault: vault.map(Into::into) }).unwrap()
    }
    #[test]
    fn named_vaults_are_separate_from_the_main_vault() {
        let token = "t".repeat(40);
        let server = tempfile::tempdir().unwrap();
        let url = serve(server.path(), &token);
        assert_eq!(vaults(&url, &token).unwrap(), Vec::<String>::new());

        // Import: an existing folder uploads into Vaults/<name>/.
        let imported = tempfile::tempdir().unwrap();
        fs::create_dir_all(imported.path().join("Projects")).unwrap();
        fs::write(imported.path().join("Projects/Plan.md"), "# Plan").unwrap();
        fs::write(imported.path().join("Inbox.md"), "hi").unwrap();
        create_vault(&url, &token, "Work Notes").unwrap();
        assert_eq!(vaults(&url, &token).unwrap(), vec!["Work Notes".to_owned()]);
        let s = sync_with(imported.path(), &url, &token, Some("Work Notes"));
        assert_eq!(s.sent, 2);
        assert_eq!(fs::read_to_string(server.path().join("Vaults/Work Notes/Projects/Plan.md")).unwrap(), "# Plan");

        // The main vault does not see, and cannot write into, the named vaults.
        let main = tempfile::tempdir().unwrap();
        fs::write(main.path().join("Main.md"), "main").unwrap();
        assert_eq!(sync_with(main.path(), &url, &token, None).sent, 1);
        assert!(!main.path().join("Vaults").exists());
        assert_eq!(test(&url, &token, None).unwrap(), "Connected. The server holds 1 files.");
        let blocked = Client::new()
            .put(format!("{url}/file"))
            .query(&[("name", "Vaults/Work Notes/x.md"), ("base", "")])
            .bearer_auth(&token)
            .body("x")
            .send()
            .unwrap();
        assert_eq!(blocked.status(), reqwest::StatusCode::BAD_REQUEST);

        // A second computer downloads the named vault into an empty folder.
        let other = tempfile::tempdir().unwrap();
        let s = sync_with(other.path(), &url, &token, Some("Work Notes"));
        assert_eq!(s.received, 2);
        assert_eq!(fs::read_to_string(other.path().join("Inbox.md")).unwrap(), "hi");
        assert!(!other.path().join("Main.md").exists());

        // Bad names and wrong tokens are refused.
        assert!(create_vault(&url, &token, "../escape").is_err());
        assert!(create_vault(&url, &token, ".hidden").is_err());
        assert!(vaults(&url, &"x".repeat(40)).is_err());
    }
}
