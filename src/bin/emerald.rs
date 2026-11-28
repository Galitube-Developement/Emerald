#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use anyhow::{ensure, Result};
use clap::Parser;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use emerald::{atomic, digest, index, sync, target, Vault};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    vault: Option<PathBuf>,
    /// Open the log pane at start
    #[arg(long)]
    verbose: bool,
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let verbose = args.verbose;
    let first_run = args.vault.is_none() && prefs_file().is_some_and(|p| !p.exists());
    let path = args.vault.unwrap_or_else(|| {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default()
            .join("Emerald")
    });
    let viewport = egui::ViewportBuilder::default().with_inner_size([1060., 720.]);
    // On macOS, the Dock icon comes from the app bundle's AppIcon.icns. Setting a
    // runtime window icon here overrides it with the generic window PNG in the Dock.
    #[cfg(not(target_os = "macos"))]
    let viewport = {
        let icon = image::load_from_memory(include_bytes!("../../assets/emerald-256.png"))
            .expect("valid Emerald app icon")
            .into_rgba8();
        let (icon_width, icon_height) = icon.dimensions();
        let icon = egui::IconData {
            rgba: icon.into_raw(),
            width: icon_width,
            height: icon_height,
        };
        viewport.with_icon(std::sync::Arc::new(icon))
    };
    let options = eframe::NativeOptions {
        viewport,
        // OpenGL flickers on some Windows drivers; DirectX via wgpu does not.
        #[cfg(windows)]
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Emerald",
        options,
        Box::new(move |cc| {
            fonts(&cc.egui_ctx);
            let mut app = Notebook::new(path)?;
            app.show_log = verbose;
            let flag = app.vault.root.join(".emerald-tour");
            if first_run {
                // Offer to import a folder or download a server vault before the tour.
                app.welcome = true;
                let _ = fs::write(flag, "");
            } else if !flag.exists() {
                app.tour = Some(0);
                let _ = fs::write(flag, "");
            }
            Ok(Box::new(app))
        }),
    )
}

// ---------------------------------------------------------------- theme

struct Pal {
    bg: Color32,
    side: Color32,
    ribbon: Color32,
    line: Color32,
    text: Color32,
    muted: Color32,
    hover: Color32,
    sel: Color32,
    accent: Color32,
    link: Color32,
    red: Color32,
    amber: Color32,
}
const DARK: Pal = Pal {
    bg: Color32::from_rgb(0x1e, 0x1e, 0x1e),
    side: Color32::from_rgb(0x26, 0x26, 0x26),
    ribbon: Color32::from_rgb(0x16, 0x16, 0x16),
    line: Color32::from_rgb(0x36, 0x36, 0x36),
    text: Color32::from_rgb(0xda, 0xda, 0xda),
    muted: Color32::from_rgb(0x99, 0x99, 0x99),
    hover: Color32::from_rgb(0x2f, 0x2f, 0x2f),
    sel: Color32::from_rgb(0x3a, 0x3a, 0x3a),
    accent: Color32::from_rgb(0x2f, 0xb5, 0x83),
    link: Color32::from_rgb(0x46, 0xd1, 0x9c),
    red: Color32::from_rgb(0xe5, 0x53, 0x4b),
    amber: Color32::from_rgb(0xf0, 0xa7, 0x3a),
};
const LIGHT: Pal = Pal {
    bg: Color32::from_rgb(0xff, 0xff, 0xff),
    side: Color32::from_rgb(0xf5, 0xf6, 0xf5),
    ribbon: Color32::from_rgb(0xeb, 0xed, 0xec),
    line: Color32::from_rgb(0xdc, 0xdf, 0xdd),
    text: Color32::from_rgb(0x1f, 0x24, 0x21),
    muted: Color32::from_rgb(0x6b, 0x71, 0x6d),
    hover: Color32::from_rgb(0xe9, 0xec, 0xea),
    sel: Color32::from_rgb(0xdd, 0xe3, 0xdf),
    accent: Color32::from_rgb(0x1f, 0x9a, 0x6c),
    link: Color32::from_rgb(0x15, 0x8a, 0x5e),
    red: Color32::from_rgb(0xc9, 0x3a, 0x31),
    amber: Color32::from_rgb(0xb8, 0x6e, 0x00),
};
static IS_LIGHT: AtomicBool = AtomicBool::new(false);
fn pal() -> &'static Pal {
    if IS_LIGHT.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}
fn fonts(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../assets/Inter-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "inter-bold".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../assets/Inter-SemiBold.ttf"))),
    );
    // Inter first; keep egui's bundled fonts as fallbacks for symbols such as ⟳.
    let fallback = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.insert(egui::FontFamily::Proportional, [vec!["inter".to_owned()], fallback.clone()].concat());
    fonts.families.insert(egui::FontFamily::Name("bold".into()), [vec!["inter-bold".to_owned()], fallback].concat());
    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().insert(0, "inter".into());
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(6., 3.);
        s.spacing.button_padding = Vec2::new(10., 5.);
        s.spacing.scroll = egui::style::ScrollStyle::solid();
        for (_, f) in s.text_styles.iter_mut() {
            f.size = (f.size * 0.95).max(12.);
        }
    });
}
fn apply_theme(ctx: &egui::Context, light: bool) {
    IS_LIGHT.store(light, Ordering::Relaxed);
    let p = pal();
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = p.bg;
    v.window_fill = p.side;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.side;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x27, 0xa7, 0x7a, 0x55);
    // Colour of selected text (tabs, toggles, tree rows): readable on the green fill.
    v.selection.stroke = egui::Stroke::new(1.0_f32, if light { p.text } else { Color32::WHITE });
    v.text_cursor.stroke = egui::Stroke::new(2.0_f32, p.accent);
    v.hyperlink_color = p.link;
    v.window_stroke = egui::Stroke::new(1.0_f32, p.line);
    v.window_corner_radius = egui::CornerRadius::same(10);
    let r = egui::CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, p.line);
    w.inactive.weak_bg_fill = p.bg;
    w.inactive.bg_fill = p.bg;
    w.inactive.bg_stroke = egui::Stroke::new(1.0_f32, p.line);
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_fill = p.hover;
    w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, p.line);
    w.active.weak_bg_fill = p.sel;
    w.active.bg_fill = p.sel;
    w.active.bg_stroke = egui::Stroke::new(1.0_f32, p.line);
    // Text colour via the widget strokes rather than override_text_color, which would
    // also repaint hyperlinks in plain text colour.
    for x in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        x.corner_radius = r;
        x.fg_stroke.color = p.text;
    }
    ctx.set_visuals(v);
}
macro_rules! ico {
    ($n:literal) => {
        egui::include_image!(concat!("../../assets/icons/", $n, ".svg"))
    };
}
fn svg(name: &str) -> egui::ImageSource<'static> {
    match name {
        "side" => ico!("side"),
        "files" => ico!("files"),
        "search" => ico!("search"),
        "graph" => ico!("graph"),
        "today" => ico!("today"),
        "pal" => ico!("pal"),
        "sync" => ico!("sync"),
        "set" => ico!("set"),
        "new" => ico!("new"),
        "folder" => ico!("folder"),
        "sort" => ico!("sort"),
        "collapse" => ico!("collapse"),
        "close" => ico!("close"),
        "plus" => ico!("plus"),
        "chev" => ico!("chev"),
        "back" => ico!("back"),
        "fwd" => ico!("fwd"),
        _ => ico!("panel"),
    }
}
/// Obsidian-style icon button: 30x28, muted icon, hover/selected background.
fn ib(ui: &mut egui::Ui, name: &str, tip: &str, on: bool) -> egui::Response {
    let p = pal();
    let (rect, r) = ui.allocate_exact_size(Vec2::new(30., 28.), egui::Sense::click());
    if on || r.hovered() {
        ui.painter().rect_filled(rect, 5., if on { p.sel } else { p.hover });
    }
    let c = if on || r.hovered() { p.text } else { p.muted };
    egui::Image::new(svg(name))
        .tint(c)
        .fit_to_exact_size(Vec2::splat(18.))
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(18.)));
    r.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal();
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong()).fill(p.accent))
}
fn muted(t: impl Into<String>) -> egui::RichText {
    egui::RichText::new(t.into()).color(pal().muted)
}

// ---------------------------------------------------------------- pure helpers

fn banner_flag(root: &Path) -> PathBuf {
    root.join(".emerald-nobanner")
}
/// Open `[[` link at the cursor: (char index where the name starts, cursor, typed name).
fn pick_at(text: &str, cursor: usize) -> Option<(usize, usize, String)> {
    let before: String = text.chars().take(cursor).collect();
    let at = before.rfind("[[")?;
    let q = &before[at + 2..];
    if q.contains(['\n', ']']) {
        return None;
    }
    Some((before[..at].chars().count() + 2, cursor, q.to_owned()))
}
fn links(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split("[[")
        .skip(1)
        .filter_map(|s| s.split_once("]]"))
        .map(|(s, _)| s.split('|').next().unwrap_or(s))
        .filter(|s| !s.contains('\n') && !s.trim().is_empty())
        .map(note_name)
}
/// Links in first-seen order without repeats.
fn unique_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in links(text) {
        if !out.contains(&l) {
            out.push(l)
        }
    }
    out
}
fn note_name(name: &str) -> String {
    let name = name.trim();
    if name.ends_with(".md") {
        name.to_owned()
    } else {
        format!("{name}.md")
    }
}
fn stem(n: &str) -> &str {
    n.trim_end_matches(".md")
}
/// Last path segment without `.md`.
fn leaf(n: &str) -> &str {
    stem(n).rsplit('/').next().unwrap_or(n)
}
/// Folder part including the trailing slash, or "".
fn folder_of(n: &str) -> &str {
    match n.rfind('/') {
        Some(i) => &n[..=i],
        None => "",
    }
}
/// Turn `[[Note]]` and `[[Note|label]]` into Markdown links the preview can follow.
fn wiki_to_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("[[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("]]") {
            Some(j) if !after[..j].contains('\n') && !after[..j].trim().is_empty() => {
                let inner = &after[..j];
                let (target, label) = inner.split_once('|').unwrap_or((inner, leaf(inner.trim())));
                out.push_str(&format!("[{}](<emerald:{}>)", label.trim(), stem(target.trim())));
                rest = &after[j + 2..];
            }
            _ => {
                out.push_str("[[");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
/// `Notes/Idea-conflict-0123456789abcdef.md` -> `Notes/Idea.md`.
fn conflict_original(n: &str) -> Option<String> {
    let (base, ext) = n.rsplit_once('.')?;
    let (orig, hash) = base.rsplit_once("-conflict-")?;
    (hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("{orig}.{ext}"))
}
/// `.trash/20261128-110501-Work__Plan.md` -> (`Work/Plan.md`, `2026-11-28 11:05`).
fn trash_entry(file: &str) -> Option<(String, String)> {
    let (date, rest) = file.split_once('-')?;
    let (time, flat) = rest.split_once('-')?;
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    let when = format!("{}-{}-{} {}:{}", &date[..4], &date[4..6], &date[6..], &time[..2], &time[2..4]);
    Some((flat.replace("__", "/"), when))
}
/// Subsequence match score (higher is better), or None.
fn fuzzy(hay: &str, q: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let hay = hay.to_lowercase();
    let q = q.to_lowercase();
    if let Some(i) = hay.find(&q) {
        return Some(1000 - i as i32 - hay.len() as i32);
    }
    let mut it = hay.chars();
    let mut gaps = 0;
    for c in q.chars() {
        let mut found = false;
        for h in it.by_ref() {
            if h == c {
                found = true;
                break;
            }
            gaps += 1;
        }
        if !found {
            return None;
        }
    }
    Some(500 - gaps)
}
fn ago(t: Instant) -> String {
    let s = t.elapsed().as_secs();
    match s {
        0..=9 => "just now".into(),
        10..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}
fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}
fn reveal(path: &Path) {
    let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_opens_after_double_bracket() {
        assert_eq!(pick_at("hi [[", 5), Some((5, 5, String::new())));
        assert_eq!(pick_at("hi [[Wel", 8), Some((5, 8, "Wel".into())));
        assert_eq!(pick_at("é [[Wé", 6), Some((4, 6, "Wé".into())));
    }
    #[test]
    fn picker_closes_when_link_ends() {
        assert_eq!(pick_at("[[a]] b", 7), None);
        assert_eq!(pick_at("[[a\nb", 5), None);
        assert_eq!(pick_at("plain", 5), None);
    }
    #[test]
    fn links_are_deduplicated_and_aliases_dropped() {
        assert_eq!(unique_links("[[A]] [[B|bee]] [[A]] [[ ]]"), vec!["A.md", "B.md"]);
    }
    #[test]
    fn wiki_links_become_markdown() {
        assert_eq!(wiki_to_md("see [[Work/Plan]]."), "see [Plan](<emerald:Work/Plan>).");
        assert_eq!(wiki_to_md("[[A|the a]]"), "[the a](<emerald:A>)");
        assert_eq!(wiki_to_md("open [[ only"), "open [[ only");
    }
    #[test]
    fn names_split_into_parts() {
        assert_eq!(leaf("Work/Meetings/Kickoff.md"), "Kickoff");
        assert_eq!(folder_of("Work/Meetings/Kickoff.md"), "Work/Meetings/");
        assert_eq!(folder_of("Top.md"), "");
    }
    #[test]
    fn conflict_copies_map_to_originals() {
        assert_eq!(conflict_original("A/Idea-conflict-0123456789abcdef.md").as_deref(), Some("A/Idea.md"));
        assert_eq!(conflict_original("Idea-conflict-xyz.md"), None);
        assert_eq!(conflict_original("Idea.md"), None);
    }
    #[test]
    fn trash_names_decode() {
        assert_eq!(
            trash_entry("20261128-110501-Work__Plan.md"),
            Some(("Work/Plan.md".into(), "2026-11-28 11:05".into()))
        );
        assert_eq!(trash_entry("junk.md"), None);
    }
    #[test]
    fn import_scan_lists_files_that_cannot_sync() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("Projects")).unwrap();
        fs::create_dir_all(d.path().join(".obsidian")).unwrap();
        fs::write(d.path().join("Projects/Plan.md"), "# Plan").unwrap();
        fs::write(d.path().join("photo.png"), [0u8, 159, 146, 150]).unwrap();
        fs::write(d.path().join(".obsidian/app.json"), "{}").unwrap();
        fs::write(d.path().join("Latin1.md"), [0x63u8, 0x61, 0x66, 0xe9]).unwrap();
        let s = scan_folder(d.path()).unwrap();
        assert_eq!((s.notes, s.files), (1, 2));
        assert_eq!(s.problems, vec!["Latin1.md: not UTF-8 text".to_owned()]);
        if cfg!(unix) {
            fs::write(d.path().join("What?.md"), "x").unwrap();
            assert_eq!(scan_folder(d.path()).unwrap().problems.len(), 2);
        }
        assert!(scan_folder(&d.path().join("missing")).is_err());
    }
    #[test]
    fn fuzzy_prefers_contiguous_matches() {
        assert!(fuzzy("Website redesign", "web").unwrap() > fuzzy("Weekly sync", "wey").unwrap_or(-1));
        assert!(fuzzy("Kickoff", "kff").is_some());
        assert!(fuzzy("Kickoff", "zz").is_none());
    }
}

// ---------------------------------------------------------------- persisted settings

