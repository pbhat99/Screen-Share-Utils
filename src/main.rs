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
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;

struct SingleInstanceGuard(HANDLE);

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Acquire a named OS mutex to ensure only one instance of ScreenTools runs at a time.
/// Returns `Some(guard)` if this process is the primary instance, or `None` if already running.
fn acquire_single_instance_mutex(name: PCWSTR) -> Option<SingleInstanceGuard> {
    unsafe {
        let handle = CreateMutexW(None, true, name).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return None;
        }
        Some(SingleInstanceGuard(handle))
    }
}

fn main() -> windows::core::Result<()> {
    let Some(_instance_guard) = acquire_single_instance_mutex(w!("Local\\ScreenToolsSingleInstanceMutex")) else {
        return Ok(());
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_instance_mutex_exclusivity() {
        let mutex_name = w!("Local\\ScreenToolsUnitTestSingleInstanceMutex");
        let first = acquire_single_instance_mutex(mutex_name);
        assert!(first.is_some(), "First instance must acquire mutex");

        let second = acquire_single_instance_mutex(mutex_name);
        assert!(second.is_none(), "Second instance must be rejected while first is alive");

        drop(first);
        let third = acquire_single_instance_mutex(mutex_name);
        assert!(third.is_some(), "Mutex should be acquirable after first instance exits");
    }
}
