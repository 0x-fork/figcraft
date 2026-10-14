//! When the window can't open.
//!
//! Release builds on Windows have no console, so an error from `eframe::run_native` (no usable
//! graphics adapter, a surface or device the driver refuses) went to a standard error nobody sees
//! and the app quit without a word (#261). Now it is logged and shown in a message box.

/// Where a report goes.
const ISSUES: &str = "https://github.com/storytold/figcraft/issues";

/// The text shown when the window couldn't open with `error`. `backend_set`: `WGPU_BACKEND` named
/// the graphics API, so the hint is to unset it rather than to try another one.
pub fn message(error: &str, backend_set: bool) -> String {
    let hint = if backend_set {
        "WGPU_BACKEND is set and names the graphics API to use; unsetting it lets FigCraft choose."
    } else if cfg!(windows) {
        // Direct3D 12 and OpenGL are both offered (`graphics.rs`), but wgpu keeps the Direct3D 12
        // adapter it picked even when its device then fails, so naming OpenGL alone can still help.
        "Updating the graphics driver often helps. To use OpenGL only, start FigCraft from a Command Prompt with:\n\nset WGPU_BACKEND=gl\nfigcraft.exe"
    } else {
        "Updating the graphics driver often helps. WGPU_BACKEND (vulkan, gl, metal) names another graphics API to try."
    };
    format!("FigCraft couldn't open its window: {error}\n\n{hint}\n\nIf it keeps happening, please report it at {ISSUES} with this message.")
}

/// Log the failure and, unless `quiet` (started for an agent with `--control`: a modal box would
/// keep the caller waiting, and it reads standard error), show it in a message box.
pub fn failed(error: &eframe::Error, quiet: bool) {
    let text = message(&error.to_string(), std::env::var_os("WGPU_BACKEND").is_some());
    // Standard error even when RUST_LOG turned errors off.
    if log::log_enabled!(log::Level::Error) {
        log::error!("{text}");
    } else {
        eprintln!("figcraft: {text}");
    }
    if quiet {
        return;
    }
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("FigCraft")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