#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy, PartialEq)]
enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Prefs {
    theme: ThemePref,
    recent: Vec<PathBuf>,
}
#[derive(serde::Serialize, serde::Deserialize, Default, PartialEq, Clone)]
struct Session {
    tabs: Vec<String>,
    active: String,
    sidebar: bool,
    sort_desc: bool,
    hide_links: bool,
}
fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Emerald"))
}
fn load_json<T: serde::de::DeserializeOwned + Default>(p: Option<PathBuf>) -> T {
    p.and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
fn save_json<T: serde::Serialize>(p: Option<PathBuf>, v: &T) {
    if let Some(p) = p {
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        if let Ok(b) = serde_json::to_vec_pretty(v) {
            let _ = atomic(&p, &b);
        }
    }
}
fn prefs_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("app.json"))
}
fn session_file(root: &Path) -> Option<PathBuf> {
    config_dir().map(|d| d.join(format!("session-{}.json", digest(root.to_string_lossy().as_bytes()))))
}

// ---------------------------------------------------------------- state

#[derive(Default, PartialEq, Clone, Copy)]
enum View {
    #[default]
    Edit,
    Preview,
    Graph,
}
#[derive(Default, PartialEq, Clone, Copy)]
enum Section {
    #[default]
    Sync,
    Appearance,
    Vaults,
    Shortcuts,
}
#[derive(PartialEq, Clone, Copy)]
enum SetupKind {
    /// Create a new, empty local vault.
    New,
    /// Use an existing folder on this computer and upload it to the server.
    Import,
    /// Download a vault that already exists on the server.
    Server,
}
/// What a pre-import scan of a folder found.
struct Scan {
    files: usize,
    notes: usize,
    bytes: u64,
    problems: Vec<String>,
}
/// The "New vault", "Import folder" and "Open from server" window.
struct VaultSetup {
    kind: SetupKind,
    folder: String,
    name: String,
    name_edited: bool,
    folder_edited: bool,
    sync: bool,
    /// Reuse the server address and token of the vault that is open now.
    same_server: bool,
    server: String,
    token: String,
    remote: Option<Vec<String>>,
    list_rx: Option<mpsc::Receiver<Result<Vec<String>>>>,
    scan: Option<(String, Scan)>,
    msg: Option<String>,
    job: Option<mpsc::Receiver<Result<()>>>,
    /// The connection the running job is preparing.
    pending: Option<(PathBuf, sync::Connection)>,
    /// Opened from the first-launch welcome window.
    from_welcome: bool,
}
/// A connection test's message, and the server's named vaults when it lists them.
type TestResult = (String, Option<Vec<String>>);
#[derive(PartialEq)]
enum SyncState {
    Off,
    Idle,
    Syncing,
    Error(String),
}

#[derive(Default)]
struct Document {
    name: String,
    text: String,
    saved: String,
    dirty: bool,
}

/// Things the file tree's menus ask the app to do.
enum Act {
    Open(String),
    Rename(String),
    Move(String),
    Duplicate(String),
    Delete(String),
    NewIn(String),
    RenameFolder(String),
    Reveal(String),
}

#[derive(Default)]
struct Tree {
    dirs: BTreeMap<String, Tree>,
    files: Vec<(String, String)>,
}
impl Tree {
    fn add(&mut self, full: &str, rest: &str) {
        match rest.split_once('/') {
            Some((d, r)) => self.dirs.entry(d.to_owned()).or_default().add(full, r),
            None => self.files.push((full.to_owned(), stem(rest).to_owned())),
        }
    }
    fn show(&self, ui: &mut egui::Ui, path: &str, cur: &str, act: &mut Option<Act>, desc: bool, collapse: bool) {
        let mut dirs: Vec<_> = self.dirs.iter().collect();
        let mut files: Vec<_> = self.files.iter().collect();
        if desc {
            dirs.reverse();
            files.reverse();
        }
        for (d, t) in dirs {
            let full = format!("{path}{d}/");
            let r = egui::CollapsingHeader::new(d)
                .id_salt(&full)
                .default_open(true)
                .open(collapse.then_some(false))
                .show(ui, |ui| t.show(ui, &full, cur, act, desc, collapse));
            r.header_response.context_menu(|ui| {
                if ui.button("New note here").clicked() {
                    *act = Some(Act::NewIn(full.clone()));
                    ui.close_menu();
                }
                if ui.button("Rename folder…").clicked() {
                    *act = Some(Act::RenameFolder(full.clone()));
                    ui.close_menu();
                }
                if ui.button("Reveal in file manager").clicked() {
                    *act = Some(Act::Reveal(full.clone()));
                    ui.close_menu();
                }
            });
        }
        for (full, name) in files {
            let r = ui.selectable_label(full == cur, name);
            if r.clicked() {
                *act = Some(Act::Open(full.clone()))
            }
            r.context_menu(|ui| {
                let mut pick = |ui: &mut egui::Ui, label: &str, a: Act| {
                    if ui.button(label).clicked() {
                        *act = Some(a);
                        ui.close_menu();
                    }
                };
                pick(ui, "Open", Act::Open(full.clone()));
                pick(ui, "Rename…", Act::Rename(full.clone()));
                pick(ui, "Move to folder…", Act::Move(full.clone()));
                pick(ui, "Duplicate", Act::Duplicate(full.clone()));
                if ui.button("Copy link").clicked() {
                    ui.ctx().copy_text(format!("[[{}]]", stem(full)));
                    ui.close_menu();
                }
                pick(ui, "Reveal in file manager", Act::Reveal(full.clone()));
                ui.separator();
                if ui.button(egui::RichText::new("Delete…").color(pal().red)).clicked() {
                    *act = Some(Act::Delete(full.clone()));
                    ui.close_menu();
                }
            });
        }
    }
}

struct Notebook {
    vault: Vault,
    notes: BTreeMap<String, String>,
    doc: Document,
    tabs: Vec<String>,
    view: View,
    // sidebar
    sidebar: bool,
    sort_desc: bool,
    collapse: bool,
    query: String,
    focus_search: bool,
    new_folder: Option<String>,
    folder_err: Option<String>,
    // prompts
    move_win: Option<(String, String)>,
    folder_rename: Option<(String, String)>,
    confirm_delete: Option<String>,
    // editor
    title: String,
    focus_title: bool,
    select_title: bool,
    focus_editor: bool,
    last_edit: Option<Instant>,
    pick: Option<(usize, usize, String)>,
    pick_sel: usize,
    hide_links: bool,
    // status
    status: String,
    status_err: bool,
    status_at: Instant,
    undo: Option<(String, PathBuf, Instant)>,
    log: Vec<String>,
    show_log: bool,
    // sync
    sync_state: SyncState,
    last_sync: Option<Instant>,
    banner_off: bool,
    pending: Option<mpsc::Receiver<Result<sync::Summary>>>,
    last_auto_sync: Instant,
    close_after_sync: bool,
    // windows
    palette: bool,
    pal_query: String,
    pal_sel: usize,
    vault_win: Option<String>,
    setup: Option<VaultSetup>,
    welcome: bool,
    tour: Option<usize>,
    tour_rects: [Option<Rect>; 4],
    settings: Option<Section>,
    edit_server: bool,
    server: String,
    server_vault: String,
    server_vaults: Option<Vec<String>>,
    secret: String,
    show_secret: bool,
    test_rx: Option<mpsc::Receiver<Result<TestResult>>>,
    test_msg: Option<(bool, String)>,
    trash: bool,
    trash_confirm: Option<String>,
    compare: Option<String>,
    // graph
    gpos: BTreeMap<String, Vec2>,
    gzoom: f32,
    gpan: Vec2,
    gfilter: String,
    glocal: bool,
    glabels: bool,
    // misc
    prefs: Prefs,
    light: Option<bool>,
    saved_session: Session,
    cache: CommonMarkCache,
}

