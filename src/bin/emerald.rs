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
        Box::new(move |_| Ok(Box::new(Notebook::new(path)?))),
    )
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

struct Notebook {
    vault: Vault,
    notes: BTreeMap<String, String>,
    doc: Document,
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
            vault: Vault::new(path, String::new())?,
            notes: Default::default(),
            doc: Default::default(),
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
        egui::SidePanel::left("vault")
            .default_width(238.)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.heading("Emerald");
                    ui.small(self.vault.root.display().to_string());
                    ui.separator();
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_name)
                            .hint_text("Folder/Note name"),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.new_name.trim().is_empty(),
                                egui::Button::new("New note"),
                            )
                            .clicked()
                        {
                            open = Some(note_name(&self.new_name));
                            self.new_name.clear()
                        }
                        if ui.button("Daily note").clicked() {
                            open = Some(format!("Daily/{}.md", chrono::Local::now().date_naive()))
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Sync").clicked() {
                            result = self.sync(ctx)
                        }
                        if ui.button("Sync settings").clicked() {
                            self.settings = true
                        }
                    });
                    if ui.button("Graph").clicked() {
                        self.view = View::Graph
                    }
                    ui.separator();
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("Search notes or #tags"),
                    );
                    let q = self.query.to_lowercase();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (n, t) in &self.notes {
                            if (q.is_empty()
                                || n.to_lowercase().contains(&q)
                                || t.to_lowercase().contains(&q))
                                && ui.selectable_label(*n == self.doc.name, n).clicked()
                            {
                                open = Some(n.clone())
                            }
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
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
                if self.doc.name.is_empty() {
                    ui.heading("Open or create a note");
                    return;
                }
                ui.heading(&self.doc.name);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.view, View::Edit, "Edit");
                    ui.selectable_value(&mut self.view, View::Preview, "Preview");
                    if ui.button("Save").clicked()
                        || ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
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
                ui.horizontal_wrapped(|ui| {
                    for (n, t) in &self.notes {
                        if links(t).any(|p| p == self.doc.name)
                            && ui.link(format!("← {n}")).clicked()
                        {
                            open = Some(n.clone())
                        }
                    }
                });
                ui.separator();
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
