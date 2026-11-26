use anyhow::{anyhow, ensure, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
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
pub fn target(root: &Path, name: &str) -> Result<PathBuf> {
    let mut p = root.to_owned();
    for part in name.split('/') {
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
        p.push(part);
        ensure!(!p.is_symlink(), "Symlinks are unsupported");
    }
    Ok(p)
}
pub fn index(root: &Path) -> Result<Index> {
    let mut out = Index::new();
    for entry in walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
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
async fn list(State(v): State<Vault>, h: HeaderMap) -> Result<Json<Index>, ErrResp> {
    check(&v, &h)?;
    let _g = v.lock.lock().unwrap();
    Ok(Json(index(&v.root).map_err(bad)?))
}
#[derive(Deserialize)]
struct File {
    name: String,
    base: Option<String>,
}
async fn read(
    State(v): State<Vault>,
    h: HeaderMap,
    Query(q): Query<File>,
) -> Result<Response, ErrResp> {
    check(&v, &h)?;
    let p = target(&v.root, &q.name).map_err(bad)?;
    Ok(fs::read(p).map_err(bad)?.into_response())
}
fn change(v: Vault, h: HeaderMap, q: File, b: Option<Bytes>) -> Result<StatusCode, ErrResp> {
    check(&v, &h)?;
    let _g = v.lock.lock().unwrap();
    let p = target(&v.root, &q.name).map_err(bad)?;
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
async fn write(
    State(v): State<Vault>,
    h: HeaderMap,
    Query(q): Query<File>,
    b: Bytes,
) -> Result<StatusCode, ErrResp> {
    change(v, h, q, Some(b))
}
async fn remove(
    State(v): State<Vault>,
    h: HeaderMap,
    Query(q): Query<File>,
) -> Result<StatusCode, ErrResp> {
    change(v, h, q, None)
}
pub fn api() -> Router<Vault> {
    Router::new()
        .route("/files", get(list))
        .route("/file", get(read).put(write).delete(remove))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
}
pub fn client() -> Router<Vault> {
    api()
        .route(
            "/connection",
            post(
                |State(v): State<Vault>, h: HeaderMap, Json(c): Json<sync::Connection>| async move {
                    check(&v, &h)?;
                    sync::configure(&v, c).map_err(bad)
                },
            ),
        )
        .route(
            "/",
            get(|| async { axum::response::Html(include_str!("ui.html")) }),
        )
        .route(
            "/preview",
            post(
                |State(v): State<Vault>, h: HeaderMap, s: String| async move {
                    check(&v, &h)?;
                    let mut html = String::new();
                    pulldown_cmark::html::push_html(
                        &mut html,
                        pulldown_cmark::Parser::new_ext(&s, pulldown_cmark::Options::all()),
                    );
                    Ok::<_, ErrResp>(ammonia::clean(&html))
                },
            ),
        )
        .route(
            "/sync",
            post(|State(v): State<Vault>, h: HeaderMap| async move {
                check(&v, &h)?;
                tokio::task::spawn_blocking(move || sync::run(v))
                    .await
                    .map_err(bad)?
                    .map_err(bad)
            }),
        )
}
mod sync;