impl Notebook {
    fn new(path: PathBuf) -> Result<Self> {
        let mut prefs: Prefs = load_json(prefs_file());
        let canon = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        prefs.recent.retain(|p| p != &canon);
        prefs.recent.insert(0, canon);
        save_json(prefs_file(), &prefs);
        let session: Session = load_json(session_file(&path));
        let mut app = Self {
            vault: Vault::new(path, String::new())?,
            notes: Default::default(),
            doc: Default::default(),
            tabs: Vec::new(),
            view: View::Edit,
            sidebar: true,
            sort_desc: false,
            collapse: false,
            query: String::new(),
            focus_search: false,
            new_folder: None,
            folder_err: None,
            move_win: None,
            folder_rename: None,
            confirm_delete: None,
            title: String::new(),
            focus_title: false,
            select_title: false,
            focus_editor: false,
            last_edit: None,
            pick: None,
            pick_sel: 0,
            hide_links: false,
            status: String::new(),
            status_err: false,
            status_at: Instant::now(),
            undo: None,
            log: Vec::new(),
            show_log: false,
            sync_state: SyncState::Off,
            last_sync: None,
            banner_off: false,
            pending: None,
            last_auto_sync: Instant::now(),
            close_after_sync: false,
            palette: false,
            pal_query: String::new(),
            pal_sel: 0,
            vault_win: None,
            setup: None,
            welcome: false,
            tour: None,
            tour_rects: [None; 4],
            settings: None,
            edit_server: false,
            server: String::new(),
            server_vault: String::new(),
            server_vaults: None,
            secret: String::new(),
            show_secret: false,
            test_rx: None,
            test_msg: None,
            trash: false,
            trash_confirm: None,
            compare: None,
            gpos: BTreeMap::new(),
            gzoom: 1.,
            gpan: Vec2::ZERO,
            gfilter: String::new(),
            glocal: false,
            glabels: true,
            prefs,
            light: None,
            saved_session: Session::default(),
            cache: Default::default(),
        };
        app.refresh()?;
        if sync::configured(&app.vault) {
            app.sync_state = SyncState::Idle;
        }
        app.banner_off = banner_flag(&app.vault.root).exists();
        // Restore the previous session for this vault.
        if session != Session::default() {
            app.sidebar = session.sidebar;
            app.sort_desc = session.sort_desc;
            app.hide_links = session.hide_links;
            app.tabs = session.tabs.iter().filter(|t| app.notes.contains_key(*t)).cloned().collect();
            let active = if app.tabs.contains(&session.active) { Some(session.active.clone()) } else { app.tabs.last().cloned() };
            if let Some(a) = active {
                app.load(a)?;
                app.focus_editor = false;
            }
        }
        app.saved_session = app.session();
        Ok(app)
    }
    fn session(&self) -> Session {
        Session {
            tabs: self.tabs.clone(),
            active: self.doc.name.clone(),
            sidebar: self.sidebar,
            sort_desc: self.sort_desc,
            hide_links: self.hide_links,
        }
    }
    fn say(&mut self, m: impl Into<String>) {
        self.status = m.into();
        self.status_err = false;
        self.status_at = Instant::now();
    }
    fn fail(&mut self, e: impl std::fmt::Display) {
        let m = e.to_string();
        self.log_line(format!("error: {m}"));
        self.status = m;
        self.status_err = true;
        self.status_at = Instant::now();
    }
    fn busy(&self) -> bool {
        self.pending.is_some()
    }
    fn refresh(&mut self) -> Result<()> {
        let mut notes = BTreeMap::new();
        for n in index(&self.vault.root)?.into_keys().filter(|n| n.ends_with(".md")) {
            notes.insert(n.clone(), fs::read_to_string(target(&self.vault.root, &n)?)?);
        }
        self.notes = notes;
        Ok(())
    }
    fn load(&mut self, n: String) -> Result<()> {
        let path = target(&self.vault.root, &n)?;
        let (text, saved) = match fs::read_to_string(path) {
            Ok(t) => {
                let h = digest(t.as_bytes());
                (t, h)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), String::new()),
            Err(e) => return Err(e.into()),
        };
        if !self.tabs.contains(&n) {
            self.tabs.push(n.clone());
        }
        self.doc = Document { name: n, text, saved, dirty: false };
        self.title = leaf(&self.doc.name).to_owned();
        self.focus_editor = true;
        self.last_edit = None;
        if self.view == View::Graph {
            self.view = View::Edit;
        }
        Ok(())
    }
    fn fresh_in(&self, folder: &str) -> String {
        let mut i = 0;
        loop {
            let n = if i == 0 { format!("{folder}Untitled.md") } else { format!("{folder}Untitled {i}.md") };
            if !self.notes.contains_key(&n) && !self.tabs.contains(&n) {
                return n;
            }
            i += 1;
        }
    }
    /// Open a new untitled note with its title selected, ready to be named.
    fn new_note(&mut self, folder: &str) -> Result<()> {
        let n = self.fresh_in(folder);
        self.open(n)?;
        self.focus_title = true;
        self.select_title = true;
        self.focus_editor = false;
        Ok(())
    }
    fn rename_to(&mut self, to: String) -> Result<()> {
        ensure!(!self.busy(), "Wait for sync to finish, then rename");
        let to = note_name(&to);
        if to == self.doc.name {
            return Ok(());
        }
        ensure!(!self.notes.contains_key(&to), "A note called \"{}\" already exists", stem(&to));
        self.save()?;
        let from = self.checked_path()?;
        let dest = target(&self.vault.root, &to)?;
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?
        }
        if from.exists() {
            fs::rename(from, dest)?;
        }
        let old = std::mem::replace(&mut self.doc.name, to.clone());
        if let Some(t) = self.notes.remove(&old) {
            self.notes.insert(to.clone(), t);
        }
        for t in &mut self.tabs {
            if *t == old {
                *t = to.clone()
            }
        }
        self.title = leaf(&to).to_owned();
        self.say(format!("Renamed to {}", stem(&to)));
        Ok(())
    }
    fn open(&mut self, n: String) -> Result<()> {
        self.save()?;
        self.load(n)
    }
    fn checked_path(&self) -> Result<PathBuf> {
        let path = target(&self.vault.root, &self.doc.name)?;
        let hash = match fs::read(&path) {
            Ok(b) => digest(&b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        ensure!(hash == self.doc.saved, "File changed on disk. Copy your edits before reloading.");
        Ok(path)
    }
    fn save(&mut self) -> Result<()> {
        if self.doc.name.is_empty() || (!self.doc.dirty && !self.doc.saved.is_empty()) || (self.doc.text.is_empty() && self.doc.saved.is_empty()) {
            return Ok(());
        }
        atomic(&self.checked_path()?, self.doc.text.as_bytes())?;
        self.notes.insert(self.doc.name.clone(), self.doc.text.clone());
        self.doc.saved = digest(self.doc.text.as_bytes());
        self.doc.dirty = false;
        self.say("Saved");
        Ok(())
    }
    fn close_tab(&mut self, tab: &str) -> Result<()> {
        if tab == self.doc.name {
            self.save()?;
        }
        self.tabs.retain(|t| t != tab);
        if tab == self.doc.name {
            match self.tabs.last().cloned() {
                Some(n) => self.load(n)?,
                None => {
                    self.doc = Document::default();
                    self.title.clear();
                }
            }
        }
        Ok(())
    }
    fn backlinks_of(&self, name: &str) -> Vec<String> {
        self.notes
            .iter()
            .filter(|(n, t)| n.as_str() != name && links(t).any(|p| p == name))
            .map(|(n, _)| n.clone())
            .collect()
    }
    fn delete_note(&mut self, name: &str) -> Result<()> {
        ensure!(!self.busy(), "Wait for sync to finish, then delete");
        if self.doc.name != name {
            self.open(name.to_owned())?;
        }
        let path = self.checked_path()?;
        if path.exists() {
            let dir = self.vault.root.join(".trash");
            fs::create_dir_all(&dir)?;
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let dest = dir.join(format!("{stamp}-{}", name.replace('/', "__")));
            fs::rename(&path, &dest)?;
            self.undo = Some((name.to_owned(), dest, Instant::now()));
        }
        self.notes.remove(name);
        self.tabs.retain(|t| t != name);
        self.doc = Document::default();
        self.title.clear();
        if let Some(n) = self.tabs.last().cloned() {
            self.load(n)?;
        }
        self.say(format!("Moved {} to Trash", leaf(name)));
        Ok(())
    }
    fn restore(&mut self, name: &str, from: &Path) -> Result<()> {
        let to = target(&self.vault.root, name)?;
        ensure!(!to.exists(), "A note called \"{}\" already exists", stem(name));
        if let Some(p) = to.parent() {
            fs::create_dir_all(p)?
        }
        fs::rename(from, to)?;
        self.refresh()?;
        self.open(name.to_owned())?;
        self.say(format!("Restored {}", leaf(name)));
        Ok(())
    }
    fn rename_folder(&mut self, from: &str, to: &str) -> Result<()> {
        ensure!(!self.busy(), "Wait for sync to finish, then rename");
        let to = format!("{}/", to.trim().trim_matches('/'));
        ensure!(to != "/", "Enter a folder name");
        if to == from {
            return Ok(());
        }
        self.save()?;
        let src = target(&self.vault.root, from.trim_end_matches('/'))?;
        let dst = target(&self.vault.root, to.trim_end_matches('/'))?;
        ensure!(!dst.exists(), "A folder called \"{}\" already exists", to.trim_end_matches('/'));
        if let Some(p) = dst.parent() {
            fs::create_dir_all(p)?
        }
        fs::rename(src, dst)?;
        let remap = |n: &str| match n.strip_prefix(from) {
            Some(rest) => format!("{to}{rest}"),
            None => n.to_owned(),
        };
        self.tabs = self.tabs.iter().map(|t| remap(t)).collect();
        if !self.doc.name.is_empty() {
            self.doc.name = remap(&self.doc.name);
        }
        self.refresh()?;
        self.say(format!("Renamed folder to {}", to.trim_end_matches('/')));
        Ok(())
    }
    fn duplicate(&mut self, name: &str) -> Result<()> {
        self.save()?;
        let text = self.notes.get(name).cloned().unwrap_or_default();
        let base = stem(name);
        let mut i = 1;
        let copy = loop {
            let c = format!("{base} copy{}.md", if i == 1 { String::new() } else { format!(" {i}") });
            if !self.notes.contains_key(&c) {
                break c;
            }
            i += 1;
        };
        atomic(&target(&self.vault.root, &copy)?, text.as_bytes())?;
        self.notes.insert(copy.clone(), text);
        self.open(copy)
    }
    fn conflicts(&self) -> Vec<String> {
        self.notes.keys().filter(|n| conflict_original(n).is_some()).cloned().collect()
    }
    fn log_line(&mut self, m: String) {
        let t = chrono::Local::now().format("%H:%M:%S");
        self.log.push(format!("{t}  {m}"));
        if self.log.len() > 500 {
            self.log.remove(0);
        }
    }
    fn sync(&mut self, ctx: &egui::Context) -> Result<()> {
        if self.busy() {
            return Ok(());
        }
        self.save()?;
        let vault = self.vault.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.sync_state = SyncState::Syncing;
        std::thread::spawn(move || {
            let _ = tx.send(sync::run(vault));
            ctx.request_repaint();
        });
        Ok(())
    }
    fn poll(&mut self, ctx: &egui::Context) -> Result<()> {
        let Some(rx) = &self.pending else {
            return Ok(());
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(e) => Err(e.into()),
        };
        self.pending = None;
        if self.close_after_sync {
            let _ = self.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let active_name = self.doc.name.clone();
        let active_hash = self.doc.saved.clone();
        self.refresh()?;
        if !active_name.is_empty() {
            match self.notes.get(&active_name) {
                Some(text) if digest(text.as_bytes()) != active_hash => {
                    // Update the open note only when sync actually changed it, without
                    // stealing focus or switching views.
                    if !self.doc.dirty {
                        self.doc.text = text.clone();
                        self.doc.saved = digest(text.as_bytes());
                    }
                }
                None if !self.doc.saved.is_empty() => {
                    self.tabs.retain(|t| t != &active_name);
                    self.doc = Document::default();
                    self.title.clear();
                    self.pick = None;
                }
                _ => {}
            }
        }
        match result {
            Ok(s) => {
                if s.sent + s.received > 0 || s.conflicts > 0 {
                    self.log_line(format!("sync: {s}"));
                }
                self.sync_state = SyncState::Idle;
                self.last_sync = Some(Instant::now());
            }
            Err(e) => {
                let m = format!("{e:#}");
                self.log_line(format!("sync error: {m}"));
                self.sync_state = SyncState::Error(m);
            }
        }
        Ok(())
    }
    // ------------------------------------------------------------ graph
    fn graph(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let p = pal();
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let r = response.rect;
        let all_edges: Vec<(String, String)> = self
            .notes
            .iter()
            .flat_map(|(a, t)| links(t).filter(|l| self.notes.contains_key(l) && l != a).map(|l| (a.clone(), l)).collect::<Vec<_>>())
            .collect();
        let f = self.gfilter.to_lowercase();
        let cur = self.doc.name.clone();
        let names: Vec<String> = self
            .notes
            .keys()
            .filter(|k| f.is_empty() || k.to_lowercase().contains(&f))
            .filter(|k| !self.glocal || cur.is_empty() || **k == cur || all_edges.iter().any(|(a, b)| (a == *k && *b == cur) || (b == *k && *a == cur)))
            .cloned()
            .collect();
        let edges: Vec<(String, String)> = all_edges.into_iter().filter(|(a, b)| names.contains(a) && names.contains(b)).collect();
        let n = names.len();
        if n == 0 {
            painter.text(r.center(), egui::Align2::CENTER_CENTER, "No notes match", egui::FontId::proportional(14.), p.muted);
            return None;
        }
        self.gpos.retain(|k, _| self.notes.contains_key(k));
        for (i, k) in names.iter().enumerate() {
            self.gpos.entry(k.clone()).or_insert_with(|| Vec2::angled(i as f32 * std::f32::consts::TAU / n.max(1) as f32) * 160.);
        }
        // force layout: repulsion, springs, gentle pull to centre
        let mut force: BTreeMap<String, Vec2> = names.iter().map(|k| (k.clone(), Vec2::ZERO)).collect();
        for i in 0..n {
            for j in i + 1..n {
                let d = self.gpos[&names[i]] - self.gpos[&names[j]];
                let l = d.length().max(1.);
                let f = d / l * (9000. / (l * l)).min(20.);
                *force.get_mut(&names[i]).unwrap() += f;
                *force.get_mut(&names[j]).unwrap() -= f;
            }
        }
        for (a, b) in &edges {
            let d = self.gpos[b] - self.gpos[a];
            let l = d.length().max(1.);
            let f = d / l * (l - 110.) * 0.02;
            *force.get_mut(a).unwrap() += f;
            *force.get_mut(b).unwrap() -= f;
        }
        let dragged_id = egui::Id::new("graph-drag");
        let mut dragging: Option<String> = ui.data(|d| d.get_temp(dragged_id));
        let to_screen = |v: Vec2, z: f32, pan: Vec2| r.center() + (v * z) + pan;
        let hit = response
            .hover_pos()
            .and_then(|h| names.iter().find(|k| to_screen(self.gpos[*k], self.gzoom, self.gpan).distance(h) < 14.).cloned());
        if response.drag_started() {
            dragging = hit.clone();
        }
        if response.drag_stopped() {
            dragging = None;
        }
        if response.dragged() {
            match &dragging {
                Some(k) => {
                    let z = self.gzoom;
                    *self.gpos.get_mut(k).unwrap() += response.drag_delta() / z;
                }
                None => self.gpan += response.drag_delta(),
            }
        }
        ui.data_mut(|d| match &dragging {
            Some(k) => d.insert_temp(dragged_id, k.clone()),
            None => d.remove::<String>(dragged_id),
        });
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                self.gzoom = (self.gzoom * (1. + scroll * 0.002)).clamp(0.3, 3.);
            }
        }
        let mut moving = 0f32;
        for (k, f) in force {
            if dragging.as_ref() != Some(&k) {
                let cur = self.gpos[&k];
                let step = (f - cur * 0.01) * 0.5;
                moving = moving.max(step.length());
                *self.gpos.get_mut(&k).unwrap() += step;
            }
        }
        // Repaint only while the layout is still settling or being dragged.
        if moving > 0.05 || dragging.is_some() {
            ui.ctx().request_repaint();
        }
        let (z, pan) = (self.gzoom, self.gpan);
        let near: Vec<&String> = match &hit {
            Some(h) => edges.iter().filter_map(|(a, b)| if a == h { Some(b) } else if b == h { Some(a) } else { None }).collect(),
            None => Vec::new(),
        };
        let dimc = if IS_LIGHT.load(Ordering::Relaxed) { Color32::from_gray(0xc4) } else { Color32::from_gray(0x55) };
        for (a, b) in &edges {
            let on = hit.as_ref().is_some_and(|h| h == a || h == b);
            painter.line_segment([to_screen(self.gpos[a], z, pan), to_screen(self.gpos[b], z, pan)], (if on { 1.8_f32 } else { 1.0 }, if on { p.accent } else { dimc }));
        }
        for k in &names {
            let pos = to_screen(self.gpos[k], z, pan);
            let lit = hit.as_ref() == Some(k) || near.contains(&k);
            let dim = hit.is_some() && !lit;
            painter.circle_filled(pos, if hit.as_ref() == Some(k) { 8. } else { 6. } * z.sqrt(), if dim { dimc } else { p.accent });
            if *k == self.doc.name {
                painter.circle_stroke(pos, 10. * z.sqrt(), (1.5_f32, p.text));
            }
            if self.glabels || lit {
                let mut label = leaf(k).to_owned();
                if label.chars().count() > 24 {
                    label = label.chars().take(23).collect::<String>() + "\u{2026}";
                }
                painter.text(pos + Vec2::new(0., 10. * z.sqrt()), egui::Align2::CENTER_TOP, label, egui::FontId::proportional(13.), if dim { p.muted } else { p.text });
            }
        }
        if let Some(h) = &hit {
            response.clone().on_hover_text(stem(h));
            if response.clicked() {
                return Some(h.clone());
            }
        }
        None
    }
}

// ---------------------------------------------------------------- frame

