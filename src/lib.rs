use anyhow::{anyhow, ensure, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use subtle::ConstantTimeEq;
pub type Index = BTreeMap<String, String>;
pub fn digest(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}
pub fn atomic(p: &Path, b: &[u8]) -> Result<()> {
    let parent = p.parent().ok_or_else(|| anyhow!("Missing parent"))?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(b)?;
    tmp.as_file().sync_all()?;
    tmp.persist(p)?;
    Ok(())
}
fn valid_part(part: &str) -> Result<()> {
    ensure!(
        !part.is_empty()
            && !part.starts_with('.')
            && !part.ends_with([' ', '.'])
            && !part
                .chars()
                .any(|c| c.is_control() || "<>:\"\\|?*".contains(c)),
        "Invalid vault path"
    );
    let stem = part.split('.').next().unwrap().to_ascii_uppercase();
    ensure!(
        !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit()),
        "Reserved name"
    );
    Ok(())
}
pub fn target(root: &Path, name: &str) -> Result<PathBuf> {
    let mut p = root.to_owned();
    for part in name.split('/') {
        valid_part(part)?;
        p.push(part);
        ensure!(!p.is_symlink(), "Symlinks are unsupported");
    }
    Ok(p)
}
/// Server folder that holds the named vaults: `<server folder>/Vaults/<name>/…`.
pub const VAULTS_DIR: &str = "Vaults";
fn is_reserved_top(first: &str) -> bool {
    first.eq_ignore_ascii_case(VAULTS_DIR)
}
/// Check a server vault name: one folder name, no slashes, at most 100 characters.
pub fn vault_name(name: &str) -> Result<&str> {
    let name = name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= 100 && !name.contains('/'),
        "Enter a vault name without slashes"
    );
    valid_part(name).map_err(|_| anyhow!("That vault name can't be used as a folder name"))?;
    Ok(name)
}
pub fn index(root: &Path) -> Result<Index> {
    walk(root, false)
}
/// Like [`index`], but leaves out the top-level `Vaults` folder of a server root.
fn index_main(root: &Path) -> Result<Index> {
    walk(root, true)
}
fn walk(root: &Path, skip_vaults: bool) -> Result<Index> {
    let mut out = Index::new();
    for entry in walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !n.starts_with('.') && !(skip_vaults && e.depth() == 1 && e.file_type().is_dir() && is_reserved_top(&n))
        })
    {
        let e = entry?;
        if e.file_type().is_file() {
            let n = e
                .path()
                .strip_prefix(root)?
                .to_str()
                .ok_or_else(|| anyhow!("Non Unicode path"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let p = target(root, &n)?;
            out.insert(n, digest(&fs::read(p)?));
        }
    }
    Ok(out)
}
#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub token: String,
    pub lock: Arc<Mutex<()>>,
}
impl Vault {
    pub fn new(root: PathBuf, token: String) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            root: root.canonicalize()?,
            token,
            lock: Default::default(),
        })
    }
    pub fn auth(&self, h: &HeaderMap) -> bool {
        h.get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|v| bool::from(v.as_bytes().ct_eq(self.token.as_bytes())))
    }
}
type ErrResp = (StatusCode, String);
fn bad(e: impl ToString) -> ErrResp {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn check(v: &Vault, h: &HeaderMap) -> Result<(), ErrResp> {
    if v.auth(h) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "Authentication required".into()))
    }
}
/// Which vault a request is for: the server's main folder, or a named vault under `Vaults/`.
enum Scope {
    Main,
    Named(String),
}
impl Scope {
    fn root(&self, v: &Vault) -> Result<PathBuf, ErrResp> {
        match self {
            Scope::Main => Ok(v.root.clone()),
            Scope::Named(n) => {
                let n = vault_name(n).map_err(bad)?;
                target(&v.root, &format!("{VAULTS_DIR}/{n}")).map_err(bad)
            }
        }
    }
    fn file(&self, v: &Vault, name: &str) -> Result<PathBuf, ErrResp> {
        if matches!(self, Scope::Main) && is_reserved_top(name.split('/').next().unwrap_or("")) {
            return Err(bad("The Vaults folder is reserved for named vaults"));
        }
        target(&self.root(v)?, name).map_err(bad)
    }
}
fn list_in(v: &Vault, h: &HeaderMap, s: Scope) -> Result<Json<Index>, ErrResp> {
    check(v, h)?;
    let _g = v.lock.lock().unwrap();
    let root = s.root(v)?;
    Ok(Json(match s {
        Scope::Main => index_main(&root).map_err(bad)?,
        // A vault nobody has written to yet is simply empty.
        Scope::Named(_) if !root.exists() => Index::new(),
        Scope::Named(_) => index(&root).map_err(bad)?,
    }))
}
#[derive(Deserialize)]
struct File {
    name: String,
    base: Option<String>,
}
fn read_in(v: &Vault, h: &HeaderMap, s: Scope, q: File) -> Result<Response, ErrResp> {
    check(v, h)?;
    let p = s.file(v, &q.name)?;
    Ok(fs::read(p).map_err(bad)?.into_response())
}
fn change(v: &Vault, h: &HeaderMap, s: Scope, q: File, b: Option<Bytes>) -> Result<StatusCode, ErrResp> {
    check(v, h)?;
    let _g = v.lock.lock().unwrap();
    let p = s.file(v, &q.name)?;
    let old = match fs::read(&p) {
        Ok(b) => digest(&b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(bad(e)),
    };
    if q.base.as_ref() != Some(&old) {
        return Err((
            StatusCode::CONFLICT,
            "File changed. Reload before saving.".into(),
        ));
    }
    if let Some(bytes) = b {
        atomic(&p, &bytes).map_err(bad)?
    } else if !old.is_empty() {
        fs::remove_file(p).map_err(bad)?
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn list(State(v): State<Vault>, h: HeaderMap) -> Result<Json<Index>, ErrResp> {
    list_in(&v, &h, Scope::Main)
}
async fn read(State(v): State<Vault>, h: HeaderMap, Query(q): Query<File>) -> Result<Response, ErrResp> {
    read_in(&v, &h, Scope::Main, q)
}
async fn write(State(v): State<Vault>, h: HeaderMap, Query(q): Query<File>, b: Bytes) -> Result<StatusCode, ErrResp> {
    change(&v, &h, Scope::Main, q, Some(b))
}
async fn remove(State(v): State<Vault>, h: HeaderMap, Query(q): Query<File>) -> Result<StatusCode, ErrResp> {
    change(&v, &h, Scope::Main, q, None)
}
async fn named_list(State(v): State<Vault>, h: HeaderMap, UrlPath(n): UrlPath<String>) -> Result<Json<Index>, ErrResp> {
    list_in(&v, &h, Scope::Named(n))
}
async fn named_read(State(v): State<Vault>, h: HeaderMap, UrlPath(n): UrlPath<String>, Query(q): Query<File>) -> Result<Response, ErrResp> {
    read_in(&v, &h, Scope::Named(n), q)
}
async fn named_write(State(v): State<Vault>, h: HeaderMap, UrlPath(n): UrlPath<String>, Query(q): Query<File>, b: Bytes) -> Result<StatusCode, ErrResp> {
    change(&v, &h, Scope::Named(n), q, Some(b))
}
async fn named_remove(State(v): State<Vault>, h: HeaderMap, UrlPath(n): UrlPath<String>, Query(q): Query<File>) -> Result<StatusCode, ErrResp> {
    change(&v, &h, Scope::Named(n), q, None)
}
/// `GET /vaults`: the names of the server's named vaults, sorted.
async fn vaults(State(v): State<Vault>, h: HeaderMap) -> Result<Json<Vec<String>>, ErrResp> {
    check(&v, &h)?;
    let _g = v.lock.lock().unwrap();
    let mut out = Vec::new();
    match fs::read_dir(v.root.join(VAULTS_DIR)) {
        Ok(dir) => {
            for e in dir {
                let e = e.map_err(bad)?;
                let n = e.file_name().to_string_lossy().into_owned();
                if e.file_type().map_err(bad)?.is_dir() && vault_name(&n).is_ok() {
                    out.push(n);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(bad(e)),
    }
    out.sort_by_key(|n| n.to_lowercase());
    Ok(Json(out))
}
/// `PUT /vaults/{name}`: create an empty named vault. Creating one that exists is fine.
async fn create_vault(State(v): State<Vault>, h: HeaderMap, UrlPath(n): UrlPath<String>) -> Result<StatusCode, ErrResp> {
    check(&v, &h)?;
    let _g = v.lock.lock().unwrap();
    fs::create_dir_all(Scope::Named(n).root(&v)?).map_err(bad)?;
    Ok(StatusCode::NO_CONTENT)
}
pub fn api() -> Router<Vault> {
    Router::new()
        .route("/files", get(list))
        .route("/file", get(read).put(write).delete(remove))
        .route("/vaults", get(vaults))
        .route("/vaults/{vault}", put(create_vault))
        .route("/vaults/{vault}/files", get(named_list))
        .route("/vaults/{vault}/file", get(named_read).put(named_write).delete(named_remove))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
}
pub mod sync;
