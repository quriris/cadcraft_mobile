//! The in-app file browser that stands in for a desktop Open dialog on Android.
//!
//! The UI crate's file picker is synchronous (`Services::pick_open` returns a path), and Android
//! has no blocking file dialog, so OPEN raises this browser instead and returns nothing; picking a
//! file here opens it with `CadApp::open_path`. Rows are large enough to tap with a finger.
//!
//! Everything here is plain `std::fs` and egui, so it is tested on the desktop too.

use std::path::{Path, PathBuf};

/// Most entries listed from one folder (a hostile or huge folder can't stall the frame).
const MAX_ENTRIES: usize = 4000;

/// File types the browser lists unless "all files" is on: what OPEN reads.
pub const DRAWING_EXTENSIONS: &[&str] = &["dwg", "dxf", "dwt", "svg", "svgz", "lsp"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// A place offered as a shortcut at the top of the browser.
#[derive(Clone, Debug)]
pub struct Place {
    pub label: &'static str,
    pub path: PathBuf,
}

pub struct Browser {
    pub open: bool,
    pub dir: PathBuf,
    pub show_all: bool,
    entries: Vec<Entry>,
    error: Option<String>,
    listed: Option<(PathBuf, bool)>,
}

impl Browser {
    pub fn new(start: PathBuf) -> Self {
        Self { open: false, dir: start, show_all: false, entries: Vec::new(), error: None, listed: None }
    }

    /// Show the browser, re-reading the folder (files may have arrived since).
    pub fn show(&mut self) {
        self.open = true;
        self.listed = None;
    }

    pub fn set_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.listed = None;
    }

    pub fn entries(&mut self) -> &[Entry] {
        self.refresh();
        &self.entries
    }

    pub fn error(&mut self) -> Option<&str> {
        self.refresh();
        self.error.as_deref()
    }

    fn refresh(&mut self) {
        let key = (self.dir.clone(), self.show_all);
        if self.listed.as_ref() == Some(&key) {
            return;
        }
        match list(&self.dir, self.show_all) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
        self.listed = Some(key);
    }

    /// The folder above the current one, if there is one.
    pub fn parent(&self) -> Option<PathBuf> {
        self.dir.parent().map(Path::to_path_buf)
    }
}

/// True for a file OPEN can read, by its extension (any case).
pub fn is_drawing(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| DRAWING_EXTENSIONS.iter().any(|d| d.eq_ignore_ascii_case(e)))
}

/// The folder's entries: folders first, then files, each by name ignoring case. Hidden entries
/// (a leading dot) are left out; files other than drawings too unless `all`.
pub fn list(dir: &Path, all: bool) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for e in rd.flatten().take(MAX_ENTRIES) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        // `metadata` follows symbolic links, so a link to a folder is a folder.
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        let is_dir = meta.is_dir();
        if !is_dir && !all && !is_drawing(&path) {
            continue;
        }
        out.push(Entry { name, path, is_dir, size: if is_dir { 0 } else { meta.len() } });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// A file size the way a file manager shows it.
pub fn human_size(bytes: u64, ru: bool) -> String {
    let (units, sep): (&[&str], &str) = if ru { (&["Б", "КБ", "МБ", "ГБ"], ",") } else { (&["B", "KB", "MB", "GB"], ".") };
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    let unit_name = units.get(unit).copied().unwrap_or("B");
    if unit == 0 { format!("{bytes} {unit_name}") } else { format!("{value:.1} {unit_name}").replace('.', sep) }
}

/// What the user did in the browser this frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Open(PathBuf),
    Close,
    /// The "allow access" button: ask Android for all-files access.
    RequestAccess,
}

/// Interface text in Russian or English (the rest of the app follows its own language setting).
pub struct Text {
    pub ru: bool,
}

impl Text {
    pub fn t<'a>(&self, en: &'a str, ru: &'a str) -> &'a str {
        if self.ru { ru } else { en }
    }
}