impl eframe::App for Notebook {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // Theme: follow the system unless the user picked one.
        let light = match self.prefs.theme {
            ThemePref::Light => true,
            ThemePref::Dark => false,
            ThemePref::System => ctx.system_theme() == Some(egui::Theme::Light),
        };
        if self.light != Some(light) {
            apply_theme(ctx, light);
            self.light = Some(light);
        }
        let p = pal();
        if let Err(e) = self.poll(ctx) {
            self.fail(e);
        }
        let busy = self.busy();
        if ctx.input(|i| i.viewport().close_requested()) {
            if busy {
                // Let the running sync finish, then close by itself.
                self.close_after_sync = true;
                self.say("Finishing sync, then closing…");
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose)
            } else if let Err(e) = self.save() {
                self.fail(e);
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose)
            }
        }
        // Autosave one second after typing stops.
        if self.doc.dirty && !busy {
            if let Some(t) = self.last_edit {
                if t.elapsed() >= Duration::from_secs(1) {
                    self.last_edit = None;
                    if let Err(e) = self.save() {
                        self.fail(e);
                    }
                } else {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            }
        }
        // Automatic sync: every 5 s, backing off to 30 s after an error.
        if self.sync_state != SyncState::Off && !busy {
            let every = if matches!(self.sync_state, SyncState::Error(_)) { 30 } else { 5 };
            let every = Duration::from_secs(every);
            if !self.doc.dirty && self.last_auto_sync.elapsed() >= every {
                self.last_auto_sync = Instant::now();
                if let Err(e) = self.sync(ctx) {
                    self.fail(e);
                }
            } else {
                ctx.request_repaint_after(every.saturating_sub(self.last_auto_sync.elapsed()).max(Duration::from_millis(50)));
            }
        }
        // Remember the session whenever it changes.
        let s = self.session();
        if s != self.saved_session {
            save_json(session_file(&self.vault.root), &s);
            self.saved_session = s;
        }
        let mut result: Result<()> = Ok(());
        let mut act: Option<Act> = None;
        let mut want_new: Option<String> = None;

        // ---------------- shortcuts
        let (k_new, k_find, k_graph, k_close, k_pal, k_save) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::N),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::O) || i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::G),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::W),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::P) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::K),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
            )
        });
        let (zin, zout, zreset) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num0),
            )
        });
        if zin || zout || zreset {
            let z = if zreset { 1. } else if zin { ctx.zoom_factor() + 0.1 } else { ctx.zoom_factor() - 0.1 };
            ctx.set_zoom_factor(z.clamp(0.7, 2.0));
        }
        if k_pal {
            self.palette = !self.palette;
            self.pal_query.clear();
            self.pal_sel = 0;
        }
        if k_new {
            want_new = Some(String::new());
        }
        if k_find {
            self.sidebar = true;
            self.focus_search = true
        }
        if k_graph {
            self.view = if self.view == View::Graph { View::Edit } else { View::Graph }
        }
        if k_save {
            result = self.save()
        }
        if k_close && !self.doc.name.is_empty() {
            let tab = self.doc.name.clone();
            result = self.close_tab(&tab);
        }

        // ---------------- ribbon
        let ribbon = egui::SidePanel::left("ribbon")
            .exact_width(44.)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.side).stroke(egui::Stroke::new(1.0_f32, p.line)).inner_margin(egui::Margin::symmetric(6, 8)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.;
                let graph = self.view == View::Graph;
                if ib(ui, "files", "Files (click again to hide)", self.sidebar && self.query.is_empty() && !graph).clicked() {
                    if self.sidebar && self.query.is_empty() && !graph {
                        self.sidebar = false;
                    } else {
                        self.sidebar = true;
                        self.query.clear();
                        if graph {
                            self.view = View::Edit;
                        }
                    }
                }
                if ib(ui, "search", "Search notes (Ctrl+O)", self.sidebar && !self.query.is_empty() && !graph).clicked() {
                    self.sidebar = true;
                    self.focus_search = true;
                    if graph {
                        self.view = View::Edit;
                    }
                }
                if ib(ui, "graph", "Graph view (Ctrl+G)", graph).clicked() {
                    self.view = if graph { View::Edit } else { View::Graph };
                }
                if ib(ui, "today", "Today's daily note", false).clicked() {
                    act = Some(Act::Open(format!("Daily/{}.md", chrono::Local::now().date_naive())));
                }
                if ib(ui, "pal", "Command palette (Ctrl+P / Ctrl+K)", self.palette).clicked() {
                    self.palette = !self.palette;
                    self.pal_query.clear();
                    self.pal_sel = 0;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if ib(ui, "set", "Settings", self.settings.is_some()).clicked() {
                        self.settings = if self.settings.is_some() { None } else { Some(Section::Sync) };
                    }
                    let tip = match &self.sync_state {
                        SyncState::Off => "Set up sync".to_owned(),
                        SyncState::Syncing => "Syncing…".to_owned(),
                        SyncState::Idle => "Sync now".to_owned(),
                        SyncState::Error(e) => format!("Sync error: {e}"),
                    };
                    if ib(ui, "sync", &tip, busy).clicked() {
                        if self.sync_state == SyncState::Off {
                            self.settings = Some(Section::Sync);
                        } else if let Err(e) = self.sync(ctx) {
                            self.fail(e);
                        }
                    }
                });
            });
        self.tour_rects[0] = Some(ribbon.response.rect);

        // ---------------- sidebar
        self.tour_rects[1] = None;
        if self.sidebar {
            let side = egui::SidePanel::left("vault")
                .default_width(250.)
                .width_range(180.0..=480.)
                .frame(egui::Frame::new().fill(p.side).stroke(egui::Stroke::new(1.0_f32, p.line)).inner_margin(egui::Margin::symmetric(8, 8)))
                .show(ctx, |ui| {
                    // Long names are truncated so the panel never resizes itself.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    // Floating scroll bars overlay the list instead of adding width to it.
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
                    // Search, always visible.
                    // Laid out right to left so the clear button takes its space first and
                    // the field fills the rest; sizing the field from a guessed button
                    // width made the panel grow a little every frame.
                    ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.query.is_empty() && ui.add(egui::Button::new(muted("×")).frame(false)).on_hover_text("Clear search").clicked() {
                            self.query.clear();
                        }
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .id(egui::Id::new("search"))
                                .hint_text(muted("Search notes   Ctrl+O"))
                                .margin(egui::Margin::symmetric(8, 5))
                                .desired_width(ui.available_width()),
                        );
                        if std::mem::take(&mut self.focus_search) {
                            r.request_focus()
                        }
                        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.query.clear();
                            self.focus_editor = true;
                        } else if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let q = self.query.to_lowercase();
                            if let Some(n) = self.notes.iter().find(|(n, t)| !q.is_empty() && (n.to_lowercase().contains(&q) || t.to_lowercase().contains(&q))) {
                                act = Some(Act::Open(n.0.clone()))
                            }
                        }
                    }));
                    ui.add_space(6.);
                    ui.horizontal(|ui| {
                        if primary(ui, "+ New note").on_hover_text("Ctrl+N").clicked() {
                            want_new = Some(String::new());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ib(ui, "collapse", "Collapse all folders", false).clicked() {
                                self.collapse = true
                            }
                            if ib(ui, "sort", if self.sort_desc { "Sorted Z–A (click for A–Z)" } else { "Sorted A–Z (click for Z–A)" }, self.sort_desc).clicked() {
                                self.sort_desc = !self.sort_desc
                            }
                            if ib(ui, "folder", "New folder", self.new_folder.is_some()).clicked() {
                                self.new_folder = Some(String::new());
                                self.folder_err = None;
                            }
                        });
                    });
                    if let Some(mut name) = self.new_folder.take() {
                        let mut keep = true;
                        ui.add_space(4.);
                        ui.label(muted("New folder name"));
                        ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let create = ui.button("Create").clicked();
                            let r = ui.add(egui::TextEdit::singleline(&mut name).id(egui::Id::new("new-folder")).hint_text(muted("e.g. Recipes")).desired_width(ui.available_width()));
                            if name.is_empty() && !r.has_focus() && self.folder_err.is_none() {
                                r.request_focus();
                            }
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                keep = false;
                            }
                            if create || enter {
                                let n = name.trim().trim_matches('/');
                                if n.is_empty() {
                                    self.folder_err = Some("Type a folder name first.".into());
                                } else {
                                    // Folders exist through their notes: create one inside and name it.
                                    want_new = Some(format!("{n}/"));
                                    keep = false;
                                }
                            }
                        }));
                        if let Some(e) = &self.folder_err {
                            ui.label(egui::RichText::new(e).color(p.red).small());
                        }
                        if ui.small_button("Cancel").clicked() {
                            keep = false;
                        }
                        if keep {
                            self.new_folder = Some(name);
                        }
                    }
                    ui.add_space(4.);
                    let collapse_now = std::mem::take(&mut self.collapse);
                    let q = self.query.to_lowercase();
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if q.is_empty() {
                            let mut tree = Tree::default();
                            for n in self.notes.keys() {
                                tree.add(n, n);
                            }
                            if !self.doc.name.is_empty() && !self.notes.contains_key(&self.doc.name) {
                                tree.add(&self.doc.name, &self.doc.name);
                            }
                            if self.notes.is_empty() && self.doc.name.is_empty() {
                                ui.add_space(12.);
                                ui.label(muted("No notes yet."));
                                ui.label(muted("Press + New note to write your first one."));
                            }
                            tree.show(ui, "", &self.doc.name, &mut act, self.sort_desc, collapse_now);
                            return;
                        }
                        let mut hits = 0;
                        for (n, t) in &self.notes {
                            let title_hit = n.to_lowercase().contains(&q);
                            let line = t.lines().find(|l| l.to_lowercase().contains(&q));
                            if !title_hit && line.is_none() {
                                continue;
                            }
                            hits += 1;
                            let r = ui.selectable_label(*n == self.doc.name, egui::RichText::new(leaf(n)).strong().color(p.text));
                            let r = r.on_hover_text(stem(n));
                            if r.clicked() {
                                act = Some(Act::Open(n.clone()))
                            }
                            let folder = folder_of(n);
                            if !folder.is_empty() {
                                ui.label(egui::RichText::new(folder.trim_end_matches('/')).small().color(p.muted));
                            }
                            if let Some(l) = line {
                                let l = l.trim();
                                let low = l.to_lowercase();
                                let mut job = egui::text::LayoutJob::default();
                                let fmt = egui::TextFormat { font_id: egui::FontId::proportional(12.), color: p.muted, ..Default::default() };
                                let hl = egui::TextFormat { color: p.accent, ..fmt.clone() };
                                // Start the snippet at a word boundary a little before the hit.
                                let at = low.find(&q).unwrap_or(0);
                                let start = if low.len() == l.len() && at > 24 {
                                    let from = (0..=at - 24).rev().find(|i| l.is_char_boundary(*i)).unwrap_or(0);
                                    l[from..at].find(' ').map(|s| from + s + 1).unwrap_or(from)
                                } else {
                                    0
                                };
                                let seg = &l[start..];
                                let lowseg = seg.to_lowercase();
                                let lead = if start > 0 { "…" } else { "" };
                                match lowseg.find(&q).filter(|_| lowseg.len() == seg.len()) {
                                    Some(i) => {
                                        let end = i + q.len();
                                        job.append(&format!("{lead}{}", &seg[..i]), 0., fmt.clone());
                                        job.append(&seg[i..end], 0., hl);
                                        job.append(&seg[end..], 0., fmt);
                                    }
                                    None => job.append(&format!("{lead}{seg}"), 0., fmt),
                                }
                                // Indent with real space: a leading_space on the job is not
                                // counted by truncation and widened the panel every frame.
                                ui.horizontal(|ui| {
                                    ui.add_space(8.);
                                    ui.label(job);
                                });
                            }
                            ui.add_space(5.);
                        }
                        if hits == 0 {
                            ui.add_space(8.);
                            ui.label(muted(format!("Nothing matches \"{}\".", self.query)));
                            if ui.button(format!("Create note \"{}\"", self.query.trim())).clicked() {
                                act = Some(Act::Open(note_name(self.query.trim())));
                                self.query.clear();
                            }
                        }
                    });
                });
            self.tour_rects[1] = Some(side.response.rect);
        }

        // ---------------- linked-from panel
        let backlinks = if self.doc.name.is_empty() { Vec::new() } else { self.backlinks_of(&self.doc.name) };
        if !self.doc.name.is_empty() && !self.hide_links && self.view != View::Graph {
            egui::SidePanel::right("backlinks")
                .default_width(210.)
                .width_range(160.0..=360.)
                .frame(egui::Frame::new().fill(p.side).inner_margin(10.))
                .show(ctx, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("Linked from ({})", backlinks.len())).strong().color(p.muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(muted("×")).frame(false)).on_hover_text("Hide this panel").clicked() {
                                self.hide_links = true;
                            }
                        });
                    });
                    ui.add_space(4.);
                    if backlinks.is_empty() {
                        ui.label(muted("No other notes link here yet."));
                        ui.label(egui::RichText::new(format!("Type [[{}]] in another note to link it.", leaf(&self.doc.name))).small().color(p.muted));
                    }
                    for n in &backlinks {
                        ui.horizontal(|ui| {
                            if ui.link(leaf(n)).on_hover_text(stem(n)).clicked() {
                                act = Some(Act::Open(n.clone()))
                            }
                            let f = folder_of(n);
                            if !f.is_empty() {
                                ui.label(egui::RichText::new(f.trim_end_matches('/')).small().color(p.muted));
                            }
                        });
                    }
                });
        }

        // ---------------- status bar
        let status_bar = egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(p.ribbon).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let vname = self.vault.root.file_name().map_or(String::new(), |n| n.to_string_lossy().into());
                    let vaults: Vec<PathBuf> = self.prefs.recent.iter().skip(1).cloned().collect();
                    ui.menu_button(egui::RichText::new(format!("{vname} ⌄")).family(egui::FontFamily::Name("bold".into())).size(13.), |ui| {
                        ui.label(muted(self.vault.root.display().to_string()));
                        ui.separator();
                        if ui.button("Open another vault…").clicked() {
                            self.vault_win = Some(self.vault.root.display().to_string());
                            ui.close_menu();
                        }
                        if ui.button("Import a folder…").clicked() {
                            self.start_setup(SetupKind::Import);
                            ui.close_menu();
                        }
                        if ui.button("Manage vaults…").clicked() {
                            self.settings = Some(Section::Vaults);
                            ui.close_menu();
                        }
                        if !vaults.is_empty() {
                            ui.label(muted("Recent vaults"));
                            for v in &vaults {
                                if ui.button(v.display().to_string()).clicked() {
                                    self.vault_win = Some(v.display().to_string());
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                        }
                        if ui.button("Reveal vault folder").clicked() {
                            reveal(&self.vault.root);
                            ui.close_menu();
                        }
                        if ui.button("Trash…").clicked() {
                            self.trash = true;
                            ui.close_menu();
                        }
                    })
                    .response
                    .on_hover_text("Switch vault");
                    ui.label(egui::RichText::new(plural(self.notes.len(), "note")).size(13.).color(p.muted));
                    let fresh = self.status_err || self.status_at.elapsed() < Duration::from_secs(6);
                    if !self.status.is_empty() && fresh {
                        let c = if self.status_err { p.red } else { p.muted };
                        ui.label(egui::RichText::new(format!("· {}", self.status)).size(13.).color(c));
                        if !self.status_err {
                            ctx.request_repaint_after(Duration::from_secs(6));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let nconf = self.conflicts().len();
                        let (txt, col, tip) = match &self.sync_state {
                            SyncState::Off => ("Sync off".to_owned(), p.muted, "Click to set up sync".to_owned()),
                            SyncState::Syncing => ("⟳ Syncing…".to_owned(), p.muted, "Syncing with the server".to_owned()),
                            SyncState::Idle => {
                                let txt = match self.last_sync {
                                    Some(t) => format!("● Synced {}", ago(t)),
                                    None => "● Sync on".to_owned(),
                                };
                                (txt, p.accent, "Click for sync details".to_owned())
                            }
                            SyncState::Error(e) => ("● Sync error".to_owned(), p.red, format!("{e}\nClick for details")),
                        };
                        let r = ui.add(egui::Button::new(egui::RichText::new(txt).size(12.5).color(col)).frame(false)).on_hover_text(tip);
                        if r.clicked() {
                            self.settings = Some(Section::Sync);
                        }
                        if nconf > 0 && ui.add(egui::Button::new(egui::RichText::new(format!("{} ·", plural(nconf, "conflict"))).size(12.5).color(p.amber)).frame(false)).on_hover_text("Review conflicting edits").clicked() {
                            self.settings = Some(Section::Sync);
                        }
                        if self.sync_state == SyncState::Idle && self.last_sync.is_some() {
                            ctx.request_repaint_after(Duration::from_secs(10));
                        }
                    });
                });
            });
        self.tour_rects[3] = Some(status_bar.response.rect);

        // ---------------- central
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(24, 10)))
            .show(ctx, |ui| {
                if self.view == View::Graph {
                    ui.horizontal(|ui| {
                        ui.heading("Graph");
                        ui.add(egui::TextEdit::singleline(&mut self.gfilter).hint_text(muted("Filter notes")).desired_width(160.));
                        ui.add_enabled(!self.doc.name.is_empty(), egui::Checkbox::new(&mut self.glocal, "Only around current note"));
                        ui.checkbox(&mut self.glabels, "Labels");
                        if ui.button("Reset view").clicked() {
                            self.gzoom = 1.;
                            self.gpan = Vec2::ZERO;
                            self.gpos.clear();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close graph").clicked() {
                                self.view = View::Edit
                            }
                        });
                    });
                    ui.label(muted("Click a note to open it. Drag to move, scroll to zoom."));
                    if let Some(n) = self.graph(ui) {
                        act = Some(Act::Open(n))
                    }
                    return;
                }
                // Tabs scroll sideways instead of pushing the rest of the window.
                let mut close: Option<String> = None;
                egui::ScrollArea::horizontal().id_salt("tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for tab in self.tabs.clone() {
                            let mut title = leaf(&tab).to_owned();
                            if title.chars().count() > 22 {
                                title = title.chars().take(21).collect::<String>() + "…";
                            }
                            if self.doc.dirty && tab == self.doc.name {
                                title.push_str(" ●")
                            }
                            let active = tab == self.doc.name;
                            egui::Frame::new()
                                .fill(if active { p.side } else { Color32::TRANSPARENT })
                                .corner_radius(egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 })
                                .inner_margin(egui::Margin::symmetric(12, 5))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.;
                                    let l = ui.add(egui::Label::new(egui::RichText::new(title).color(if active { p.text } else { p.muted })).selectable(false).sense(egui::Sense::click()));
                                    let l = l.on_hover_text(stem(&tab));
                                    if l.clicked() {
                                        act = Some(Act::Open(tab.clone()));
                                    }
                                    if l.middle_clicked() {
                                        close = Some(tab.clone());
                                    }
                                    if ui.add(egui::Button::new(muted("×")).frame(false)).on_hover_text("Close tab (Ctrl+W)").clicked() {
                                        close = Some(tab.clone());
                                    }
                                });
                        }
                        if ui.add(egui::Button::new(egui::RichText::new("+").size(16.).color(p.muted)).frame(false)).on_hover_text("New note (Ctrl+N)").clicked() {
                            want_new = Some(String::new());
                        }
                    });
                });
                if let Some(t) = close {
                    result = self.close_tab(&t);
                }
                ui.separator();
                if self.doc.name.is_empty() {
                    ui.add_space(ui.available_height() * 0.28);
                    ui.vertical_centered(|ui| {
                        ui.label(muted(if self.notes.is_empty() { "Welcome to Emerald. Start with a note." } else { "No note open." }));
                        ui.add_space(8.);
                        let b = |ui: &mut egui::Ui, i: &str, t: &str, k: &str| {
                            let img = egui::Image::new(svg(i)).tint(pal().text).fit_to_exact_size(Vec2::splat(16.));
                            ui.add_sized([260., 34.], egui::Button::image_and_text(img, t).shortcut_text(k).corner_radius(17.)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                        };
                        if b(ui, "new", "New note", "Ctrl+N") {
                            want_new = Some(String::new());
                        }
                        if b(ui, "search", "Search notes", "Ctrl+O") {
                            self.sidebar = true;
                            self.focus_search = true
                        }
                        if b(ui, "today", "Today's daily note", "") {
                            act = Some(Act::Open(format!("Daily/{}.md", chrono::Local::now().date_naive())))
                        }
                        if b(ui, "graph", "Graph view", "Ctrl+G") {
                            self.view = View::Graph
                        }
                    });
                    return;
                }
                if self.sync_state == SyncState::Off && !self.banner_off {
                    ui.horizontal(|ui| {
                        ui.label(muted("Want these notes on another device?"));
                        if ui.link("Set up sync").clicked() {
                            self.settings = Some(Section::Sync)
                        }
                        if ui.add(egui::Button::new(muted("×")).frame(false)).on_hover_text("Don't show again").clicked() {
                            self.banner_off = true;
                            let _ = fs::write(banner_flag(&self.vault.root), "");
                        }
                    });
                }
                // Title: the note's own name, with its folder as a breadcrumb.
                let folder = folder_of(&self.doc.name).to_owned();
                if !folder.is_empty() {
                    ui.label(muted(folder.trim_end_matches('/').replace('/', " › ") + " ›"));
                }
                let current = leaf(&self.doc.name).to_owned();
                let hovering = ui.rect_contains_pointer(Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 40.)));
                let editing = ui.memory(|m| m.has_focus(egui::Id::new("title")));
                let frame = egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0_f32, if hovering || editing { p.line } else { Color32::TRANSPARENT }))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(4, 0));
                let t = frame
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let hint = if editing { "Enter to rename" } else if hovering && !self.busy() { "click to rename" } else { "" };
                                ui.add_sized([110., 20.], egui::Label::new(muted(hint)));
                                egui::TextEdit::singleline(&mut self.title)
                                    .id(egui::Id::new("title"))
                                    .font(egui::FontId::new(26., egui::FontFamily::Name("bold".into())))
                                    .frame(false)
                                    .desired_width(ui.available_width())
                                    .show(ui)
                            })
                            .inner
                        })
                        .inner
                    })
                    .inner;
                self.tour_rects[2] = Some(t.response.rect.expand(4.));
                let t_resp = t.response.clone().on_hover_text("Click to rename this note");
                if std::mem::take(&mut self.focus_title) {
                    t_resp.request_focus();
                }
                if std::mem::take(&mut self.select_title) {
                    let mut st = t.state.clone();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.title.chars().count()))));
                    st.store(ui.ctx(), t_resp.id);
                }
                if t_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.title = current.clone();
                    ui.memory_mut(|m| m.surrender_focus(t_resp.id));
                } else if t_resp.lost_focus() {
                    let typed = self.title.trim().to_owned();
                    if typed.is_empty() {
                        self.title = current.clone();
                    } else if typed != current {
                        // A name with a slash is a full path; otherwise keep the folder.
                        let to = if typed.contains('/') { typed } else { format!("{folder}{typed}") };
                        if let Err(e) = self.rename_to(to) {
                            result = Err(e);
                            self.title = current.clone();
                        }
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.focus_editor = true;
                    }
                }
                // Toolbar.
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.view, View::Edit, "Edit");
                    ui.selectable_value(&mut self.view, View::Preview, "Preview");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("•••", |ui| {
                            if ui.button("Rename").clicked() {
                                self.focus_title = true;
                                self.select_title = true;
                                ui.close_menu();
                            }
                            if ui.button("Move to folder…").clicked() {
                                self.move_win = Some((self.doc.name.clone(), folder.trim_end_matches('/').to_owned()));
                                ui.close_menu();
                            }
                            if ui.button("Duplicate").clicked() {
                                act = Some(Act::Duplicate(self.doc.name.clone()));
                                ui.close_menu();
                            }
                            if ui.button("Save now").on_hover_text("Notes also save a second after you stop typing").clicked() {
                                result = self.save();
                                ui.close_menu();
                            }
                            if ui.button("Reload from disk").on_hover_text("Discard this editor buffer and read the file again").clicked() {
                                result = self.load(self.doc.name.clone());
                                ui.close_menu();
                            }
                            if ui.button("Reveal in file manager").clicked() {
                                act = Some(Act::Reveal(self.doc.name.clone()));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button(egui::RichText::new("Delete…").color(p.red)).clicked() {
                                self.confirm_delete = Some(self.doc.name.clone());
                                ui.close_menu();
                            }
                        });
                        if self.hide_links && ui.button(format!("Linked from {}", backlinks.len())).on_hover_text("Show the notes that link here").clicked() {
                            self.hide_links = false;
                        }
                    });
                });
                // Outgoing links, by note name, once each.
                let outgoing = unique_links(&self.doc.text);
                if !outgoing.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(muted("Links:"));
                        for n in outgoing {
                            let exists = self.notes.contains_key(&n);
                            let text = if exists { egui::RichText::new(leaf(&n)) } else { egui::RichText::new(format!("{} (new)", leaf(&n))).color(p.muted) };
                            let tip = if exists { stem(&n).to_owned() } else { format!("{} doesn't exist yet. Click to create it.", stem(&n)) };
                            if ui.link(text).on_hover_text(tip).clicked() {
                                act = Some(Act::Open(n))
                            }
                        }
                    });
                }
                ui.add_space(4.);
                egui::ScrollArea::vertical().id_salt("document").show(ui, |ui| {
                    if self.view == View::Edit {
                        self.editor(ui);
                    } else {
                        let md = wiki_to_md(&self.doc.text);
                        let targets = unique_links(&self.doc.text);
                        for n in &targets {
                            self.cache.add_link_hook(format!("emerald:{}", stem(n)));
                        }
                        CommonMarkViewer::new().show(ui, &mut self.cache, &md);
                        for n in targets {
                            let hook = format!("emerald:{}", stem(&n));
                            if self.cache.get_link_hook(&hook) == Some(true) {
                                act = Some(Act::Open(n));
                            }
                        }
                    }
                });
            });
        if self.doc.name.is_empty() {
            self.tour_rects[2] = Some(central.response.rect.shrink(40.));
        }

        // ---------------- deferred actions
        if let Some(folder) = want_new {
            if let Err(e) = self.new_note(&folder) {
                result = Err(e);
            }
        }
        if let Some(a) = act {
            let r = match a {
                Act::Open(n) => self.open(n),
                Act::Rename(n) => self.open(n).map(|_| {
                    self.focus_title = true;
                    self.select_title = true;
                    self.focus_editor = false;
                }),
                Act::Move(n) => {
                    self.move_win = Some((n.clone(), folder_of(&n).trim_end_matches('/').to_owned()));
                    Ok(())
                }
                Act::Duplicate(n) => self.duplicate(&n),
                Act::Delete(n) => {
                    self.confirm_delete = Some(n);
                    Ok(())
                }
                Act::NewIn(f) => self.new_note(&f),
                Act::RenameFolder(f) => {
                    self.folder_rename = Some((f.clone(), f.trim_end_matches('/').to_owned()));
                    Ok(())
                }
                Act::Reveal(n) => target(&self.vault.root, n.trim_end_matches('/')).map(|p| reveal(&p)),
            };
            if let Err(e) = r {
                result = Err(e);
            }
        }

        // ---------------- windows
        self.palette_window(ctx, &mut result);
        self.settings_window(ctx, &mut result);
        self.prompts(ctx, &mut result, &backlinks);
        self.trash_window(ctx, &mut result);
        self.compare_window(ctx, &mut result);
        self.undo_toast(ctx, &mut result);
        self.vault_window(ctx);
        self.welcome_window(ctx);
        self.setup_window(ctx);
        self.tour_window(ctx, &mut result);
        if self.show_log {
            let mut win = true;
            egui::Window::new("Log").open(&mut win).default_size([480., 260.]).show(ctx, |ui| {
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    if self.log.is_empty() {
                        ui.label(muted("Nothing logged yet."));
                    }
                    for l in &self.log {
                        ui.label(egui::RichText::new(l.as_str()).size(12.));
                    }
                });
            });
            self.show_log = win;
        }
        if let Err(e) = result {
            self.fail(e);
        }
    }
}

