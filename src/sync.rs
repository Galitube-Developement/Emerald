use super::*;
use reqwest::blocking::Client;
use std::collections::BTreeSet;
#[derive(serde::Serialize, Deserialize)]
pub struct Connection {
    pub server: String,
    pub token: String,
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
pub fn configure(v: &Vault, c: Connection) -> Result<()> {
    server_url(&c.server)?;
    ensure!(c.token.len() >= 32, "Enter the server token");
    atomic(&config_file(v)?, &serde_json::to_vec(&c)?)
}
pub fn run(v: Vault) -> Result<String> {
    let _guard = v.lock.lock().unwrap();
    let c = match (
        std::env::var("EMERALD_SERVER"),
        std::env::var("EMERALD_TOKEN"),
    ) {
        (Ok(server), Ok(token)) => Connection { server, token },
        _ => serde_json::from_slice(
            &fs::read(config_file(&v)?)
                .map_err(|_| anyhow!("Configure a server in Sync settings"))?,
        )?,
    };
    let url = server_url(&c.server)?;
    let token = c.token;
    let http = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
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
    let mut conflicts = 0;
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
            } else {
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
                    conflicts += 1;
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
    Ok(format!(
        "Sync complete. {conflicts} conflicts. Local conflict copies sync on the next pass."
    ))
}
pub fn configured(v: &Vault) -> bool {
    (std::env::var("EMERALD_SERVER").is_ok() && std::env::var("EMERALD_TOKEN").is_ok())
        || config_file(v).map(|p| p.exists()).unwrap_or(false)
}
pub fn test(server: &str, token: &str) -> Result<String> {
    let url = server_url(server)?;
    let http = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let n: Index = http
        .get(url.join("files")?)
        .bearer_auth(token)
        .send()?
        .error_for_status()?
        .json()?;
    Ok(format!("Connected. The server holds {} files.", n.len()))
}