/// Draw the browser as a modal over the app. `access` is false while Android withholds access to
/// shared storage; a banner then offers to ask for it.
pub fn ui(ctx: &egui::Context, b: &mut Browser, places: &[Place], access: bool, text: &Text) -> Action {
    if !b.open {
        return Action::None;
    }
    let mut action = Action::None;
    let screen = ctx.content_rect();
    let width = (screen.width() - 32.0).clamp(280.0, 900.0);
    let height = (screen.height() - 64.0).clamp(240.0, 1200.0);
    let row = 48.0;
    let modal = egui::Modal::new(egui::Id::new("cadcraft_android_open")).show(ctx, |ui| {
        ui.set_width(width);
        ui.set_max_height(height);
        ui.horizontal(|ui| {
            ui.heading(text.t("Open drawing", "Открыть чертёж"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(text.t("Cancel", "Отмена")).min_size(egui::vec2(96.0, 40.0))).clicked() {
                    action = Action::Close;
                }
            });
        });
        if !access {
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(text.t(
                    "CADCraft can't see your files yet. Allow access to all files to open drawings from Download, Documents or a messenger.",
                    "Нет доступа к файлам. Разрешите доступ ко всем файлам, чтобы открывать чертежи из «Загрузок», «Документов» и мессенджеров.",
                ));
                if ui.add(egui::Button::new(text.t("Allow access", "Разрешить доступ")).min_size(egui::vec2(160.0, 40.0))).clicked() {
                    action = Action::RequestAccess;
                }
            });
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for p in places {
                if ui.add(egui::Button::new(p.label).min_size(egui::vec2(0.0, 40.0))).clicked() {
                    b.set_dir(p.path.clone());
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let up = b.parent();
            if ui.add_enabled(up.is_some(), egui::Button::new("⬆").min_size(egui::vec2(48.0, 40.0))).clicked()
                && let Some(up) = up
            {
                b.set_dir(up);
            }
            ui.add(egui::Label::new(egui::RichText::new(b.dir.display().to_string()).monospace()).truncate());
        });
        let mut all = b.show_all;
        if ui.checkbox(&mut all, text.t("Show all files", "Показывать все файлы")).changed() {
            b.show_all = all;
        }
        ui.separator();
        if let Some(e) = b.error() {
            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
        }
        let entries = b.entries().to_vec();
        if entries.is_empty() && b.error().is_none() {
            ui.label(text.t("No drawings here.", "Здесь нет чертежей."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(height - 220.0).show_rows(ui, row, entries.len(), |ui, range| {
            for e in entries.get(range).unwrap_or_default() {
                let label = if e.is_dir { format!("📁  {}", e.name) } else { format!("📄  {}    {}", e.name, human_size(e.size, text.ru)) };
                let button = egui::Button::new(label).frame(false).min_size(egui::vec2(ui.available_width(), row - 4.0));
                if ui.add(button).clicked() {
                    if e.is_dir {
                        b.set_dir(e.path.clone());
                    } else {
                        action = Action::Open(e.path.clone());
                    }
                }
            }
        });
    });
    if modal.should_close() && action == Action::None {
        action = Action::Close;
    }
    match &action {
        Action::Open(_) | Action::Close => b.open = false,
        Action::None | Action::RequestAccess => {}
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-android-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn drawings_are_recognised_by_extension() {
        assert!(is_drawing(Path::new("/a/plan.DWG")));
        assert!(is_drawing(Path::new("plan.dxf")));
        assert!(!is_drawing(Path::new("photo.jpg")));
        assert!(!is_drawing(Path::new("noext")));
    }

    #[test]
    fn folders_come_first_and_other_files_are_hidden() {
        let d = temp("list");
        std::fs::create_dir(d.join("Zeta")).unwrap();
        std::fs::create_dir(d.join("alpha")).unwrap();
        std::fs::create_dir(d.join(".hidden")).unwrap();
        std::fs::write(d.join("b.dwg"), b"x").unwrap();
        std::fs::write(d.join("A.dxf"), b"xy").unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        let names: Vec<_> = list(&d, false).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["alpha", "Zeta", "A.dxf", "b.dwg"]);
        let all: Vec<_> = list(&d, true).unwrap().into_iter().map(|e| e.name).collect();
        assert!(all.contains(&"notes.txt".to_string()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_panic() {
        let mut b = Browser::new(PathBuf::from("/definitely/not/here"));
        assert!(b.entries().is_empty());
        assert!(b.error().is_some());
    }

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(human_size(512, false), "512 B");
        assert_eq!(human_size(1536, false), "1.5 KB");
        assert_eq!(human_size(1536, true), "1,5 КБ");
        assert_eq!(human_size(5 * 1024 * 1024, true), "5,0 МБ");
    }
}