impl Notebook {
    fn editor(&mut self, ui: &mut egui::Ui) {
        let p = pal();
        let eid = ui.make_persistent_id("doceditor");
        let matches_for = |pick: &Option<(usize, usize, String)>, notes: &BTreeMap<String, String>| -> Vec<String> {
            match pick {
                Some((_, _, q)) => {
                    let mut m: Vec<(i32, String)> = notes.keys().filter_map(|n| fuzzy(stem(n), q).map(|s| (s, n.clone()))).collect();
                    m.sort_by(|a, b| b.0.cmp(&a.0));
                    m.into_iter().take(6).map(|x| x.1).collect()
                }
                None => Vec::new(),
            }
        };
        let matches = matches_for(&self.pick, &self.notes);
        let mut insert: Option<String> = None;
        if self.pick.is_some() && !matches.is_empty() {
            let (take, up, down, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                )
            });
            if down {
                self.pick_sel = (self.pick_sel + 1).min(matches.len() - 1);
            }
            if up {
                self.pick_sel = self.pick_sel.saturating_sub(1);
            }
            if take {
                insert = Some(matches[self.pick_sel.min(matches.len() - 1)].clone())
            }
            if esc {
                self.pick = None;
            }
        }
        let out = egui::TextEdit::multiline(&mut self.doc.text)
            .id(eid)
            .frame(false)
            .hint_text(muted("Start typing… type [[ to link another note"))
            .desired_width(f32::INFINITY)
            .desired_rows(26)
            .show(ui);
        let r = out.response.clone();
        let cursor = out.cursor_range.map(|c| c.primary.ccursor.index);
        let before_pick = self.pick.clone();
        if r.has_focus() {
            let now = cursor.and_then(|c| pick_at(&self.doc.text, c));
            if now.as_ref().map(|x| &x.2) != self.pick.as_ref().map(|x| &x.2) {
                self.pick_sel = 0;
            }
            self.pick = now;
        } else if insert.is_none() {
            self.pick = None
        }
        if self.pick != before_pick {
            ui.ctx().request_repaint();
        }
        let matches = matches_for(&self.pick, &self.notes);
        if let (Some((s, c, _)), false) = (self.pick.clone(), matches.is_empty()) {
            // Anchor the suggestions just below the text cursor.
            let crect = out.galley.pos_from_cursor(&out.galley.from_ccursor(egui::text::CCursor::new(c)));
            let at = out.galley_pos + crect.left_bottom().to_vec2() + Vec2::new(0., 4.);
            egui::Area::new(egui::Id::new("pickpop")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(240.);
                    for (i, m) in matches.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(i == self.pick_sel, leaf(m)).clicked() {
                                insert = Some(m.clone())
                            }
                            let f = folder_of(m);
                            if !f.is_empty() {
                                ui.label(egui::RichText::new(f.trim_end_matches('/')).small().color(p.muted));
                            }
                        });
                    }
                    ui.label(egui::RichText::new("↑↓ choose · Enter insert · Esc close").small().color(p.muted));
                });
            });
            if let Some(m) = insert.take() {
                let chars: Vec<char> = self.doc.text.chars().collect();
                let mut text: String = chars[..s].iter().collect();
                text.push_str(stem(&m));
                text.push_str("]]");
                let end = text.chars().count();
                // Skip a closing "]]" the user already typed.
                let mut rest = c.min(chars.len());
                if chars.get(rest) == Some(&']') && chars.get(rest + 1) == Some(&']') {
                    rest += 2;
                }
                text.extend(chars[rest..].iter());
                self.doc.text = text;
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), eid) {
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
                    st.store(ui.ctx(), eid);
                }
                self.pick = None;
                self.doc.dirty = true;
                self.last_edit = Some(Instant::now());
                r.request_focus();
            }
        }
        if std::mem::take(&mut self.focus_editor) {
            r.request_focus()
        }
        if r.changed() {
            self.doc.dirty = true;
            self.last_edit = Some(Instant::now());
        }
    }

    fn run_command(&mut self, ctx: &egui::Context, id: &str) -> Result<()> {
        match id {
            "new" => self.new_note("")?,
            "folder" => {
                self.sidebar = true;
                self.new_folder = Some(String::new());
            }
            "graph" => self.view = View::Graph,
            "daily" => self.open(format!("Daily/{}.md", chrono::Local::now().date_naive()))?,
            "search" => {
                self.sidebar = true;
                self.focus_search = true;
                self.view = View::Edit;
            }
            "side" => self.sidebar = !self.sidebar,
            "links" => self.hide_links = !self.hide_links,
            "sync" => {
                if self.sync_state == SyncState::Off {
                    self.settings = Some(Section::Sync)
                } else {
                    self.sync(ctx)?
                }
            }
            "settings" => self.settings = Some(Section::Sync),
            "appearance" => self.settings = Some(Section::Appearance),
            "theme" => {
                self.prefs.theme = if IS_LIGHT.load(Ordering::Relaxed) { ThemePref::Dark } else { ThemePref::Light };
                save_json(prefs_file(), &self.prefs);
            }
            "log" => self.show_log = true,
            "vault" => self.vault_win = Some(self.vault.root.display().to_string()),
            "vault-new" => self.start_setup(SetupKind::New),
            "vault-import" => self.start_setup(SetupKind::Import),
            "vault-server" => self.start_setup(SetupKind::Server),
            "vaults" => self.settings = Some(Section::Vaults),
            "trash" => self.trash = true,
            "tour" => self.tour = Some(0),
            "shortcuts" => self.settings = Some(Section::Shortcuts),
            "rename" => {
                self.focus_title = true;
                self.select_title = true;
            }
            "move" => self.move_win = Some((self.doc.name.clone(), folder_of(&self.doc.name).trim_end_matches('/').to_owned())),
            "delete" => self.confirm_delete = Some(self.doc.name.clone()),
            "preview" => self.view = if self.view == View::Preview { View::Edit } else { View::Preview },
            n => {
                if let Some(n) = n.strip_prefix("note:") {
                    self.open(n.to_owned())?
                }
            }
        }
        Ok(())
    }

    fn palette_window(&mut self, ctx: &egui::Context, result: &mut Result<()>) {
        if !self.palette {
            return;
        }
        let p = pal();
        let mut cmds: Vec<(String, &str, &str)> = vec![
            ("New note".into(), "Ctrl+N", "new"),
            ("New folder".into(), "", "folder"),
            ("Search notes".into(), "Ctrl+O", "search"),
            ("Open today's daily note".into(), "", "daily"),
            ("Open graph view".into(), "Ctrl+G", "graph"),
            ("Toggle sidebar".into(), "", "side"),
            ("Toggle \"Linked from\" panel".into(), "", "links"),
            ("Sync now".into(), "", "sync"),
            ("Settings".into(), "", "settings"),
            ("Switch light / dark theme".into(), "", "theme"),
            ("Open another vault…".into(), "", "vault"),
            ("New vault…".into(), "", "vault-new"),
            ("Import a folder as a vault…".into(), "", "vault-import"),
            ("Open a vault from the server…".into(), "", "vault-server"),
            ("Manage vaults".into(), "", "vaults"),
            ("Show trash".into(), "", "trash"),
            ("Show log".into(), "", "log"),
            ("Keyboard shortcuts".into(), "", "shortcuts"),
            ("Show tour".into(), "", "tour"),
        ];
        if !self.doc.name.is_empty() {
            cmds.push(("Rename current note".into(), "", "rename"));
            cmds.push(("Move current note to folder…".into(), "", "move"));
            cmds.push(("Toggle preview".into(), "", "preview"));
            cmds.push(("Delete current note".into(), "", "delete"));
        }
        let q = self.pal_query.trim().to_owned();
        let mut shown: Vec<(String, String, String, bool)> = Vec::new();
        if q.is_empty() {
            // Recently opened notes first.
            for t in self.tabs.iter().rev().filter(|t| **t != self.doc.name).take(5) {
                shown.push((leaf(t).to_owned(), "recent".into(), format!("note:{t}"), true));
            }
        } else {
            let mut notes: Vec<(i32, &String)> = self.notes.keys().filter_map(|n| fuzzy(stem(n), &q).map(|s| (s, n))).collect();
            notes.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, n) in notes.into_iter().take(8) {
                shown.push((leaf(n).to_owned(), folder_of(n).trim_end_matches('/').to_owned(), format!("note:{n}"), true));
            }
        }
        let mut c: Vec<(i32, (String, &str, &str))> = cmds.into_iter().filter_map(|x| fuzzy(&x.0, &q).map(|s| (s, x))).collect();
        c.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, (l, k, id)) in c {
            shown.push((l, k.to_owned(), id.to_owned(), false));
        }
        let (up, down, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if !shown.is_empty() {
            if down {
                self.pal_sel = (self.pal_sel + 1) % shown.len();
            }
            if up {
                self.pal_sel = (self.pal_sel + shown.len() - 1) % shown.len();
            }
            self.pal_sel = self.pal_sel.min(shown.len() - 1);
        }
        let mut chosen: Option<String> = None;
        let mut keep = !esc;
        let before = self.pal_query.clone();
        egui::Window::new("palette")
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0., 80.])
            .min_width(480.)
            .max_width(480.)
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.pal_query).hint_text(muted("Jump to a note or run a command…")).desired_width(f32::INFINITY));
                r.request_focus();
                ui.separator();
                egui::ScrollArea::vertical().max_height(360.).show(ui, |ui| {
                    let w = ui.available_width();
                    let mut last_note = None;
                    for (i, (label, key, id, is_note)) in shown.iter().enumerate() {
                        if last_note != Some(*is_note) {
                            ui.label(egui::RichText::new(if *is_note { if q.is_empty() { "Recent notes" } else { "Notes" } } else { "Commands" }).small().color(p.muted));
                            last_note = Some(*is_note);
                        }
                        let sel = i == self.pal_sel;
                        let b = ui.add(egui::Button::new(label.as_str()).shortcut_text(key.as_str()).selected(sel).frame(sel).min_size(Vec2::new(w, 26.)));
                        if sel && (up || down) {
                            b.scroll_to_me(None);
                        }
                        if b.hovered() && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
                            self.pal_sel = i;
                        }
                        if b.clicked() {
                            chosen = Some(id.clone())
                        }
                    }
                    if shown.is_empty() {
                        ui.label(muted("No matches."));
                        if ui.button(format!("Create note \"{q}\"")).clicked() {
                            chosen = Some(format!("note:{}", note_name(&q)));
                        }
                    }
                });
                ui.label(egui::RichText::new("↑↓ move · Enter run · Esc close").small().color(p.muted));
            });
        if self.pal_query != before {
            self.pal_sel = 0;
        }
        if enter {
            chosen = shown.get(self.pal_sel).map(|s| s.2.clone()).or_else(|| (!q.is_empty()).then(|| format!("note:{}", note_name(&q))));
        }
        if let Some(id) = chosen {
            keep = false;
            if let Err(e) = self.run_command(ctx, &id) {
                *result = Err(e);
            }
        }
        self.palette = keep;
    }

    fn settings_window(&mut self, ctx: &egui::Context, result: &mut Result<()>) {
        let Some(mut section) = self.settings else {
            return;
        };
        let p = pal();
        let mut open = true;
        egui::Window::new("Settings").open(&mut open).collapsible(false).default_width(500.).anchor(egui::Align2::CENTER_TOP, [0., 60.]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (s, l) in [(Section::Sync, "Sync"), (Section::Appearance, "Appearance"), (Section::Vaults, "Vaults"), (Section::Shortcuts, "Shortcuts")] {
                    ui.selectable_value(&mut section, s, l);
                }
            });
            ui.separator();
            match section {
                Section::Sync => self.sync_section(ui, ctx, result),
                Section::Appearance => {
                    ui.label("Theme");
                    ui.horizontal(|ui| {
                        let before = self.prefs.theme;
                        ui.radio_value(&mut self.prefs.theme, ThemePref::System, "Match system");
                        ui.radio_value(&mut self.prefs.theme, ThemePref::Dark, "Dark");
                        ui.radio_value(&mut self.prefs.theme, ThemePref::Light, "Light");
                        if self.prefs.theme != before {
                            save_json(prefs_file(), &self.prefs);
                        }
                    });
                    ui.add_space(8.);
                    ui.label("Text size");
                    ui.horizontal(|ui| {
                        let z = ctx.zoom_factor();
                        if ui.button("A−").clicked() {
                            ctx.set_zoom_factor((z - 0.1).clamp(0.7, 2.));
                        }
                        ui.label(format!("{:.0}%", z * 100.));
                        if ui.button("A+").clicked() {
                            ctx.set_zoom_factor((z + 0.1).clamp(0.7, 2.));
                        }
                        if ui.button("Reset").clicked() {
                            ctx.set_zoom_factor(1.);
                        }
                        ui.label(muted("Ctrl+= / Ctrl+− / Ctrl+0"));
                    });
                }
                Section::Vaults => self.vaults_section(ui),
                Section::Shortcuts => {
                    egui::Grid::new("keys").num_columns(2).spacing([24., 6.]).show(ui, |ui| {
                        for (k, d) in [
                            ("Ctrl+N", "New note"),
                            ("Ctrl+O, Ctrl+Shift+F", "Search notes"),
                            ("Ctrl+P, Ctrl+K", "Jump to note / command palette"),
                            ("Ctrl+G", "Graph view"),
                            ("Ctrl+W", "Close tab"),
                            ("Ctrl+S", "Save now (notes also save after 1 s)"),
                            ("Ctrl+= / − / 0", "Zoom in / out / reset"),
                            ("[[", "Link another note"),
                            ("Esc", "Close search, palette or rename"),
                        ] {
                            ui.label(egui::RichText::new(k).strong());
                            ui.label(d);
                            ui.end_row();
                        }
                    });
                }
            }
            let _ = p;
        });
        // Opening a vault setup window replaces Settings.
        self.settings = (open && self.setup.is_none()).then_some(section);
        if !open {
            self.secret.clear();
            self.edit_server = false;
        }
    }

    fn sync_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, result: &mut Result<()>) {
        let p = pal();
        let current = sync::current(&self.vault);
        if let (Some((server, remote_vault, env)), false) = (&current, self.edit_server) {
            let (txt, col) = match &self.sync_state {
                SyncState::Syncing => ("Syncing…".to_owned(), p.muted),
                SyncState::Error(e) => (format!("Last sync failed: {e}"), p.red),
                _ => (format!("Synced {}", self.last_sync.map(ago).unwrap_or_else(|| "— waiting for first pass".into())), p.accent),
            };
            ui.label(egui::RichText::new(txt).color(col));
            ui.label(format!("Server: {server}"));
            ui.label(format!("Server vault: {}", remote_vault.as_deref().unwrap_or("main vault")));
            if *env {
                ui.label(muted("Set by the EMERALD_SERVER, EMERALD_TOKEN and EMERALD_VAULT environment variables."));
            }
            ui.label(muted("Changes sync automatically every few seconds while Emerald is open."));
            ui.add_space(6.);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy(), egui::Button::new("Sync now")).clicked() {
                    if let Err(e) = self.sync(ctx) {
                        *result = Err(e);
                    }
                }
                if !env && ui.button("Change server…").clicked() {
                    self.edit_server = true;
                    self.server = server.clone();
                    self.server_vault = remote_vault.clone().unwrap_or_default();
                    self.server_vaults = None;
                    self.test_msg = None;
                }
                if !env && ui.button("Disconnect").on_hover_text("Stop syncing this vault. Your notes stay on this computer.").clicked() {
                    match sync::disconnect(&self.vault) {
                        Ok(()) => {
                            self.sync_state = SyncState::Off;
                            self.last_sync = None;
                            self.say("Sync turned off");
                        }
                        Err(e) => *result = Err(e),
                    }
                }
                if ui.button("Show log").clicked() {
                    self.show_log = true;
                }
            });
            let conflicts = self.conflicts();
            if !conflicts.is_empty() {
                ui.add_space(10.);
                ui.label(egui::RichText::new(format!("{} {} a decision", plural(conflicts.len(), "conflict"), if conflicts.len() == 1 { "needs" } else { "need" })).color(p.amber).strong());
                ui.label(muted("Both sides edited these notes. Compare them and keep the version you want."));
                for c in conflicts {
                    ui.horizontal(|ui| {
                        ui.label(conflict_original(&c).map(|o| stem(&o).to_owned()).unwrap_or_default());
                        if ui.small_button("Compare…").clicked() {
                            self.compare = Some(c.clone());
                        }
                    });
                }
            }
            return;
        }
        ui.label(egui::RichText::new("Keep this vault the same on several computers").strong());
        ui.label(muted("Sync uses an Emerald Server that you run yourself. It keeps a copy of your notes as plain files."));
        ui.add_space(4.);
        ui.label(muted("1. Start emerald-server on the computer that will keep the shared copy (see SERVER_SETUP.md).\n2. Enter its address and the token it was started with.\n3. Press Test, then Connect."));
        ui.add_space(8.);
        ui.label("Server address");
        ui.add(egui::TextEdit::singleline(&mut self.server).hint_text(muted("https://notes.example.org")).desired_width(360.));
        ui.label(egui::RichText::new("Same computer: http://127.0.0.1:7878 · Other computers need an https:// address.").small().color(p.muted));
        ui.add_space(4.);
        ui.label("Server token");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.secret).password(!self.show_secret).desired_width(300.));
            ui.checkbox(&mut self.show_secret, "Show");
        });
        ui.label(egui::RichText::new(format!("The EMERALD_TOKEN value, at least 32 characters ({} typed).", self.secret.chars().count())).small().color(p.muted));
        ui.add_space(4.);
        ui.label("Server vault");
        ui.add(egui::TextEdit::singleline(&mut self.server_vault).hint_text(muted("Empty: the server's main vault")).desired_width(300.));
        if let Some(list) = &self.server_vaults {
            if list.is_empty() {
                ui.label(egui::RichText::new("The server has no named vaults yet. Type a name to create one.").small().color(p.muted));
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("On the server:").small().color(p.muted));
                    for n in list {
                        if ui.selectable_label(self.server_vault.trim() == n, n.as_str()).clicked() {
                            self.server_vault = n.clone();
                        }
                    }
                });
            }
        } else {
            ui.label(egui::RichText::new("Each server can hold several vaults. Press Test to list them.").small().color(p.muted));
        }
        ui.add_space(6.);
        ui.horizontal(|ui| {
            let testing = self.test_rx.is_some();
            if ui.add_enabled(!testing, egui::Button::new(if testing { "Testing…" } else { "Test" })).clicked() {
                let (tx, rx) = mpsc::channel();
                let (s, t) = (self.server.trim().to_owned(), self.secret.trim().to_owned());
                let v = self.server_vault.trim().to_owned();
                let c = ctx.clone();
                self.test_msg = None;
                self.test_rx = Some(rx);
                std::thread::spawn(move || {
                    let r = sync::test(&s, &t, Some(&v)).map(|m| (m, sync::vaults(&s, &t).ok()));
                    let _ = tx.send(r);
                    c.request_repaint();
                });
            }
            if primary(ui, "Connect").clicked() {
                let vault = Some(self.server_vault.trim().to_owned()).filter(|n| !n.is_empty());
                let c = sync::Connection { server: self.server.trim().to_owned(), token: self.secret.trim().to_owned(), vault };
                let created = match &c.vault {
                    // Named vaults are created on first connect, so they show up for other clients.
                    Some(n) => sync::create_vault(&c.server, &c.token, n),
                    None => Ok(()),
                };
                match created.and_then(|_| sync::configure(&self.vault, c)) {
                    Ok(()) => {
                        self.secret.clear();
                        self.edit_server = false;
                        self.sync_state = SyncState::Idle;
                        self.last_sync = None;
                        self.last_auto_sync = Instant::now() - Duration::from_secs(10);
                        self.test_msg = None;
                        self.say("Sync connected");
                    }
                    Err(e) => self.test_msg = Some((false, friendly(&e))),
                }
            }
            if self.edit_server && ui.button("Cancel").clicked() {
                self.edit_server = false;
                self.secret.clear();
                self.test_msg = None;
            }
        });
        if let Some(rx) = &self.test_rx {
            if let Ok(r) = rx.try_recv() {
                self.test_msg = Some(match r {
                    Ok((m, list)) => {
                        self.server_vaults = list;
                        (true, format!("{m} Press Connect to start syncing."))
                    }
                    Err(e) => (false, friendly(&e)),
                });
                self.test_rx = None;
            }
        }
        if let Some((ok, m)) = &self.test_msg {
            ui.add_space(4.);
            ui.label(egui::RichText::new(m.as_str()).color(if *ok { p.accent } else { p.red }));
        }
    }

    /// Delete confirmation, move-to-folder and folder rename prompts.
    fn prompts(&mut self, ctx: &egui::Context, result: &mut Result<()>, backlinks_cur: &[String]) {
        let p = pal();
        if let Some(name) = self.confirm_delete.clone() {
            let linked = if name == self.doc.name { backlinks_cur.to_vec() } else { self.backlinks_of(&name) };
            let mut close = false;
            let m = egui::Modal::new(egui::Id::new("delete")).show(ctx, |ui| {
                ui.set_width(340.);
                ui.heading(format!("Delete \"{}\"?", leaf(&name)));
                ui.add_space(4.);
                ui.label("It moves to the Trash, where you can restore it later.");
                if !linked.is_empty() {
                    ui.label(egui::RichText::new(format!("{} {} to it: {}.", plural(linked.len(), "note"), if linked.len() == 1 { "links" } else { "link" }, linked.iter().map(|n| leaf(n)).collect::<Vec<_>>().join(", "))).color(p.amber));
                }
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Move to Trash").color(Color32::WHITE)).fill(p.red)).clicked() {
                        if let Err(e) = self.delete_note(&name) {
                            *result = Err(e);
                        }
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if close || m.should_close() {
                self.confirm_delete = None;
            }
        }
        if let Some((note, mut folder)) = self.move_win.take() {
            let mut keep = true;
            let mut go = false;
            egui::Window::new("Move note").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0., 120.]).show(ctx, |ui| {
                ui.label(format!("Move \"{}\" to folder", leaf(&note)));
                let r = ui.add(egui::TextEdit::singleline(&mut folder).hint_text(muted("Folder, e.g. Projects/2026 (empty = top level)")).desired_width(320.));
                r.request_focus();
                go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let mut folders: Vec<&str> = self.notes.keys().map(|n| folder_of(n).trim_end_matches('/')).filter(|f| !f.is_empty()).collect();
                folders.dedup();
                if !folders.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for f in folders {
                            if ui.small_button(f).clicked() {
                                folder = f.to_owned();
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    go |= primary(ui, "Move").clicked();
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        keep = false
                    }
                });
            });
            if go {
                let f = folder.trim().trim_matches('/');
                let to = if f.is_empty() { format!("{}.md", leaf(&note)) } else { format!("{f}/{}.md", leaf(&note)) };
                let r = if self.doc.name == note { Ok(()) } else { self.open(note.clone()) };
                if let Err(e) = r.and_then(|_| self.rename_to(to)) {
                    *result = Err(e);
                }
            } else if keep {
                self.move_win = Some((note, folder));
            }
        }
        if let Some((from, mut to)) = self.folder_rename.take() {
            let mut keep = true;
            let mut go = false;
            egui::Window::new("Rename folder").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0., 120.]).show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut to).desired_width(300.));
                r.request_focus();
                go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.label(muted("Notes inside move with it. Links to them are not rewritten."));
                ui.horizontal(|ui| {
                    go |= primary(ui, "Rename").clicked();
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        keep = false
                    }
                });
            });
            if go {
                if let Err(e) = self.rename_folder(&from, &to) {
                    *result = Err(e);
                }
            } else if keep {
                self.folder_rename = Some((from, to));
            }
        }
    }

    fn undo_toast(&mut self, ctx: &egui::Context, result: &mut Result<()>) {
        let Some((name, from, at)) = self.undo.clone() else {
            return;
        };
        if at.elapsed() > Duration::from_secs(10) {
            self.undo = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let p = pal();
        egui::Area::new(egui::Id::new("undo-toast")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, [0., -40.]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Moved \"{}\" to Trash", leaf(&name)));
                    if ui.add(egui::Button::new(egui::RichText::new("Undo").strong().color(p.accent))).clicked() {
                        self.undo = None;
                        if let Err(e) = self.restore(&name, &from) {
                            *result = Err(e);
                        }
                    }
                    if ui.add(egui::Button::new(muted("×")).frame(false)).clicked() {
                        self.undo = None;
                    }
                });
            });
        });
    }

    fn trash_window(&mut self, ctx: &egui::Context, result: &mut Result<()>) {
        if !self.trash {
            return;
        }
        let p = pal();
        let dir = self.vault.root.join(".trash");
        let mut items: Vec<String> = fs::read_dir(&dir).map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        items.sort();
        items.reverse();
        let mut open = true;
        egui::Window::new("Trash").open(&mut open).default_width(560.).show(ctx, |ui| {
            if items.is_empty() {
                ui.label(muted("Trash is empty."));
            }
            egui::ScrollArea::vertical().max_height(360.).show(ui, |ui| {
                for f in &items {
                    let (name, when) = trash_entry(f).unwrap_or((f.clone(), String::new()));
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(Vec2::new(230., 18.), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_min_width(230.);
                            ui.add(egui::Label::new(stem(&name)).truncate()).on_hover_text(f);
                        });
                        ui.allocate_ui_with_layout(Vec2::new(110., 18.), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_min_width(110.);
                            ui.label(muted(&when));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.trash_confirm.as_deref() == Some(f.as_str()) {
                                if ui.add(egui::Button::new(egui::RichText::new("Confirm delete").color(Color32::WHITE)).fill(p.red)).clicked() {
                                    if let Err(e) = fs::remove_file(dir.join(f)) {
                                        *result = Err(e.into());
                                    }
                                    self.trash_confirm = None;
                                }
                            } else if ui.small_button("Delete forever").clicked() {
                                self.trash_confirm = Some(f.clone());
                            }
                            if ui.small_button("Restore").clicked() {
                                if let Err(e) = self.restore(&name, &dir.join(f)) {
                                    *result = Err(e);
                                }
                            }
                        });
                    });
                }
            });
        });
        self.trash = open;
    }

    fn compare_window(&mut self, ctx: &egui::Context, result: &mut Result<()>) {
        let Some(copy) = self.compare.clone() else {
            return;
        };
        let Some(orig) = conflict_original(&copy) else {
            self.compare = None;
            return;
        };
        let theirs = self.notes.get(&orig).cloned().unwrap_or_default();
        let mine = self.notes.get(&copy).cloned().unwrap_or_default();
        let mut open = true;
        let mut choice: Option<bool> = None;
        egui::Window::new(format!("Conflict: {}", stem(&orig))).open(&mut open).default_size([760., 440.]).show(ctx, |ui| {
            ui.label(muted("The server's version kept the note's name. Your edit was saved as a copy."));
            ui.columns(2, |cols| {
                for (ui, (title, text)) in cols.iter_mut().zip([("Server version", &theirs), ("Your version (copy)", &mine)]) {
                    ui.label(egui::RichText::new(title).strong());
                    egui::ScrollArea::vertical().id_salt(title).max_height(320.).show(ui, |ui| {
                        let mut t = text.as_str();
                        ui.add(egui::TextEdit::multiline(&mut t).desired_width(f32::INFINITY));
                    });
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Keep server version").clicked() {
                    choice = Some(false);
                }
                if primary(ui, "Keep my version").clicked() {
                    choice = Some(true);
                }
                if ui.button("Keep both").on_hover_text("Leave both notes as they are").clicked() {
                    self.compare = None;
                }
            });
        });
        if let Some(keep_mine) = choice {
            let r = (|| -> Result<()> {
                ensure!(!self.busy(), "Wait for sync to finish, then try again");
                self.save()?;
                if keep_mine {
                    atomic(&target(&self.vault.root, &orig)?, mine.as_bytes())?;
                }
                let dir = self.vault.root.join(".trash");
                fs::create_dir_all(&dir)?;
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                fs::rename(target(&self.vault.root, &copy)?, dir.join(format!("{stamp}-{}", copy.replace('/', "__"))))?;
                self.refresh()?;
                self.tabs.retain(|t| *t != copy);
                if self.doc.name == copy || self.doc.name == orig {
                    self.load(orig.clone())?;
                }
                self.say(format!("Resolved conflict in {}", leaf(&orig)));
                Ok(())
            })();
            if let Err(e) = r {
                *result = Err(e);
            }
            self.compare = None;
        } else if !open {
            self.compare = None;
        }
    }

    fn vault_window(&mut self, ctx: &egui::Context) {
        let Some(mut path) = self.vault_win.take() else {
            return;
        };
        let mut keep = true;
        let mut go = false;
        egui::Window::new("Open another vault").collapsible(false).anchor(egui::Align2::CENTER_TOP, [0., 100.]).show(ctx, |ui| {
            ui.label("Folder to use as a vault (created if it doesn't exist)");
            let r = ui.add(egui::TextEdit::singleline(&mut path).desired_width(380.));
            go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let recent: Vec<PathBuf> = self.prefs.recent.iter().skip(1).cloned().collect();
            if !recent.is_empty() {
                ui.label(muted("Recent"));
                for v in recent {
                    if ui.selectable_label(path == v.display().to_string(), v.display().to_string()).clicked() {
                        path = v.display().to_string();
                    }
                }
            }
            ui.horizontal(|ui| {
                go |= primary(ui, "Open").clicked();
                if ui.button("Cancel").clicked() {
                    keep = false
                }
            });
        });
        if go {
            if self.busy() {
                self.fail("Wait for sync to finish, then switch vaults");
                self.vault_win = Some(path);
                return;
            }
            let p = PathBuf::from(path.trim());
            if let Err(e) = fs::create_dir_all(&p).map_err(Into::into).and_then(|_| self.switch_to(p, false)) {
                self.fail(e);
            }
        } else if keep {
            self.vault_win = Some(path)
        }
    }

    fn tour_window(&mut self, ctx: &egui::Context, result: &mut Result<()>) {
        let Some(step) = self.tour else {
            return;
        };
        if self.welcome || self.setup.is_some() {
            return;
        }
        const STEPS: [(&str, &str); 4] = [
            ("Switch views", "These icons open Files, Search, the Graph and today's Daily note. Settings and sync live at the bottom."),
            ("Your notes", "Search is always at the top. New note and New folder are right below it. Right-click a note or folder for more."),
            ("Write and link", "Click a note's title to rename it. Type [[ to link another note. Notes save on their own."),
            ("Status bar", "Click the vault name to switch vaults. The right side shows whether sync is on."),
        ];
        let p = pal();
        let target = self.tour_rects[step].or(self.tour_rects[0]);
        if let Some(r) = target {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tour-spot")));
            painter.rect_stroke(r.shrink(2.), 8., egui::Stroke::new(2.5_f32, p.accent), egui::StrokeKind::Inside);
        }
        let screen = ctx.screen_rect();
        let pos = match (step, target) {
            (0, Some(r)) | (1, Some(r)) => Pos2::new(r.right() + 12., r.top() + 60.),
            // Below the title, or inside the highlight when it fills the editor.
            (2, Some(r)) => Pos2::new((r.left() + 20.).min(screen.right() - 340.), if r.bottom() + 180. > screen.bottom() { r.top() + 20. } else { r.bottom() + 12. }),
            (3, Some(r)) => Pos2::new(r.left() + 20., r.top() - 150.),
            _ => Pos2::new(60., 60.),
        };
        let mut next = Some(step);
        egui::Window::new(format!("tour{step}")).title_bar(false).resizable(false).fixed_pos(pos).fixed_size([300., 0.]).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.label(muted(format!("{} of 4", step + 1)));
            ui.label(egui::RichText::new(STEPS[step].0).strong().size(16.));
            ui.label(STEPS[step].1);
            ui.add_space(6.);
            ui.horizontal(|ui| {
                if step == 3 {
                    let label = if self.notes.is_empty() { "Create my first note" } else { "Done" };
                    if primary(ui, label).clicked() {
                        next = None;
                        if self.notes.is_empty() {
                            if let Err(e) = self.new_note("") {
                                *result = Err(e);
                            }
                        }
                    }
                } else if primary(ui, "Next").clicked() {
                    next = Some(step + 1);
                }
                if step > 0 && ui.button("Back").clicked() {
                    next = Some(step - 1);
                }
                if ui.add(egui::Button::new(muted("Skip tour")).frame(false)).clicked() {
                    next = None;
                }
            });
        });
        self.tour = next;
    }
}

