#![windows_subsystem = "windows"]

mod annotation;
mod app;
mod color_names;
mod config;
mod cursor;
mod geometry;
mod keystroke;
mod renderer;
mod settings_ui;
mod toolbar;
mod tray;

use app::App;
use windows::Win32::UI::HiDpi::*;

fn main() -> windows::core::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write("panic.log", format!("PANIC: {}\n", info));
    }));
    let _ = std::fs::remove_file("error.log");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    if let Err(e) = App::create_and_run() {
        let _ = std::fs::write("error.log", format!("App error: {:?}\n", e));
    }
    Ok(())
}
