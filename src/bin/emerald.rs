#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use anyhow::{ensure, Result};
use clap::Parser;
use eframe::egui::{self, Color32, Pos2, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use emerald::{atomic, digest, index, sync, target, Vault};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::mpsc, time::{Duration, Instant}};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    vault: Option<PathBuf>,
}

fn main() -> eframe::Result<()> {
    let path = Args::parse().vault.unwrap_or_else(|| {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default()
            .join("Emerald")
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1060., 720.]),
        ..Default::default()
    };
    eframe::run_native(
        "Emerald",
        options,
        Box::new(move |cc| {
            theme(&cc.egui_ctx);
            Ok(Box::new(Notebook::new(path)?))
        }),
    )
}
const BG: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
const SIDE: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
const RIBBON: Color32 = Color32::from_rgb(0x16, 0x16, 0x16);
const LINE: Color32 = Color32::from_rgb(0x36, 0x36, 0x36);
const TEXT: Color32 = Color32::from_rgb(0xda, 0xda, 0xda);
const MUTED: Color32 = Color32::from_rgb(0x99, 0x99, 0x99);
const HOVER: Color32 = Color32::from_rgb(0x2f, 0x2f, 0x2f);
const SEL: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
const ACCENT: Color32 = Color32::from_rgb(0x2f, 0xb5, 0x83);
fn theme(ctx: &egui::Context) {
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
    fonts.families.insert(egui::FontFamily::Proportional, vec!["inter".into()]);
    fonts.families.insert(egui::FontFamily::Name("bold".into()), vec!["inter-bold".into()]);
    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().insert(0, "inter".into());
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SIDE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = SIDE;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x27, 0xa7, 0x7a, 0x55);
    v.selection.stroke = egui::Stroke::new(1., ACCENT);
    v.hyperlink_color = Color32::from_rgb(0x46, 0xd1, 0x9c);
    v.window_stroke = egui::Stroke::new(1., LINE);
    v.window_corner_radius = egui::CornerRadius::same(10);
    let r = egui::CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = egui::Stroke::new(1., LINE);
    w.inactive.weak_bg_fill = BG;
    w.inactive.bg_fill = BG;
    w.inactive.bg_stroke = egui::Stroke::new(1., LINE);
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_fill = HOVER;
    w.hovered.bg_stroke = egui::Stroke::new(1., LINE);
    w.active.weak_bg_fill = SEL;
    w.active.bg_fill = SEL;
    w.active.bg_stroke = egui::Stroke::new(1., LINE);
    for x in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        x.corner_radius = r;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(6., 3.);
        s.spacing.button_padding = Vec2::new(10., 5.);
        s.spacing.scroll = egui::style::ScrollStyle::solid();
        for (_, f) in s.text_styles.iter_mut() {
            f.size = (f.size * 0.95).max(12.);
        }
    });
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
    let (rect, r) = ui.allocate_exact_size(Vec2::new(30., 28.), egui::Sense::click());
    if on || r.hovered() {
        ui.painter().rect_filled(rect, 5., if on { SEL } else { HOVER });
    }
    let c = if on || r.hovered() { TEXT } else { MUTED };
    egui::Image::new(svg(name))
        .tint(c)
        .fit_to_exact_size(Vec2::splat(18.))
        .paint_at(ui, egui::Rect::from_center_size(rect.center(), Vec2::splat(18.)));
    r.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}
#[derive(Default, PartialEq)]
enum View {
    #[default]
    Edit,
    Preview,
    Graph,
}

#[derive(Default)]
struct Document {
    name: String,
    text: String,
    saved: String,
    dirty: bool,
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
            None => self
                .files
                .push((full.to_owned(), rest.trim_end_matches(".md").to_owned())),
        }
    }
    fn show(&self, ui: &mut egui::Ui, cur: &str, open: &mut Option<String>, force: bool, desc: bool, collapse: bool) {
        let mut dirs: Vec<_> = self.dirs.iter().collect();
        let mut files: Vec<_> = self.files.iter().collect();
        if desc {
            dirs.reverse();
            files.reverse();
        }
        for (d, t) in dirs {
            let state = if force { Some(true) } else if collapse { Some(false) } else { None };
            egui::CollapsingHeader::new(d)
                .default_open(true)
                .open(state)
                .show(ui, |ui| t.show(ui, cur, open, force, desc, collapse));
        }
        for (full, name) in files {
            if ui.selectable_label(full == cur, name).clicked() {
                *open = Some(full.clone())
            }
        }
    }
}