// ---------------------------------------------------------------- vaults

/// Where new and downloaded vaults go by default: `Documents/Emerald Vaults/<name>`.
fn vaults_home() -> PathBuf {
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default().join("Emerald Vaults")
}
fn pick_folder(start: &str) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new();
    let start = Path::new(start.trim());
    if let Some(dir) = start.ancestors().find(|a| a.is_dir()) {
        d = d.set_directory(dir);
    }
    d.pick_folder()
}
fn is_empty_dir(p: &Path) -> bool {
    match fs::read_dir(p) {
        Ok(mut d) => d.next().is_none(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}
fn human_size(b: u64) -> String {
    match b {
        0..=1023 => format!("{b} B"),
        1024..=1_048_575 => format!("{:.0} KB", b as f64 / 1024.),
        _ => format!("{:.1} MB", b as f64 / 1_048_576.),
    }
}
/// Check that every file in a folder can be opened and synced, before importing it.
fn scan_folder(root: &Path) -> Result<Scan> {
    ensure!(root.is_dir(), "Choose a folder that exists on this computer");
    let mut s = Scan { files: 0, notes: 0, bytes: 0, problems: Vec::new() };
    let entries = walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'));
    for entry in entries {
        let e = match entry {
            Ok(e) => e,
            Err(err) => {
                s.problems.push(err.to_string());
                continue;
            }
        };
        let rel = e.path().strip_prefix(root).unwrap_or(e.path());
        let Some(n) = rel.to_str().map(|n| n.replace(std::path::MAIN_SEPARATOR, "/")) else {
            s.problems.push(format!("{}: the name isn't valid Unicode", rel.display()));
            continue;
        };
        if e.path_is_symlink() {
            s.problems.push(format!("{n}: symbolic links can't be synced"));
            continue;
        }
        if !e.file_type().is_file() {
            continue;
        }
        if target(root, &n).is_err() {
            s.problems.push(format!("{n}: rename it without < > : \" \\ | ? * and without a dot or space at the end"));
            continue;
        }
        let len = e.metadata().map(|m| m.len()).unwrap_or(0);
        if len > 64 * 1024 * 1024 {
            s.problems.push(format!("{n}: larger than the 64 MB sync limit"));
            continue;
        }
        if n.ends_with(".md") {
            if fs::read_to_string(e.path()).is_err() {
                s.problems.push(format!("{n}: not UTF-8 text"));
                continue;
            }
            s.notes += 1;
        }
        s.files += 1;
        s.bytes += len;
    }
    Ok(s)
}

impl Notebook {
    /// Open another vault in place of this one.
    fn switch_to(&mut self, p: PathBuf, sync_now: bool) -> Result<()> {
        ensure!(!self.busy(), "Wait for sync to finish, then switch vaults");
        self.save()?;
        let mut nb = Notebook::new(p)?;
        nb.log = std::mem::take(&mut self.log);
        nb.show_log = self.show_log;
        if sync_now && nb.sync_state != SyncState::Off {
            nb.last_auto_sync = Instant::now() - Duration::from_secs(60);
        }
        nb.say("Vault opened");
        *self = nb;
        Ok(())
    }
    fn start_setup(&mut self, kind: SetupKind) {
        let from_env = sync::current(&self.vault).is_some_and(|c| c.2);
        let has_server = sync::connection(&self.vault).is_some() && !from_env;
        let mut st = VaultSetup {
            kind,
            folder: String::new(),
            name: String::new(),
            name_edited: false,
            folder_edited: false,
            sync: kind != SetupKind::New || has_server,
            same_server: has_server,
            server: String::new(),
            token: String::new(),
            remote: None,
            list_rx: None,
            scan: None,
            msg: None,
            job: None,
            pending: None,
            from_welcome: std::mem::take(&mut self.welcome),
        };
        if kind != SetupKind::Import {
            st.folder = vaults_home().display().to_string();
        }
        if kind == SetupKind::Server && has_server {
            self.list_remote(&mut st);
        }
        self.settings = None;
        self.setup = Some(st);
    }
    /// The connection a setup would use, without its vault name.
    fn setup_connection(&self, st: &VaultSetup) -> Result<sync::Connection> {
        if st.same_server {
            let mut c = sync::connection(&self.vault).ok_or_else(|| anyhow::anyhow!("This vault isn't connected to a server"))?;
            c.vault = None;
            Ok(c)
        } else {
            Ok(sync::Connection { server: st.server.trim().to_owned(), token: st.token.trim().to_owned(), vault: None })
        }
    }
    fn list_remote(&self, st: &mut VaultSetup) {
        match self.setup_connection(st) {
            Ok(c) => {
                let (tx, rx) = mpsc::channel();
                st.list_rx = Some(rx);
                st.msg = None;
                std::thread::spawn(move || {
                    let _ = tx.send(sync::vaults(&c.server, &c.token));
                });
            }
            Err(e) => st.msg = Some(friendly(&e)),
        }
    }
    /// Check the form and start the server part of a setup in the background.
    fn begin_setup(&self, st: &mut VaultSetup, ctx: &egui::Context) -> Result<()> {
        let folder = PathBuf::from(st.folder.trim());
        ensure!(!st.folder.trim().is_empty(), "Choose a folder");
        ensure!(st.kind == SetupKind::Import || !st.name.trim().is_empty(), "Enter a name for the vault");
        match st.kind {
            SetupKind::Import => {
                let scan = scan_folder(&folder)?;
                let bad = !scan.problems.is_empty();
                st.scan = Some((st.folder.trim().to_owned(), scan));
                ensure!(!bad, "Fix the files listed above, then import again");
            }
            SetupKind::New | SetupKind::Server => {
                ensure!(is_empty_dir(&folder), "That folder already has files in it. Choose a new or empty folder");
            }
        }
        let mut c = if st.sync { Some(self.setup_connection(st)?) } else { None };
        if let Some(c) = &mut c {
            c.vault = Some(emerald::vault_name(&st.name)?.to_owned());
        }
        let (tx, rx) = mpsc::channel();
        let kind = st.kind;
        let job = c.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = (|| -> Result<()> {
                let Some(c) = job else { return Ok(()) };
                let name = c.vault.clone().unwrap_or_default();
                let list = sync::vaults(&c.server, &c.token)?;
                let exists = list.iter().any(|n| n.eq_ignore_ascii_case(&name));
                if kind == SetupKind::Server {
                    ensure!(exists, "The server has no vault named \"{name}\"");
                } else {
                    ensure!(!exists, "The server already has a vault named \"{name}\". Choose another name, or use Open from server to download it");
                    sync::create_vault(&c.server, &c.token, &name)?;
                }
                Ok(())
            })();
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        st.job = Some(rx);
        st.pending = Some((folder, c.unwrap_or(sync::Connection { server: String::new(), token: String::new(), vault: None })));
        Ok(())
    }
    /// The server is ready: save the connection and open the vault.
    fn finish_setup(&mut self, st: VaultSetup) -> Result<()> {
        let (folder, c) = st.pending.ok_or_else(|| anyhow::anyhow!("Nothing to set up"))?;
        fs::create_dir_all(&folder)?;
        if st.sync {
            sync::configure(&Vault::new(folder.clone(), String::new())?, c)?;
        }
        let old = self.vault.root.clone();
        let old_empty = index(&old).map(|i| i.is_empty()).unwrap_or(false);
        self.switch_to(folder, true)?;
        if st.from_welcome {
            // The empty default vault made on first launch is no longer needed.
            if old_empty && old != self.vault.root {
                let _ = fs::remove_file(old.join(".emerald-tour"));
                if fs::remove_dir(&old).is_ok() {
                    self.prefs.recent.retain(|p| p != &old);
                    save_json(prefs_file(), &self.prefs);
                }
            }
            let _ = fs::write(self.vault.root.join(".emerald-tour"), "");
            self.tour = Some(0);
        }
        self.say(match st.kind {
            SetupKind::Import if st.sync => "Folder imported. Uploading it to the server…",
            SetupKind::Import => "Folder opened as a vault",
            SetupKind::Server => "Downloading the vault from the server…",
            SetupKind::New => "Vault created",
        });
        Ok(())
    }

    fn vaults_section(&mut self, ui: &mut egui::Ui) {
        let p = pal();
        ui.label(egui::RichText::new("This vault").strong());
        ui.label(muted(self.vault.root.display().to_string()));
        match sync::current(&self.vault) {
            Some((server, v, _)) => ui.label(muted(format!("Synced with {} on {server}", v.map(|v| format!("\"{v}\"")).unwrap_or_else(|| "the main vault".into())))),
            None => ui.label(muted("Not synced")),
        };
        ui.horizontal(|ui| {
            if ui.button("Reveal in file manager").clicked() {
                reveal(&self.vault.root);
            }
            if ui.button("Trash…").clicked() {
                self.trash = true;
            }
        });
        ui.add_space(10.);
        ui.label(egui::RichText::new("Add a vault").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Import a folder…").on_hover_text("Use a folder of notes from this computer and upload it to your server").clicked() {
                self.start_setup(SetupKind::Import);
            }
            if ui.button("Open from server…").on_hover_text("Download a vault that is already on your server").clicked() {
                self.start_setup(SetupKind::Server);
            }
            if ui.button("New vault…").clicked() {
                self.start_setup(SetupKind::New);
            }
            if ui.button("Open a folder…").clicked() {
                self.vault_win = Some(self.vault.root.display().to_string());
            }
        });
        ui.add_space(10.);
        ui.label(egui::RichText::new("Your vaults").strong());
        let mut remove = None;
        egui::ScrollArea::vertical().max_height(260.).show(ui, |ui| {
            for v in self.prefs.recent.clone() {
                let here = v == self.vault.root;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let name = v.file_name().map_or_else(|| v.display().to_string(), |n| n.to_string_lossy().into_owned());
                        ui.label(egui::RichText::new(name).color(if here { p.accent } else { p.text }));
                        ui.label(egui::RichText::new(v.display().to_string()).small().color(p.muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if here {
                            ui.label(muted("open"));
                        } else {
                            if ui.small_button("Remove").on_hover_text("Remove from this list. The folder and its notes stay where they are.").clicked() {
                                remove = Some(v.clone());
                            }
                            if ui.small_button("Open").clicked() {
                                self.vault_win = Some(v.display().to_string());
                            }
                        }
                    });
                });
            }
        });
        if let Some(v) = remove {
            self.prefs.recent.retain(|p| p != &v);
            save_json(prefs_file(), &self.prefs);
        }
    }

    fn welcome_window(&mut self, ctx: &egui::Context) {
        if !self.welcome {
            return;
        }
        let mut pick = None;
        egui::Window::new("Welcome to Emerald").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0., 0.]).fixed_size([420., 0.]).show(ctx, |ui| {
            ui.label("How do you want to start?");
            ui.add_space(8.);
            let mut choice = |ui: &mut egui::Ui, title: &str, text: &str, k: Option<SetupKind>, main: bool| {
                let r = if main { primary(ui, title) } else { ui.button(title) };
                if r.clicked() {
                    pick = Some(k);
                }
                ui.label(muted(text));
                ui.add_space(6.);
            };
            choice(ui, "Start with an empty vault", &format!("Notes go in {}.", self.vault.root.display()), None, true);
            choice(ui, "Import an existing folder…", "Use a folder of Markdown notes from this computer. Emerald can upload it to your sync server.", Some(SetupKind::Import), false);
            choice(ui, "Open a vault from my server…", "Download a vault you already keep on an Emerald Server.", Some(SetupKind::Server), false);
            ui.label(egui::RichText::new("You can add more vaults later in Settings › Vaults.").small().color(pal().muted));
        });
        match pick {
            Some(None) => {
                self.welcome = false;
                self.tour = Some(0);
            }
            Some(Some(k)) => self.start_setup(k),
            None => {}
        }
    }

    fn setup_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.setup.take() else {
            return;
        };
        let p = pal();
        // Background results.
        if let Some(rx) = &st.list_rx {
            if let Ok(r) = rx.try_recv() {
                st.list_rx = None;
                match r {
                    Ok(list) => st.remote = Some(list),
                    Err(e) => st.msg = Some(friendly(&e)),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if let Some(rx) = &st.job {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    st.job = None;
                    let back = st.from_welcome;
                    if let Err(e) = self.finish_setup(st) {
                        self.fail(&e);
                        self.welcome = back;
                    }
                    return;
                }
                Ok(Err(e)) => {
                    st.job = None;
                    st.pending = None;
                    st.msg = Some(friendly(&e));
                }
                Err(_) => ctx.request_repaint_after(Duration::from_millis(100)),
            }
        }
        let working = st.job.is_some();
        let from_env = sync::current(&self.vault).is_some_and(|c| c.2);
        let has_server = sync::connection(&self.vault).is_some() && !from_env;
        let current_server = sync::current(&self.vault).map(|c| c.0).unwrap_or_default();
        let title = match st.kind {
            SetupKind::New => "New vault",
            SetupKind::Import => "Import a folder",
            SetupKind::Server => "Open a vault from the server",
        };
        let mut keep = true;
        let mut go = false;
        let mut refresh = false;
        egui::Window::new(title).collapsible(false).resizable(false).default_width(460.).anchor(egui::Align2::CENTER_TOP, [0., 70.]).show(ctx, |ui| {
            ui.add_enabled_ui(!working, |ui| {
                // ---- local folder
                match st.kind {
                    SetupKind::Import => {
                        ui.label("Folder on this computer");
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::TextEdit::singleline(&mut st.folder).hint_text(muted("/path/to/notes")).desired_width(330.));
                            let mut chosen = r.lost_focus();
                            if ui.button("Browse…").clicked() {
                                if let Some(f) = pick_folder(&st.folder) {
                                    st.folder = f.display().to_string();
                                    chosen = true;
                                }
                            }
                            if chosen && !st.folder.trim().is_empty() {
                                st.msg = None;
                                match scan_folder(Path::new(st.folder.trim())) {
                                    Ok(scan) => st.scan = Some((st.folder.trim().to_owned(), scan)),
                                    Err(e) => {
                                        st.scan = None;
                                        st.msg = Some(e.to_string());
                                    }
                                }
                            }
                        });
                        if !st.name_edited {
                            st.name = Path::new(st.folder.trim()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        }
                        if let Some((_, scan)) = st.scan.as_ref().filter(|(f, _)| f == st.folder.trim()) {
                            ui.label(muted(format!("{}, {} in all ({}).", plural(scan.notes, "note"), plural(scan.files, "file"), human_size(scan.bytes))));
                            if !scan.problems.is_empty() {
                                ui.label(egui::RichText::new(format!("{} can't be synced:", plural(scan.problems.len(), "file"))).color(p.red));
                                egui::ScrollArea::vertical().max_height(120.).show(ui, |ui| {
                                    for m in scan.problems.iter().take(50) {
                                        ui.label(egui::RichText::new(m.as_str()).small().color(p.red));
                                    }
                                });
                            }
                        }
                        ui.label(egui::RichText::new("Emerald works on the folder where it is. Nothing is moved or copied on this computer.").small().color(p.muted));
                    }
                    SetupKind::New | SetupKind::Server => {}
                }
                // ---- server
                if st.kind == SetupKind::New || st.kind == SetupKind::Import {
                    ui.add_space(6.);
                    let label = if st.kind == SetupKind::Import { "Upload to my sync server and keep it in sync" } else { "Sync with my server" };
                    ui.add_enabled(!from_env, egui::Checkbox::new(&mut st.sync, label));
                }
                if from_env {
                    ui.label(egui::RichText::new("EMERALD_SERVER is set, so every vault syncs with the server and vault it names.").small().color(p.muted));
                    st.sync = st.kind == SetupKind::Server;
                }
                if st.sync {
                    ui.add_space(4.);
                    if has_server {
                        ui.horizontal(|ui| {
                            refresh |= ui.radio_value(&mut st.same_server, true, format!("Same server as this vault ({current_server})")).changed();
                        });
                        refresh |= ui.radio_value(&mut st.same_server, false, "Another server").changed();
                    }
                    if !st.same_server {
                        ui.label("Server address");
                        ui.add(egui::TextEdit::singleline(&mut st.server).hint_text(muted("https://notes.example.org")).desired_width(360.));
                        ui.label("Server token");
                        ui.add(egui::TextEdit::singleline(&mut st.token).password(true).desired_width(360.));
                    }
                    ui.add_space(4.);
                    ui.label(if st.kind == SetupKind::Server { "Vault on the server" } else { "Name on the server" });
                    let r = ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(300.));
                    if r.changed() {
                        st.name_edited = true;
                    }
                    ui.horizontal_wrapped(|ui| {
                        if st.list_rx.is_some() {
                            ui.spinner();
                        } else if ui.small_button(if st.remote.is_some() { "Refresh list" } else { "List server vaults" }).clicked() {
                            refresh = true;
                        }
                        if let Some(list) = &st.remote {
                            if list.is_empty() {
                                ui.label(egui::RichText::new("No named vaults on the server yet.").small().color(p.muted));
                            }
                            for n in list {
                                let taken = st.kind != SetupKind::Server;
                                let txt = egui::RichText::new(n.as_str()).color(if taken { p.muted } else { p.text });
                                if ui.selectable_label(st.name.trim() == n, txt).on_hover_text(if taken { "Already on the server" } else { "Download this vault" }).clicked() && !taken {
                                    st.name = n.clone();
                                    st.name_edited = true;
                                }
                            }
                        }
                    });
                    if st.kind != SetupKind::Server {
                        ui.label(egui::RichText::new("Stored on the server under Vaults/<name>/, next to your other vaults.").small().color(p.muted));
                    }
                }
                // ---- local folder for new / downloaded vaults
                if st.kind != SetupKind::Import {
                    if st.kind == SetupKind::New && !st.sync {
                        ui.add_space(4.);
                        ui.label("Name");
                        if ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(300.)).changed() {
                            st.name_edited = true;
                        }
                    }
                    if !st.folder_edited {
                        st.folder = vaults_home().join(st.name.trim()).display().to_string();
                    }
                    ui.add_space(4.);
                    ui.label("Folder on this computer");
                    ui.horizontal(|ui| {
                        if ui.add(egui::TextEdit::singleline(&mut st.folder).desired_width(330.)).changed() {
                            st.folder_edited = true;
                        }
                        if ui.button("Browse…").on_hover_text("Choose where the vault folder goes").clicked() {
                            if let Some(f) = pick_folder(&st.folder) {
                                st.folder = if is_empty_dir(&f) { f } else { f.join(st.name.trim()) }.display().to_string();
                                st.folder_edited = true;
                            }
                        }
                    });
                    ui.label(egui::RichText::new("A new or empty folder.").small().color(p.muted));
                }
            });
            if let Some(m) = &st.msg {
                ui.add_space(4.);
                ui.label(egui::RichText::new(m.as_str()).color(p.red));
            }
            ui.add_space(6.);
            ui.horizontal(|ui| {
                let label = match st.kind {
                    SetupKind::Import => "Import",
                    SetupKind::New => "Create vault",
                    SetupKind::Server => "Download",
                };
                if working {
                    ui.spinner();
                    ui.label(muted("Checking the server…"));
                } else {
                    go = primary(ui, label).clicked();
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                }
            });
        });
        if refresh {
            st.remote = None;
            self.list_remote(&mut st);
        }
        if go {
            st.msg = None;
            if let Err(e) = self.begin_setup(&mut st, ctx) {
                st.msg = Some(friendly(&e));
            }
        }
        if keep {
            self.setup = Some(st);
        } else if st.from_welcome {
            self.welcome = true;
        }
    }
}

/// Plain-language versions of common connection errors.
fn friendly(e: &anyhow::Error) -> String {
    let m = format!("{e:#}");
    if m.contains("Use HTTPS") {
        "Other computers must be reached over https://. For a server on this computer use http://127.0.0.1:7878. See SERVER_SETUP.md for HTTPS.".into()
    } else if m.contains("Enter the server token") {
        "The token must be at least 32 characters. Copy the EMERALD_TOKEN value the server was started with.".into()
    } else if m.contains("401") {
        "The server rejected the token. Check it matches EMERALD_TOKEN on the server.".into()
    } else if m.contains("relative URL") || m.contains("invalid") && m.contains("URL") {
        "That doesn't look like an address. Start with https:// or http://127.0.0.1:7878.".into()
    } else if m.contains("connect") || m.contains("timed out") || m.contains("dns") {
        format!("Couldn't reach the server. Is emerald-server running at that address? ({m})")
    } else {
        m
    }
}
