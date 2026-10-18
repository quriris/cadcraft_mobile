//! Where drawings live on an Android device, and the names files are saved under.

use std::path::{Path, PathBuf};

use crate::browser::Place;

/// The device's shared storage (what a file manager calls "Internal storage").
pub const SHARED_ROOT: &str = "/storage/emulated/0";

/// The app's own folder in shared storage: new drawings, exports and fonts (`fonts/`).
pub fn drawings_dir(root: &Path) -> PathBuf {
    root.join("Documents").join("CADCraft")
}

/// Characters Android's shared storage (FAT rules) refuses in a file name.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// The path a file suggested as `name` is saved to in `dir`: only its last component counts,
/// characters shared storage refuses become `_`, and an empty name becomes `Drawing.dxf`.
pub fn save_path(dir: &Path, name: &str) -> PathBuf {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last.chars().map(|c| if FORBIDDEN.contains(&c) || c.is_control() { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    dir.join(if clean.is_empty() { "Drawing.dxf".to_string() } else { clean })
}

/// Shortcuts at the top of the file browser: the app's folder always, the others when present.
pub fn places(root: &Path, ru: bool) -> Vec<Place> {
    let t = |en: &'static str, r: &'static str| if ru { r } else { en };
    let mut out = vec![Place { label: "CADCraft", path: drawings_dir(root) }];
    let candidates = [
        (t("Download", "Загрузки"), root.join("Download")),
        (t("Documents", "Документы"), root.join("Documents")),
        ("Telegram", root.join("Download").join("Telegram")),
        ("Telegram", root.join("Android/media/org.telegram.messenger/Telegram/Telegram Documents")),
        (t("Internal storage", "Память устройства"), root.to_path_buf()),
    ];
    for (label, path) in candidates {
        if path.is_dir() && !out.iter().any(|p| p.label == label) {
            out.push(Place { label, path });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_names_are_safe_file_names() {
        let dir = Path::new("/d");
        assert_eq!(save_path(dir, "plan.dwg"), Path::new("/d/plan.dwg"));
        assert_eq!(save_path(dir, "/other/place/plan.dxf"), Path::new("/d/plan.dxf"));
        assert_eq!(save_path(dir, "a:b?.pdf"), Path::new("/d/a_b_.pdf"));
        assert_eq!(save_path(dir, ""), Path::new("/d/Drawing.dxf"));
        assert_eq!(save_path(dir, "dir/"), Path::new("/d/Drawing.dxf"));
        assert_eq!(save_path(dir, "..hidden.dxf"), Path::new("/d/hidden.dxf"));
    }

    #[test]
    fn the_app_folder_is_always_offered() {
        let places = places(Path::new("/definitely/not/here"), true);
        assert_eq!(places.len(), 1);
        assert_eq!(places.first().map(|p| p.label), Some("CADCraft"));
    }
}
