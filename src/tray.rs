use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAYICON: u32 = WM_USER + 101;

// Menu IDs
pub const IDM_TOGGLE_DRAW: usize = 2001;
pub const IDM_TOGGLE_LASER: usize = 2002;
pub const IDM_TOGGLE_HIGHLIGHTER: usize = 2003;
pub const IDM_TOGGLE_OSD: usize = 2004;
pub const IDM_CLEAR_SCREEN: usize = 2005;
pub const IDM_UNDO: usize = 2006;
pub const IDM_REDO: usize = 2009;
pub const IDM_OPEN_CONFIG: usize = 2007;
pub const IDM_EXIT: usize = 2008;
pub const IDM_TOGGLE_DRAWING_PANEL: usize = 2010;

// Tool sub-menu IDs (2100..2199)
pub const IDM_TOOL_FREEHAND: usize = 2101;
pub const IDM_TOOL_LINE: usize = 2102;
pub const IDM_TOOL_ARROW: usize = 2103;
pub const IDM_TOOL_RECT: usize = 2104;
pub const IDM_TOOL_ELLIPSE: usize = 2105;
pub const IDM_TOOL_EMOJI: usize = 2106;
pub const IDM_TOOL_ERASER: usize = 2107;

// Color sub-menu IDs (2200..2299)
pub const IDM_COLOR_BASE: usize = 2200;

// Emoji sub-menu IDs (2400..2499)
pub const IDM_EMOJI_BASE: usize = 2400;

