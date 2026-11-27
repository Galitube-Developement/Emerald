#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use anyhow::{ensure, Result};
use clap::Parser;
use eframe::egui::{self, Color32, Pos2, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use emerald::{atomic, digest, index, sync, target, Vault};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::mpsc};

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
    fn show(&self, ui: &mut egui::Ui, cur: &str, open: &mut Option<String>, force: bool) {
        for (d, t) in &self.dirs {
            egui::CollapsingHeader::new(d)
                .default_open(true)
                .open(force.then_some(true))
                .show(ui, |ui| t.show(ui, cur, open, force));
        }
        for (full, name) in &self.files {
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
            settings: false,
            server: String::new(),
            secret: String::new(),
            status: String::new(),
            cache: Default::default(),
            pending: None,
        };
        app.refresh()?;
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
        self.view = View::Edit;
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
        if self.doc.name.is_empty() || (!self.doc.dirty && !self.doc.saved.is_empty()) {
            return Ok(());
        }
        atomic(&self.checked_path()?, self.doc.text.as_bytes())?;
        self.notes
            .insert(self.doc.name.clone(), self.doc.text.clone());
        self.doc.saved = digest(self.doc.text.as_bytes());
        self.doc.dirty = false;
        self.status = "Saved".into();
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
        self.status = result?;
        Ok(())
    }
    fn graph(&self, ui: &mut egui::Ui) -> Option<String> {
        let (response, p) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
        let r = response.rect;
        let radius = (r.width().min(r.height()) * 0.38).max(1.);
        let points: BTreeMap<_, _> = self
            .notes
            .keys()
            .enumerate()
            .map(|(i, n)| {
                let a = i as f32 * std::f32::consts::TAU / self.notes.len() as f32;
                (n.clone(), r.center() + radius * Vec2::angled(a))
            })
            .collect();
        for (n, t) in &self.notes {
            for link in links(t) {
                if let Some(to) = points.get(&link) {
                    p.line_segment([points[n], *to], (1., Color32::GRAY));
                }
            }
        }
        for (n, pos) in points {
            p.circle_filled(pos, 6., Color32::from_rgb(52, 168, 123));
            p.text(
                pos + Vec2::new(0., 9.),
                egui::Align2::CENTER_TOP,
                &n,
                egui::FontId::proportional(13.),
                ui.visuals().text_color(),
            );
            if response.clicked()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|pt: Pos2| pt.distance(pos) < 16.)
            {
                return Some(n);
            }
        }
        None
    }
}
impl eframe::App for Notebook {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Err(e) = self.poll() {
            self.status = e.to_string()
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
        let mut open = None;
        let mut result = Ok(());
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
                    if ib(ui, "files", "Files", self.sidebar && !self.searching).clicked() {
                        self.sidebar = true;
                        self.searching = false;
                    }
                    if ib(ui, "search", "Search (Ctrl+Shift+F)", self.sidebar && self.searching).clicked() {
                        self.sidebar = true;
                        self.searching = true;
                        self.focus_search = true
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
                    egui::TopBottomPanel::bottom("vaultname")
                        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 8)))
                        .show_inside(ui, |ui| {
                            ui.label(
                                egui::RichText::new(
                                    self.vault.root.file_name().map_or(String::new(), |n| n.to_string_lossy().into()),
                                )
                                .family(egui::FontFamily::Name("bold".into())),
                            );
                        });
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 4. * 32.) / 2.);
                            if ib(ui, "new", "New note", false).clicked() {
                                open = Some(note_name(&format!("Untitled {}", self.notes.len() + 1)))
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
                            tree.show(ui, &self.doc.name, &mut open, !q.is_empty());
                        });
                    });
                });
        }
        egui::SidePanel::right("backlinks")
            .default_width(220.)
            .frame(egui::Frame::new().fill(SIDE).inner_margin(10.))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("BACKLINKS").small().strong().color(MUTED));
                if self.doc.name.is_empty() {
                    ui.label(egui::RichText::new("No note open").color(MUTED));
                } else {
                    let mut any = false;
                    for (n, t) in &self.notes {
                        if links(t).any(|p| p == self.doc.name) {
                            any = true;
                            if ui.link(n).clicked() {
                                open = Some(n.clone())
                            }
                        }
                    }
                    if !any {
                        ui.label(egui::RichText::new("No backlinks").color(MUTED));
                    }
                }
            });
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(RIBBON).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{} notes   {}", self.notes.len(), self.status)).small().color(MUTED));
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(24, 10)))
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                    if self.view == View::Graph {
                        ui.heading("Note graph");
                        if ui.button("Back to editor").clicked() {
                            self.view = View::Edit
                        }
                        if let Some(n) = self.graph(ui) {
                            open = Some(n)
                        }
                        return;
                    }
                    ui.horizontal(|ui| {
                        for tab in self.tabs.clone() {
                            let title = tab.rsplit('/').next().unwrap_or(&tab).trim_end_matches(".md");
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
                            open = Some(note_name(&format!("Untitled {}", self.notes.len() + 1)))
                        }
                    });
                    ui.separator();
                    if self.doc.name.is_empty() {
                        ui.add_space(ui.available_height() * 0.3);
                        ui.vertical_centered(|ui| {
                            let b = |ui: &mut egui::Ui, i: &str, t: &str| {
                                let img = egui::Image::new(svg(i)).tint(MUTED).fit_to_exact_size(Vec2::splat(16.));
                                ui.add_sized([240., 34.], egui::Button::image_and_text(img, t).corner_radius(17.)).clicked()
                            };
                            if b(ui, "new", "New note") {
                                open = Some(note_name(&format!("Untitled {}", self.notes.len() + 1)))
                            }
                            if b(ui, "graph", "Open graph") {
                                self.view = View::Graph
                            }
                            if b(ui, "today", "Daily note") {
                                open = Some(format!("Daily/{}.md", chrono::Local::now().date_naive()))
                            }
                            if b(ui, "search", "Go to file") {
                                self.sidebar = true;
                                self.focus_search = true
                            }
                        });
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.view, View::Edit, "Edit");
                        ui.selectable_value(&mut self.view, View::Preview, "Preview");
                        if ui.button("Save").clicked()
                            || ctx.input_mut(|i| {
                                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)
                            })
                        {
                            result = self.save()
                        }
                        if ui.button("Delete…").clicked() {
                            self.confirm_delete = true
                        }
                        if ui
                            .button("Reload from disk")
                            .on_hover_text("Discard this editor buffer and read the file again")
                            .clicked()
                        {
                            result = self.load(self.doc.name.clone())
                        }
                    });
                    if self.confirm_delete {
                        ui.horizontal(|ui| {
                            ui.label("Delete this note?");
                            if ui.button("Delete").clicked() {
                                result = (|| {
                                    let path = self.checked_path()?;
                                    if path.exists() {
                                        fs::remove_file(path)?
                                    }
                                    self.notes.remove(&self.doc.name);
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
                                if ui
                                    .add(
                                        egui::TextEdit::multiline(&mut self.doc.text)
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(26),
                                    )
                                    .changed()
                                {
                                    self.doc.dirty = true;
                                    self.status = "Unsaved changes".into()
                                }
                            } else {
                                CommonMarkViewer::new().show(ui, &mut self.cache, &self.doc.text);
                            }
                        });
                });
            });
        if let Some(n) = open {
            result = self.open(n)
        }
        if self.settings {
            egui::Window::new("Sync settings").show(ctx, |ui| {
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
                        self.status = "Connection saved".into()
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.secret.clear();
                    self.settings = false
                }
            });
        }
        if let Err(e) = result {
            self.status = e.to_string()
        }
    }
}
