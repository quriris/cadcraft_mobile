//! The Android entry point (`android_main`) and the shell around [`CadApp`].

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use winit::platform::android::activity::AndroidApp;

use crate::browser::{self, Browser, Text};
use crate::controls::{Controls, Request};
use crate::{jni_bridge, paths};

/// Stencil bits of the window's render pass, as on the desktop: clipped layout viewports.
const STENCIL_BITS: u8 = 8;

/// How often (seconds) the open browser re-checks whether storage access was granted.
const ACCESS_RECHECK_S: f64 = 1.0;

struct Shell {
    cad: CadApp,
    browser: Rc<RefCell<Browser>>,
    controls: Controls,
    app: AndroidApp,
    root: PathBuf,
    access: bool,
    access_checked_at: f64,
}

impl Shell {
    fn ru(&self) -> bool {
        self.cad.language().starts_with("ru")
    }
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.cad.logic(ctx);
        // There is no window to close on Android; leaving is the Home or Back gesture.
        self.cad.quit_requested = false;
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.controls.rewrite(raw);
        self.cad.raw_input_hook(raw);
        cadcraft_ui_egui::cmdline::capture_tab(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);
        let ctx = ui.ctx().clone();
        let ru = self.ru();
        match self.controls.ui(&ctx, ru) {
            Request::ShowKeyboard => self.app.show_soft_input(true),
            Request::OpenFile => self.browser.borrow_mut().show(),
            Request::None => {}
        }
        let now = ctx.input(|i| i.time);
        let open = self.browser.borrow().open;
        if open && (now - self.access_checked_at > ACCESS_RECHECK_S || self.access_checked_at == 0.0) {
            self.access = jni_bridge::has_storage_access();
            self.access_checked_at = now;
            if self.access {
                let _ = std::fs::create_dir_all(paths::drawings_dir(&self.root));
            }
        }
        if !open {
            return;
        }
        let places = paths::places(&self.root, ru);
        let action = browser::ui(&ctx, &mut self.browser.borrow_mut(), &places, self.access, &Text { ru });
        match action {
            browser::Action::Open(path) => self.cad.open_path(&path.to_string_lossy()),
            browser::Action::RequestAccess => {
                if let Err(e) = jni_bridge::request_storage_access() {
                    self.cad.set_status(format!("Storage access: {e}"));
                }
                // Re-check soon after the user comes back from the settings page.
                self.access_checked_at = now;
            }
            browser::Action::Close | browser::Action::None => {}
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn on_exit(&mut self) {
        self.cad.shutdown_autosave();
    }

    /// Preferences survive restarts, as on the desktop; egui's own state is not kept.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(cadcraft_ui_egui::options::PROFILE_KEY, self.cad.profile_json());
        storage.set_string(cadcraft_ui_egui::PREFS_KEY, self.cad.prefs_json());
        storage.set_string(cadcraft_ui_egui::autosave::PREFS_KEY, cadcraft_ui_egui::autosave::prefs_json());
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// Where saves and exports go: the app's folder in shared storage when it can be written, else
/// the app's own external folder (always writable, visible over USB under Android/data).
fn save_dir(root: &Path, app: &AndroidApp) -> PathBuf {
    let shared = paths::drawings_dir(root);
    if std::fs::create_dir_all(&shared).is_ok() && jni_bridge::has_storage_access() {
        return shared;
    }
    let own = app.external_data_path().unwrap_or_else(|| PathBuf::from(paths::SHARED_ROOT)).join("Drawings");
    let _ = std::fs::create_dir_all(&own);
    own
}

fn services(browser: Rc<RefCell<Browser>>, root: PathBuf, app: AndroidApp) -> Services {
    Services {
        // No blocking file dialog on Android: show the browser; it opens the file it picks.
        pick_open: Some(Box::new(move || {
            browser.borrow_mut().show();
            None
        })),
        pick_save: Some(Box::new(move |name: &str| Some(paths::save_path(&save_dir(&root, &app), name).to_string_lossy().into_owned()))),
    }
}

fn run(app: AndroidApp) -> eframe::Result {
    crate::install_io();
    let private = app.internal_data_path();
    let root = PathBuf::from(paths::SHARED_ROOT);
    let surface = cadcraft_ui_egui::gpu::surface_config(None, false);
    let mut options = eframe::NativeOptions {
        android_app: Some(app.clone()),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(surface),
        persistence_path: private.as_ref().map(|d| d.join("cadcraft.ron")),
        persist_window: false,
        stencil_buffer: STENCIL_BITS,
        ..Default::default()
    };
    // Large selections' previews need more than wgpu's default buffer cap; ask for what the
    // device supports (as on the desktop).
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        let base = std::sync::Arc::clone(&create.device_descriptor);
        create.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            d.required_limits.max_buffer_size = d.required_limits.max_buffer_size.max(adapter.limits().max_buffer_size);
            d
        });
    }
    eframe::run_native(
        "CADCraft",
        options,
        Box::new(move |cc| {
            let start = paths::drawings_dir(&root);
            let browser = Rc::new(RefCell::new(Browser::new(if start.is_dir() { start } else { root.clone() })));
            let mut cad = CadApp::new(Session::empty(), services(Rc::clone(&browser), root.clone(), app.clone()));
            cad.system_languages = jni_bridge::system_language().into_iter().collect();
            if let Some(profile) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::options::PROFILE_KEY)) {
                cad.load_profile(&profile);
            }
            if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::PREFS_KEY)) {
                cad.load_prefs(&prefs);
            }
            if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::autosave::PREFS_KEY)) {
                cadcraft_ui_egui::autosave::load_prefs(&prefs);
            }
            if let Some(dir) = &private {
                cad.enable_autosave(&dir.join("autosave"));
                let support = dir.join("support");
                if let Err(e) = std::fs::create_dir_all(&support) {
                    log::warn!("no LISP support folder {}: {e}", support.display());
                }
                cad.session.set_lisp_support_dir(Some(support));
            }
            if let Some(rs) = &cc.wgpu_render_state {
                cad.set_wgpu(rs, eframe::egui_wgpu::depth_format_from_bits(0, STENCIL_BITS));
            }
            if cad.session.docs.is_empty() {
                cad.session.new_drawing(true);
            }
            Ok(Box::new(Shell { cad, browser, controls: Controls::default(), app, root, access: false, access_checked_at: 0.0 }))
        }),
    )
}

/// Called by android-activity's NativeActivity glue on its own thread once the activity exists.
// The symbol must keep its name for the glue to find it, which `unsafe_code` flags.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("cadcraft"));
    log::info!("CADCraft {} starting", env!("CARGO_PKG_VERSION"));
    if let Err(e) = run(app) {
        log::error!("CADCraft stopped: {e}");
    }
}
