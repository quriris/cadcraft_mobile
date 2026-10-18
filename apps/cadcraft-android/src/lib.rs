//! CADCraft on Android phones and tablets.
//!
//! The same [`cadcraft_ui_egui::CadApp`] as the desktop and web apps, run through eframe's
//! NativeActivity backend with wgpu (Vulkan, or GLES where Vulkan is missing). What the desktop
//! takes from the operating system is supplied here:
//!
//! - **Files** ([`browser`], [`paths`]): OPEN shows an in-app browser of shared storage; saves and
//!   exports go to `Documents/CADCraft`. Android 11+ asks once for "All files access".
//! - **Touch** ([`controls`]): buttons for Enter, Esc, undo, right-click and the keyboard; the
//!   Back button is Esc.
//! - **Settings, autosave and logs** live in the app's private storage; logs go to logcat
//!   (tag `cadcraft`).
//!
//! The APK is built with cargo-apk; see `docs/android.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod browser;
pub mod controls;
pub mod paths;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod jni_bridge;

/// File format hooks for the engine (it stays I/O-agnostic), as the desktop app installs them.
pub fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    // AutoLISP's entget, entmake and entmod see entities as DXF groups.
    cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
        groups: cadcraft_io::entity_groups,
        entity: cadcraft_io::entity_from_groups,
    });
}