struct Notebook {
    sidebar: bool,
    searching: bool,
    sort_desc: bool,
    collapse: Option<bool>,
    focus_search: bool,
    vault: Vault,
    notes: BTreeMap<String, String>,
    doc: Document,
    tabs: Vec<String>,
    view: View,
    query: String,
    new_name: String,
    confirm_delete: bool,
    title: String,
    focus_title: bool,
    focus_editor: bool,
    last_edit: Option<Instant>,
    status_err: bool,
    sync_ok: bool,
    undo: Option<(String, PathBuf)>,
    palette: bool,
    pal_query: String,
    pick: Option<(usize, usize, String)>,
    test_rx: Option<mpsc::Receiver<Result<String>>>,
    test_msg: Option<(bool, String)>,
    gpos: BTreeMap<String, Vec2>,
    gzoom: f32,
    gpan: Vec2,
    settings: bool,
    server: String,
    secret: String,
    status: String,
    cache: CommonMarkCache,
    pending: Option<mpsc::Receiver<Result<String>>>,
}
fn links(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split("[[")
        .skip(1)
        .filter_map(|s| s.split_once("]]"))
        .map(|(s, _)| note_name(s))
        .filter(|s| !s.contains('\n'))
}
fn note_name(name: &str) -> String {
    let name = name.trim();
    if name.ends_with(".md") {
        name.to_owned()
    } else {
        format!("{name}.md")
    }
}
impl Notebook {
    fn new(path: PathBuf) -> Result<Self> {
        let mut app = Self {
            sidebar: true,
            searching: false,
            sort_desc: false,
            collapse: None,
            focus_search: false,
            vault: Vault::new(path, String::new())?,
            notes: Default::default(),
            doc: Default::default(),
            tabs: Vec::new(),
            view: Default::default(),
            query: String::new(),
            new_name: String::new(),
            confirm_delete: false,
            title: String::new(),
            focus_title: false,
            focus_editor: false,
            last_edit: None,
            status_err: false,
            sync_ok: false,
            undo: None,
            palette: false,
            pal_query: String::new(),
            pick: None,
            test_rx: None,
            test_msg: None,
            gpos: BTreeMap::new(),
            gzoom: 1.,
            gpan: Vec2::ZERO,
            settings: false,
            server: String::new(),
            secret: String::new(),
            status: String::new(),
            cache: Default::default(),
            pending: None,
        };
        app.refresh()?;
        app.sync_ok = sync::configured(&app.vault);
        Ok(app)
    }
    fn refresh(&mut self) -> Result<()> {
        let mut notes = BTreeMap::new();
        for n in index(&self.vault.root)?
            .into_keys()
            .filter(|n| n.ends_with(".md"))
        {
            notes.insert(
                n.clone(),
                fs::read_to_string(target(&self.vault.root, &n)?)?,
            );
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
        self.doc = Document {
            name: n,
            text,
            saved,
            dirty: false,
        };
        self.confirm_delete = false;
        self.title = self.doc.name.trim_end_matches(".md").to_owned();
        self.focus_editor = true;
        self.last_edit = None;
        self.view = View::Edit;
        Ok(())
    }
    fn fresh(&self) -> String {
        let mut i = 0;
        loop {
            let n = if i == 0 { "Untitled.md".to_owned() } else { format!("Untitled {i}.md") };
            if !self.notes.contains_key(&n) && !self.tabs.contains(&n) {
                return n;
            }
            i += 1;
        }
    }
    fn rename_to(&mut self, to: String) -> Result<()> {
        let to = note_name(&to);
        if to == self.doc.name {
            return Ok(());
        }
        ensure!(!self.notes.contains_key(&to), "A note with that name exists");
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
        self.status = "Renamed".into();
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
        ensure!(
            hash == self.doc.saved,
            "File changed on disk. Copy your edits before reloading."
        );
        Ok(path)
    }
    fn save(&mut self) -> Result<()> {
        if self.doc.name.is_empty()
            || (!self.doc.dirty && !self.doc.saved.is_empty())
            || (self.doc.text.is_empty() && self.doc.saved.is_empty())
        {
            return Ok(());
        }
        atomic(&self.checked_path()?, self.doc.text.as_bytes())?;
        self.notes
            .insert(self.doc.name.clone(), self.doc.text.clone());
        self.doc.saved = digest(self.doc.text.as_bytes());
        self.doc.dirty = false;
        self.status = "Saved".into();
        self.status_err = false;
        Ok(())
    }
    fn sync(&mut self, ctx: &egui::Context) -> Result<()> {
        self.save()?;
        let vault = self.vault.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.status = "Syncing…".into();
        std::thread::spawn(move || {
            let _ = tx.send(sync::run(vault));
            ctx.request_repaint();
        });
        Ok(())
    }
    fn poll(&mut self) -> Result<()> {
        let Some(rx) = &self.pending else {
            return Ok(());
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(e) => Err(e.into()),
        };
        self.pending = None;
        self.refresh()?;
        if self.notes.contains_key(&self.doc.name) {
            self.load(self.doc.name.clone())?
        } else {
            self.doc = Document::default()
        }
        let s = result?;
        self.status = s;
        self.status_err = false;
        Ok(())
    }
    fn graph(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let (response, p) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let r = response.rect;
        let names: Vec<String> = self.notes.keys().cloned().collect();
        let n = names.len();
        self.gpos.retain(|k, _| self.notes.contains_key(k));
        for (i, k) in names.iter().enumerate() {
            self.gpos.entry(k.clone()).or_insert_with(|| {
                let a = i as f32 * std::f32::consts::TAU / n.max(1) as f32;
                Vec2::angled(a) * 160.
            });
        }
        let edges: Vec<(String, String)> = self
            .notes
            .iter()
            .flat_map(|(a, t)| links(t).filter(|l| self.notes.contains_key(l) && l != a).map(|l| (a.clone(), l)).collect::<Vec<_>>())
            .collect();
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
        let hover = response.hover_pos();
        let hit = hover.and_then(|h| {
            names.iter().find(|k| to_screen(self.gpos[*k], self.gzoom, self.gpan).distance(h) < 14.).cloned()
        });
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
        for (k, f) in force {
            if dragging.as_ref() != Some(&k) {
                let cur = self.gpos[&k];
                *self.gpos.get_mut(&k).unwrap() += (f - cur * 0.01) * 0.5;
            }
        }
        ui.ctx().request_repaint();
        let (z, pan) = (self.gzoom, self.gpan);
        let near: Vec<&String> = match &hit {
            Some(h) => edges.iter().filter_map(|(a, b)| if a == h { Some(b) } else if b == h { Some(a) } else { None }).collect(),
            None => Vec::new(),
        };
        for (a, b) in &edges {
            let on = hit.as_ref().is_some_and(|h| h == a || h == b);
            let c = if on { ACCENT } else { Color32::from_gray(0x55) };
            p.line_segment([to_screen(self.gpos[a], z, pan), to_screen(self.gpos[b], z, pan)], (if on { 1.8 } else { 1. }, c));
        }
        let mut clicked = None;
        for k in &names {
            let pos = to_screen(self.gpos[k], z, pan);
            let lit = hit.as_ref() == Some(k) || near.contains(&k);
            let dim = hit.is_some() && !lit;
            let base = if dim { Color32::from_gray(0x55) } else { ACCENT };
            p.circle_filled(pos, if hit.as_ref() == Some(k) { 8. } else { 6. } * z.sqrt(), base);
            if *k == self.doc.name {
                p.circle_stroke(pos, 10. * z.sqrt(), (1.5, TEXT));
            }
            let mut label = k.rsplit('/').next().unwrap_or(k).trim_end_matches(".md").to_owned();
            if label.chars().count() > 24 {
                label = label.chars().take(23).collect::<String>() + "\u{2026}";
            }
            p.text(
                pos + Vec2::new(0., 10. * z.sqrt()),
                egui::Align2::CENTER_TOP,
                label,
                egui::FontId::proportional(13.),
                if dim { Color32::from_gray(0x66) } else { TEXT },
            );
        }
        if let Some(h) = &hit {
            response.clone().on_hover_text(h.as_str());
            if response.clicked() {
                clicked = Some(h.clone());
            }
        }
        clicked
    }
}
impl eframe::App for Notebook {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Err(e) = self.poll() {
            self.status = e.to_string();
            self.status_err = true;
        }
        let busy = self.pending.is_some();
        if ctx.input(|i| i.viewport().close_requested()) {
            if busy {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose)
            } else if let Err(e) = self.save() {
                self.status = e.to_string();
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose)
            }
        }
        if self.doc.dirty && !busy {
            if let Some(t) = self.last_edit {
                if t.elapsed() >= Duration::from_millis(1000) {
                    self.last_edit = None;
                    if let Err(e) = self.save() {
                        self.status = e.to_string();
                        self.status_err = true;
                    }
                } else {
                    ctx.request_repaint_after(Duration::from_millis(300));
                }
            }
        }
        let backlinks: Vec<String> = if self.doc.name.is_empty() {
            Vec::new()
        } else {
            self.notes
                .iter()
                .filter(|(n, t)| **n != self.doc.name && links(t).any(|p| p == self.doc.name))
                .map(|(n, _)| n.clone())
                .collect()
        };
        let mut open = None;
        let mut result = Ok(());
        let (k_new, k_find, k_graph, k_close) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::N),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)
                    || i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::G),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::W),
            )
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            self.palette = !self.palette;
            self.pal_query.clear();
        }
        if !busy {
            if k_new {
                open = Some(self.fresh())
            }
            if k_find {
                self.sidebar = true;
                self.searching = true;
                self.focus_search = true
            }
            if k_graph {
                self.view = if self.view == View::Graph { View::Edit } else { View::Graph }
            }
            if k_close && !self.doc.name.is_empty() {
                let tab = self.doc.name.clone();
                self.tabs.retain(|t| t != &tab);
                open = self.tabs.last().cloned();
                if open.is_none() {
                    result = self.save();
                    self.doc = Document::default();
                }
            }
        }
        egui::SidePanel::left("ribbon")
            .exact_width(44.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(SIDE)
                    .stroke(egui::Stroke::new(1., LINE))
                    .inner_margin(egui::Margin::symmetric(6, 8)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.;
                    if ib(ui, "side", "Toggle sidebar", false).clicked() {
                        self.sidebar = !self.sidebar
                    }
                    let graph = self.view == View::Graph;
                    if ib(ui, "files", "Files", self.sidebar && !self.searching && !graph).clicked() {
                        self.sidebar = true;
                        self.searching = false;
                        self.view = View::Edit;
                    }
                    if ib(ui, "search", "Search (Ctrl+O)", self.sidebar && self.searching && !graph).clicked() {
                        self.sidebar = true;
                        self.searching = true;
                        self.focus_search = true;
                        self.view = View::Edit;
                    }
                    if ib(ui, "graph", "Open graph view", self.view == View::Graph).clicked() {
                        self.view = View::Graph
                    }
                    if ib(ui, "today", "Open today's daily note", false).clicked() {
                        open = Some(format!("Daily/{}.md", chrono::Local::now().date_naive()))
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        if ib(ui, "set", "Sync settings", self.settings).clicked() {
                            self.settings = true
                        }
                        if ib(ui, "sync", "Sync", false).clicked() {
                            result = self.sync(ctx)
                        }
                    });
                });
            });
        if self.sidebar {
            egui::SidePanel::left("vault")
                .default_width(260.)
                .width_range(180.0..=480.)
                .frame(
                    egui::Frame::new()
                        .fill(SIDE)
                        .stroke(egui::Stroke::new(1., LINE))
                        .inner_margin(egui::Margin::symmetric(8, 6)),
                )
                .show(ctx, |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 4. * 32.) / 2.);
                            if ib(ui, "new", "New note", false).clicked() {
                                open = Some(self.fresh())
                            }
                            if ib(ui, "folder", "New folder note", false).clicked() {
                                self.new_name = "Folder/".into()
                            }
                            if ib(ui, "sort", "Change sort order", false).clicked() {
                                self.sort_desc = !self.sort_desc
                            }
                            if ib(ui, "collapse", "Collapse all", false).clicked() {
                                self.collapse = Some(true)
                            }
                        });
                        if self.searching || !self.query.is_empty() {
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut self.query)
                                    .hint_text("Search...")
                                    .margin(egui::Margin::symmetric(9, 5))
                                    .desired_width(f32::INFINITY),
                            );
                            if std::mem::take(&mut self.focus_search) {
                                r.request_focus()
                            }
                            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.query.clear();
                                self.searching = false;
                                self.focus_editor = true;
                            } else if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                let q = self.query.to_lowercase();
                                if let Some(n) = self.notes.iter().find(|(n, t)| {
                                    !q.is_empty() && (n.to_lowercase().contains(&q) || t.to_lowercase().contains(&q))
                                }) {
                                    open = Some(n.0.clone())
                                }
                            }
                        }
                        if !self.new_name.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_name)
                                        .hint_text("Folder/Note name")
                                        .desired_width(150.),
                                );
                                if ui.button("Create").clicked()
                                    && !self.new_name.trim().ends_with('/')
                                {
                                    open = Some(note_name(&self.new_name));
                                    self.new_name.clear()
                                }
                            });
                        }
                        ui.add_space(4.);
                        let collapse_now = self.collapse.take().unwrap_or(false);
                        let q = self.query.to_lowercase();
                        let mut tree = Tree::default();
                        for (n, t) in &self.notes {
                            if q.is_empty()
                                || n.to_lowercase().contains(&q)
                                || t.to_lowercase().contains(&q)
                            {
                                tree.add(n, n);
                            }
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if q.is_empty() {
                                tree.show(ui, &self.doc.name, &mut open, false, self.sort_desc, collapse_now);
                                return;
                            }
                            for (n, t) in &self.notes {
                                let title_hit = n.to_lowercase().contains(&q);
                                let line = t.lines().find(|l| l.to_lowercase().contains(&q));
                                if !title_hit && line.is_none() {
                                    continue;
                                }
                                let r = ui.add_sized(
                                    [ui.available_width(), 20.],
                                    egui::Button::new(n.trim_end_matches(".md")).frame(*n == self.doc.name).wrap_mode(egui::TextWrapMode::Truncate),
                                );
                                if r.clicked() {
                                    open = Some(n.clone())
                                }
                                if let Some(l) = line {
                                    let l = l.trim();
                                    let low = l.to_lowercase();
                                    let at = low.find(&q).unwrap_or(0);
                                    let start = l.char_indices().map(|c| c.0).filter(|i| *i + 20 >= at).next().unwrap_or(0);
                                    let start = (0..=start).rev().find(|i| l.is_char_boundary(*i)).unwrap_or(0);
                                    let mut job = egui::text::LayoutJob::default();
                                    let fmt = egui::TextFormat { font_id: egui::FontId::proportional(12.), color: MUTED, ..Default::default() };
                                    let hl = egui::TextFormat { color: ACCENT, ..fmt.clone() };
                                    let seg = &l[start..];
                                    let low = seg.to_lowercase();
                                    if low.len() == seg.len() {
                                        if let Some(p) = low.find(&q) {
                                            let end = p + q.len();
                                            job.append(&seg[..p], 12., fmt.clone());
                                            job.append(&seg[p..end], 0., hl);
                                            job.append(&seg[end..], 0., fmt);
                                        } else {
                                            job.append(seg, 12., fmt);
                                        }
                                    } else {
                                        job.append(seg, 12., fmt);
                                    }
                                    job.wrap.max_rows = 1;
                                    job.wrap.break_anywhere = true;
                                    ui.label(job);
                                }
                                ui.add_space(4.);
                            }
                        });
                    });
                });
        }
        if !backlinks.is_empty() {
            egui::SidePanel::right("backlinks")
                .default_width(220.)
                .frame(egui::Frame::new().fill(SIDE).inner_margin(10.))
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("BACKLINKS").small().strong().color(MUTED));
                    for n in &backlinks {
                        if ui.link(n.trim_end_matches(".md")).clicked() {
                            open = Some(n.clone())
                        }
                    }
                });
        }
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(RIBBON).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(self.vault.root.file_name().map_or(String::new(), |n| n.to_string_lossy().into()))
                            .family(egui::FontFamily::Name("bold".into()))
                            .size(13.),
                    );
                    ui.label(egui::RichText::new(format!("· {} notes", self.notes.len())).size(13.).color(MUTED));
                    let c = if self.status_err { Color32::from_rgb(0xe5, 0x53, 0x4b) } else { MUTED };
                    ui.label(egui::RichText::new(format!("· {}", self.status)).size(13.).color(c));
                    if self.undo.is_some() && ui.link("Undo").clicked() {
                        if let Some((name, from)) = self.undo.take() {
                            let r = target(&self.vault.root, &name).and_then(|to| {
                                if let Some(p) = to.parent() {
                                    fs::create_dir_all(p)?
                                }
                                ensure!(!to.exists(), "A note with that name exists");
                                Ok(fs::rename(from, to)?)
                            });
                            match r {
                                Ok(()) => {
                                    result = self.refresh();
                                    open = Some(name);
                                }
                                Err(e) => result = Err(e),
                            }
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(24, 10)))
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                    if self.view == View::Graph {
                        ui.horizontal(|ui| {
                            ui.heading("Note graph");
                            if ui.button("Back to editor").clicked() {
                                self.view = View::Edit
                            }
                        });
                        if let Some(n) = self.graph(ui) {
                            open = Some(n)
                        }
                        return;
                    }
                    ui.horizontal(|ui| {
                        for tab in self.tabs.clone() {
                            let mut title = tab.rsplit('/').next().unwrap_or(&tab).trim_end_matches(".md").to_owned();
                            if self.doc.dirty && tab == self.doc.name {
                                title.push_str(" ●")
                            }
                            if ui.selectable_label(tab == self.doc.name, title).clicked() {
                                open = Some(tab.clone());
                            }
                            if ui.add(egui::Button::new(egui::RichText::new("×").color(MUTED)).frame(false)).clicked() {
                                self.tabs.retain(|t| t != &tab);
                                if tab == self.doc.name {
                                    open = self.tabs.last().cloned();
                                    if open.is_none() {
                                        self.doc = Document::default();
                                    }
                                }
                            }
                            ui.add_space(6.);
                        }
                        if ui.add(egui::Button::new(egui::RichText::new("+").size(16.).color(MUTED)).frame(false)).clicked() {
                            open = Some(self.fresh())
                        }
                    });
                    ui.separator();
                    if self.doc.name.is_empty() {
                        ui.add_space(ui.available_height() * 0.3);
                        ui.vertical_centered(|ui| {
                            let b = |ui: &mut egui::Ui, i: &str, t: &str, k: &str| {
                                let img = egui::Image::new(svg(i)).tint(TEXT).fit_to_exact_size(Vec2::splat(16.));
                                ui.add_sized(
                                    [260., 34.],
                                    egui::Button::image_and_text(img, t).shortcut_text(k).corner_radius(17.),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            };
                            if b(ui, "new", "New note", "Ctrl+N") {
                                open = Some(self.fresh())
                            }
                            if b(ui, "graph", "Open graph", "Ctrl+G") {
                                self.view = View::Graph
                            }
                            if b(ui, "today", "Daily note", "") {
                                open = Some(format!("Daily/{}.md", chrono::Local::now().date_naive()))
                            }
                            if b(ui, "search", "Go to file", "Ctrl+O") {
                                self.sidebar = true;
                                self.searching = true;
                                self.focus_search = true
                            }
                        });
                        return;
                    }
                    if !self.sync_ok {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Sync is not set up.").color(MUTED));
                            if ui.link("Set up sync").clicked() {
                                self.settings = true
                            }
                        });
                    }
                    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
                        result = self.save()
                    }
                    let stem = self.doc.name.trim_end_matches(".md").to_owned();
                    let t = ui.add(
                        egui::TextEdit::singleline(&mut self.title)
                            .font(egui::FontId::new(26., egui::FontFamily::Name("bold".into())))
                            .frame(false)
                            .desired_width(f32::INFINITY),
                    );
                    if std::mem::take(&mut self.focus_title) {
                        t.request_focus()
                    }
                    if t.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.title = stem.clone();
                        ui.memory_mut(|m| m.surrender_focus(t.id));
                    } else if t.lost_focus() && self.title.trim() != stem && !self.title.trim().is_empty() {
                        let to = self.title.trim().to_owned();
                        result = self.rename_to(to);
                        self.title = self.doc.name.trim_end_matches(".md").to_owned();
                    }
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.view, View::Edit, "Edit");
                        ui.selectable_value(&mut self.view, View::Preview, "Preview");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.menu_button("…", |ui| {
                                if ui.button("Save").clicked() {
                                    result = self.save();
                                    ui.close_menu();
                                }
                                if ui.button("Rename").clicked() {
                                    self.focus_title = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .button("Reload from disk")
                                    .on_hover_text("Discard this editor buffer and read the file again")
                                    .clicked()
                                {
                                    result = self.load(self.doc.name.clone());
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button(egui::RichText::new("Delete…").color(Color32::from_rgb(0xe5, 0x53, 0x4b))).clicked() {
                                    self.confirm_delete = true;
                                    ui.close_menu();
                                }
                            });
                        });
                    });
                    if self.confirm_delete {
                        ui.horizontal(|ui| {
                            ui.label("Delete this note?");
                            if ui.button("Delete").clicked() {
                                result = (|| {
                                    let path = self.checked_path()?;
                                    if path.exists() {
                                        let dir = self.vault.root.join(".trash");
                                        fs::create_dir_all(&dir)?;
                                        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                                        let flat = self.doc.name.replace('/', "__");
                                        let dest = dir.join(format!("{stamp}-{flat}"));
                                        fs::rename(&path, &dest)?;
                                        self.undo = Some((self.doc.name.clone(), dest));
                                        self.status = "Moved to .trash".into();
                                    }
                                    self.notes.remove(&self.doc.name);
                                    self.tabs.retain(|t| t != &self.doc.name);
                                    self.doc = Document::default();
                                    self.confirm_delete = false;
                                    Ok(())
                                })()
                            }
                            if ui.button("Cancel").clicked() {
                                self.confirm_delete = false
                            }
                        });
                        if self.doc.name.is_empty() {
                            return;
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        for n in links(&self.doc.text) {
                            if ui.link(&n).clicked() {
                                open = Some(n)
                            }
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("document")
                        .show(ui, |ui| {
                            if self.view == View::Edit {
                                let eid = ui.make_persistent_id("doceditor");
                                let matches: Vec<String> = match &self.pick {
                                    Some((_, _, q)) => {
                                        let q = q.to_lowercase();
                                        self.notes
                                            .keys()
                                            .filter(|n| n.to_lowercase().contains(&q))
                                            .take(6)
                                            .cloned()
                                            .collect()
                                    }
                                    None => Vec::new(),
                                };
                                let mut insert: Option<String> = None;
                                if self.pick.is_some() && !matches.is_empty() {
                                    let take = ui.input_mut(|i| {
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                            || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                                    });
                                    if take {
                                        insert = Some(matches[0].clone())
                                    }
                                }
                                let r = ui.add(
                                    egui::TextEdit::multiline(&mut self.doc.text)
                                        .id(eid)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(26),
                                );
                                let mut cursor = None;
                                if let Some(st) = egui::TextEdit::load_state(ui.ctx(), eid) {
                                    cursor = st.cursor.char_range().map(|c| c.primary.index);
                                }
                                if r.has_focus() {
                                    self.pick = cursor.and_then(|c| {
                                        let before: String = self.doc.text.chars().take(c).collect();
                                        let at = before.rfind("[[")?;
                                        let q = &before[at + 2..];
                                        if q.contains(['\n', ']']) {
                                            return None;
                                        }
                                        Some((before[..at].chars().count() + 2, c, q.to_owned()))
                                    });
                                } else if insert.is_none() {
                                    self.pick = None
                                }
                                if let (Some((s, c, _)), false) = (self.pick.clone(), matches.is_empty()) {
                                    egui::Area::new(egui::Id::new("pickpop"))
                                        .order(egui::Order::Foreground)
                                        .fixed_pos(r.rect.left_top() + Vec2::new(24., 40.))
                                        .show(ui.ctx(), |ui| {
                                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                                ui.set_min_width(240.);
                                                for m in &matches {
                                                    if ui.selectable_label(false, m.trim_end_matches(".md")).clicked() {
                                                        insert = Some(m.clone())
                                                    }
                                                }
                                            });
                                        });
                                    if let Some(m) = insert.take() {
                                        let stem = m.trim_end_matches(".md");
                                        let chars: Vec<char> = self.doc.text.chars().collect();
                                        let mut out: String = chars[..s].iter().collect();
                                        out.push_str(stem);
                                        out.push_str("]]");
                                        let end = out.chars().count();
                                        out.extend(chars[c.min(chars.len())..].iter());
                                        self.doc.text = out;
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
                                    self.status = "Unsaved changes".into();
                                    self.status_err = false;
                                }
                            } else {
                                CommonMarkViewer::new().show(ui, &mut self.cache, &self.doc.text);
                            }
                        });
                });
            });
        if let Some(n) = open.take() {
            result = self.open(n)
        }
        if self.palette {
            let mut cmds: Vec<(String, &str, &str)> = vec![
                ("New note".into(), "Ctrl+N", "new"),
                ("Open graph view".into(), "Ctrl+G", "graph"),
                ("Open today's daily note".into(), "", "daily"),
                ("Search notes".into(), "Ctrl+O", "search"),
                ("Toggle sidebar".into(), "", "side"),
                ("Sync now".into(), "", "sync"),
                ("Sync settings".into(), "", "settings"),
            ];
            if !self.doc.name.is_empty() {
                cmds.push(("Rename current note".into(), "", "rename"));
                cmds.push(("Delete current note".into(), "", "delete"));
                cmds.push(("Toggle preview".into(), "", "preview"));
            }
            let q = self.pal_query.to_lowercase();
            let mut shown: Vec<(String, &str, String)> = cmds
                .into_iter()
                .filter(|(l, _, _)| l.to_lowercase().contains(&q))
                .map(|(l, k, id)| (l, k, id.to_owned()))
                .collect();
            if !q.is_empty() {
                for n in self.notes.keys().filter(|n| n.to_lowercase().contains(&q)).take(8) {
                    shown.push((format!("Open {}", n.trim_end_matches(".md")), "", format!("note:{n}")));
                }
            }
            let mut chosen: Option<String> = None;
            let mut keep = true;
            egui::Window::new("palette")
                .title_bar(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, [0., 80.])
                .fixed_size([460., 0.])
                .show(ctx, |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.pal_query)
                            .hint_text("Type a command or note name...")
                            .desired_width(f32::INFINITY),
                    );
                    r.request_focus();
                    ui.separator();
                    for (i, (label, key, id)) in shown.iter().enumerate() {
                        let b = ui.add_sized(
                            [ui.available_width(), 26.],
                            egui::Button::new(label.as_str()).shortcut_text(*key).frame(i == 0),
                        );
                        if b.clicked() {
                            chosen = Some(id.clone())
                        }
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        chosen = shown.first().map(|s| s.2.clone())
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        keep = false
                    }
                });
            if let Some(id) = chosen {
                keep = false;
                match id.as_str() {
                    "new" => open = Some(self.fresh()),
                    "graph" => self.view = View::Graph,
                    "daily" => open = Some(format!("Daily/{}.md", chrono::Local::now().date_naive())),
                    "search" => {
                        self.sidebar = true;
                        self.searching = true;
                        self.focus_search = true;
                        self.view = View::Edit;
                    }
                    "side" => self.sidebar = !self.sidebar,
                    "sync" => result = self.sync(ctx),
                    "settings" => self.settings = true,
                    "rename" => self.focus_title = true,
                    "delete" => self.confirm_delete = true,
                    "preview" => {
                        self.view = if self.view == View::Preview { View::Edit } else { View::Preview }
                    }
                    n => {
                        if let Some(n) = n.strip_prefix("note:") {
                            open = Some(n.to_owned())
                        }
                    }
                }
                if open.is_some() {
                    result = match open.take() {
                        Some(n) => self.open(n),
                        None => Ok(()),
                    }
                }
            }
            self.palette = keep;
        }
        if self.settings {
            let mut win = true;
            egui::Window::new("Sync settings").open(&mut win).show(ctx, |ui| {
                ui.label("Server URL");
                ui.text_edit_singleline(&mut self.server);
                ui.label("Server token");
                ui.add(egui::TextEdit::singleline(&mut self.secret).password(true));
                if ui.button("Save connection").clicked() {
                    result = sync::configure(
                        &self.vault,
                        sync::Connection {
                            server: self.server.clone(),
                            token: self.secret.clone(),
                        },
                    );
                    if result.is_ok() {
                        self.secret.clear();
                        self.settings = false;
                        self.sync_ok = true;
                        self.status = "Connection saved".into()
                    }
                }
                if ui.button("Test connection").clicked() {
                    let (tx, rx) = mpsc::channel();
                    let (s, t) = (self.server.clone(), self.secret.clone());
                    let c = ctx.clone();
                    self.test_msg = Some((true, "Testing…".into()));
                    self.test_rx = Some(rx);
                    std::thread::spawn(move || {
                        let _ = tx.send(sync::test(&s, &t));
                        c.request_repaint();
                    });
                }
                if let Some(rx) = &self.test_rx {
                    if let Ok(r) = rx.try_recv() {
                        self.test_msg = Some(match r {
                            Ok(m) => (true, m),
                            Err(e) => (false, e.to_string()),
                        });
                        self.test_rx = None;
                    }
                }
                if let Some((ok, m)) = &self.test_msg {
                    let c = if *ok { ACCENT } else { Color32::from_rgb(0xe5, 0x53, 0x4b) };
                    ui.label(egui::RichText::new(m.as_str()).color(c));
                }
                if ui.button("Cancel").clicked() {
                    self.secret.clear();
                    self.settings = false
                }
            });
            if !win {
                self.secret.clear();
                self.settings = false
            }
        }
        if let Err(e) = result {
            self.status = e.to_string();
            self.status_err = true;
        }
    }
}