#[allow(dead_code)]
pub const EMOJI_PRESETS: &[(&str, &str)] = &[
    ("⭐ Star", "⭐"),
    ("❤️ Heart", "❤️"),
    ("✔️ Checkmark", "✔️"),
    ("❌ Cross", "❌"),
    ("💡 Idea", "💡"),
    ("🔥 Fire", "🔥"),
    ("🚀 Rocket", "🚀"),
    ("👍 Thumbs Up", "👍"),
    ("🎯 Target", "🎯"),
    ("❓ Question", "❓"),
    ("📌 Pin", "📌"),
    ("💯 100", "💯"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMode {
    Highlighter,
    Laser,
    Drawing,
}

pub struct TrayIcon {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    hicon: HICON,
}

impl TrayIcon {
    pub fn new(hwnd: HWND, initial_mode: TrayMode) -> Result<Self> {
        let hicon = create_squircle_tray_icon(initial_mode);

        let mut tip = [0u16; 128];
        let tip_str = "ScreenTools [💡 Highlighter Mode] - Click for Laser";
        for (i, c) in tip_str.encode_utf16().enumerate().take(127) {
            tip[i] = c;
        }

        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = hicon;
        nid.szTip = tip;

        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }

        Ok(Self { hwnd, nid, hicon })
    }

    pub fn update_mode(&mut self, mode: TrayMode) {
        unsafe {
            let new_hicon = create_squircle_tray_icon(mode);
            if !self.hicon.is_invalid() {
                let _ = DestroyIcon(self.hicon);
            }
            self.hicon = new_hicon;
            self.nid.hIcon = new_hicon;
            self.nid.uFlags = NIF_ICON | NIF_TIP;
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn update_tip(&mut self, text: &str) {
        let mut tip = [0u16; 128];
        for (i, c) in text.encode_utf16().enumerate().take(127) {
            tip[i] = c;
        }
        self.nid.szTip = tip;
        self.nid.uFlags = NIF_TIP;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn show_context_menu(
        &self,
        is_drawing: bool,
        show_toolbar: bool,
        is_laser_enabled: bool,
        is_highlighter_enabled: bool,
        is_osd_enabled: bool,
        current_color_idx: usize,
        color_presets: &[[f32; 3]],
        sync_colors: bool,
    ) -> usize {
        // Automatically sync dark/light theme mode with Windows system settings
        apply_theme_to_menu(self.hwnd);

        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            let main_menu = CreatePopupMenu().unwrap();

            let add_item = |menu: HMENU, id: usize, text: &str, checked: bool, enabled: bool| {
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let mut flags = MF_STRING;
                if checked {
                    flags |= MF_CHECKED;
                }
                if !enabled {
                    flags |= MF_GRAYED | MF_DISABLED;
                }
                let _ = AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr()));
            };

            let add_separator = |menu: HMENU| {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            };

            // 1. Keystroke OSD
            add_item(main_menu, IDM_TOGGLE_OSD, "⌨️ Keystroke OSD", is_osd_enabled, true);

            add_separator(main_menu);

            // 2. Presentation Modes (Highlighter, Laser, Drawing)
            add_item(main_menu, IDM_TOGGLE_HIGHLIGHTER, "💡 Mouse Highlighter", is_highlighter_enabled, true);
            add_item(main_menu, IDM_TOGGLE_LASER, "⚡ Laser Pointer", is_laser_enabled, true);
            add_item(main_menu, IDM_TOGGLE_DRAW, "✏️ Drawing Mode", is_drawing, true);

            add_separator(main_menu);

            // Drawing Panel Toggle (Below divider, enabled only in drawing mode)
            add_item(main_menu, IDM_TOGGLE_DRAWING_PANEL, "🎨 Drawing Panel", show_toolbar, is_drawing);

            // 3. Color, Clear Drawings
            // Color Palette Sub-menu (Applies to active mode or synced to all modes)
            let color_menu = CreatePopupMenu().unwrap();
            for (idx, c) in color_presets.iter().enumerate() {
                let name = crate::color_names::get_color_display_name(*c);
                add_item(color_menu, IDM_COLOR_BASE + idx, &name, current_color_idx == idx, true);
            }
            let color_title_str = if sync_colors {
                "🎨 Color (All Modes - Synced)"
            } else if is_drawing {
                "🎨 Color (Drawing Mode)"
            } else if is_laser_enabled {
                "🎨 Color (Laser Mode)"
            } else {
                "🎨 Color (Highlighter Mode)"
            };
            let color_title: Vec<u16> = color_title_str.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = AppendMenuW(main_menu, MF_POPUP, color_menu.0 as usize, PCWSTR(color_title.as_ptr()));

            add_item(main_menu, IDM_CLEAR_SCREEN, "🗑️ Clear Drawings", false, true);

            add_separator(main_menu);

            // 4. History (Undo / Redo)
            add_item(main_menu, IDM_UNDO, "↩️ Undo", false, true);
            add_item(main_menu, IDM_REDO, "↪️ Redo", false, true);

            add_separator(main_menu);

            // 5. Settings & Exit
            add_item(main_menu, IDM_OPEN_CONFIG, "⚙️ Settings", false, true);
            add_item(main_menu, IDM_EXIT, "🚪 Exit", false, true);

            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(
                main_menu,
                TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD,
                pt.x,
                pt.y,
                0,
                self.hwnd,
                None,
            );
            let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(main_menu);

            cmd.0 as usize
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            if !self.hicon.is_invalid() {
                let _ = DestroyIcon(self.hicon);
            }
        }
    }
}

/// Helper to construct a 32-bit hardware HICON from raw BGRA pixel bytes
unsafe fn create_hicon_from_bgra(width: i32, height: i32, bgra: &[u8]) -> HICON {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut p_bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hdc_screen = GetDC(HWND::default());
    let hbm_color = CreateDIBSection(
        hdc_screen,
        &bmi,
        DIB_RGB_COLORS,
        &mut p_bits,
        HANDLE::default(),
        0,
    ).unwrap_or_default();

    if !p_bits.is_null() && bgra.len() == (width * height * 4) as usize {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), p_bits as *mut u8, bgra.len());
    }

    let hbm_mask = CreateBitmap(width, height, 1, 1, None);

    let icon_info = ICONINFO {
        fIcon: TRUE, // TRUE = icon
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };

    let hicon_raw = CreateIconIndirect(&icon_info).unwrap_or_default();

    let _ = DeleteObject(hbm_color);
    let _ = DeleteObject(hbm_mask);
    let _ = ReleaseDC(HWND::default(), hdc_screen);

    hicon_raw
}

/// Create a sleek 32x32 Squircle Tray Icon matching the active mode
pub fn create_squircle_tray_icon(mode: TrayMode) -> HICON {
    unsafe {
        let size = 32;
        let mut bgra = vec![0u8; size * size * 4];

        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) * 4;

                let cx = 15.5f32;
                let cy = 15.5f32;
                let nx = (x as f32 - cx) / 13.5;
                let ny = (y as f32 - cy) / 13.5;
                let dist = (nx.abs().powi(4) + ny.abs().powi(4)).powf(0.25);

                if dist > 1.08 {
                    continue; // Transparent outside
                }

                let alpha = if dist > 0.95 {
                    ((1.08 - dist) / 0.13).clamp(0.0, 1.0)
                } else {
                    1.0
                };

                let is_border = dist >= 0.78 && dist <= 1.02;

                match mode {
                    TrayMode::Highlighter => {
                        // Amber / Golden Highlighter theme
                        if is_border {
                            bgra[idx] = (30.0 * alpha) as u8;      // B
                            bgra[idx + 1] = (190.0 * alpha) as u8; // G
                            bgra[idx + 2] = (250.0 * alpha) as u8; // R
                            bgra[idx + 3] = (255.0 * alpha) as u8; // A
                        } else {
                            // Dark warm charcoal background
                            bgra[idx] = (20.0 * alpha) as u8;
                            bgra[idx + 1] = (24.0 * alpha) as u8;
                            bgra[idx + 2] = (30.0 * alpha) as u8;
                            bgra[idx + 3] = (245.0 * alpha) as u8;

                            // Glyph: Radiant Sun Beacon
                            let dx = x as f32 - cx;
                            let dy = y as f32 - cy;
                            let r = (dx * dx + dy * dy).sqrt();

                            if r <= 4.2 {
                                // Sun center
                                bgra[idx] = 180;
                                bgra[idx + 1] = 250;
                                bgra[idx + 2] = 255;
                                bgra[idx + 3] = 255;
                            } else if (r >= 5.5 && r <= 8.8) && (
                                dx.abs() <= 1.2 || dy.abs() <= 1.2 || ((dx.abs() - dy.abs()).abs() <= 1.2)
                            ) {
                                // 8 Sunbeams
                                bgra[idx] = 30;
                                bgra[idx + 1] = 210;
                                bgra[idx + 2] = 255;
                                bgra[idx + 3] = 255;
                            }
                        }
                    }
                    TrayMode::Laser => {
                        // Crimson / Neon Red Laser theme
                        if is_border {
                            bgra[idx] = (45.0 * alpha) as u8;      // B
                            bgra[idx + 1] = (50.0 * alpha) as u8;  // G
                            bgra[idx + 2] = (245.0 * alpha) as u8; // R
                            bgra[idx + 3] = (255.0 * alpha) as u8; // A
                        } else {
                            // Dark charcoal with crimson tint
                            bgra[idx] = (22.0 * alpha) as u8;
                            bgra[idx + 1] = (18.0 * alpha) as u8;
                            bgra[idx + 2] = (32.0 * alpha) as u8;
                            bgra[idx + 3] = (245.0 * alpha) as u8;

                            // Glyph: Feather Zap / Lightning Bolt
                            let is_zap = if y >= 7 && y <= 15 {
                                let t = (y - 7) as f32 / 8.0;
                                let center_x = 16.5 - t * 5.5;
                                (x as f32 - center_x).abs() <= 1.6
                            } else if y == 16 {
                                x >= 11 && x <= 20
                            } else if y >= 17 && y <= 24 {
                                let t = (y - 17) as f32 / 7.0;
                                let center_x = 19.5 - t * 5.5;
                                (x as f32 - center_x).abs() <= 1.6
                            } else {
                                false
                            };

                            if is_zap {
                                bgra[idx] = 255;
                                bgra[idx + 1] = 255;
                                bgra[idx + 2] = 255;
                                bgra[idx + 3] = 255;
                            }
                        }
                    }
                    TrayMode::Drawing => {
                        // Azure / Cobalt Blue Drawing theme
                        if is_border {
                            bgra[idx] = (255.0 * alpha) as u8;     // B
                            bgra[idx + 1] = (145.0 * alpha) as u8; // G
                            bgra[idx + 2] = (55.0 * alpha) as u8;  // R
                            bgra[idx + 3] = (255.0 * alpha) as u8; // A
                        } else {
                            // Dark charcoal with sapphire tint
                            bgra[idx] = (36.0 * alpha) as u8;
                            bgra[idx + 1] = (24.0 * alpha) as u8;
                            bgra[idx + 2] = (18.0 * alpha) as u8;
                            bgra[idx + 3] = (245.0 * alpha) as u8;

                            // Glyph: Feather Edit-2 Drawing Pencil
                            let along = ((x as f32 - 9.0) + (22.0 - y as f32)) / 1.4142;
                            let perp = (((x as f32 - 9.0) - (22.0 - y as f32)) / 1.4142).abs();

                            if along >= 0.0 && along <= 18.0 && perp <= 2.2 {
                                if along < 3.2 {
                                    // Lead tip
                                    bgra[idx] = 30; bgra[idx + 1] = 30; bgra[idx + 2] = 35; bgra[idx + 3] = 255;
                                } else if along < 6.0 {
                                    // Wood collar
                                    bgra[idx] = 140; bgra[idx + 1] = 190; bgra[idx + 2] = 230; bgra[idx + 3] = 255;
                                } else if along < 15.0 {
                                    // Azure body
                                    bgra[idx] = 255; bgra[idx + 1] = 230; bgra[idx + 2] = 180; bgra[idx + 3] = 255;
                                } else {
                                    // Cap
                                    bgra[idx] = 210; bgra[idx + 1] = 160; bgra[idx + 2] = 245; bgra[idx + 3] = 255;
                                }
                            }
                        }
                    }
                }
            }
        }

        create_hicon_from_bgra(size as i32, size as i32, &bgra)
    }
}

/// Detect whether Windows is currently set to Dark Mode or Light Mode
pub fn is_system_dark_mode() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_READ, &mut hkey).is_ok() {
            let mut data: u32 = 0;
            let mut data_size = std::mem::size_of::<u32>() as u32;
            let mut val_type = REG_VALUE_TYPE(0);

            // Check SystemUsesLightTheme first, fallback to AppsUseLightTheme
            let res = RegQueryValueExW(
                hkey,
                w!("SystemUsesLightTheme"),
                None,
                Some(&mut val_type),
                Some(&mut data as *mut u32 as *mut u8),
                Some(&mut data_size),
            );

            if res.is_err() {
                let mut data_size = std::mem::size_of::<u32>() as u32;
                let _ = RegQueryValueExW(
                    hkey,
                    w!("AppsUseLightTheme"),
                    None,
                    Some(&mut val_type),
                    Some(&mut data as *mut u32 as *mut u8),
                    Some(&mut data_size),
                );
            }

            let _ = RegCloseKey(hkey);
            data == 0
        } else {
            false
        }
    }
}

/// Apply dark or light theme to the window and standard Win32 menus matching Windows system settings
pub fn apply_theme_to_menu(hwnd: HWND) {
    let is_dark = is_system_dark_mode();
    unsafe {
        let use_dark: BOOL = if is_dark { TRUE } else { FALSE };
        // 1. DWM Immersive Dark Mode for the controller window
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &use_dark as *const _ as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );

        // 2. UXTheme APIs: SetPreferredAppMode (135), AllowDarkModeForWindow (133), FlushMenuThemes (136)
        if let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) {
            // Ordinal 135: SetPreferredAppMode (0 = Default, 1 = AllowDark, 2 = ForceDark, 3 = ForceLight)
            let p_mode = GetProcAddress(uxtheme, PCSTR(135 as *const u8));
            if let Some(func) = p_mode {
                let set_preferred_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(func);
                set_preferred_mode(if is_dark { 2 } else { 3 });
            }

            // Ordinal 133: AllowDarkModeForWindow
            let p_allow = GetProcAddress(uxtheme, PCSTR(133 as *const u8));
            if let Some(func) = p_allow {
                let allow_dark_window: unsafe extern "system" fn(HWND, BOOL) -> BOOL = std::mem::transmute(func);
                let _ = allow_dark_window(hwnd, use_dark);
            }

            // Ordinal 136: FlushMenuThemes
            let p_flush = GetProcAddress(uxtheme, PCSTR(136 as *const u8));
            if let Some(func) = p_flush {
                let flush_menu_themes: unsafe extern "system" fn() = std::mem::transmute(func);
                flush_menu_themes();
            }

            let _ = FreeLibrary(uxtheme);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_theme_detection() {
        // Querying dark mode should complete cleanly without panics
        let is_dark = is_system_dark_mode();
        println!("Detected Windows theme: is_dark = {}", is_dark);
    }
}
