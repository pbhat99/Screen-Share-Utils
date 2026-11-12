use crate::config::AppConfig;
use crate::geometry::Point2D;

use std::sync::{Arc, Mutex};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static mut SETTINGS_INSTANCE: Option<Box<SettingsWindow>> = None;

#[repr(C)]
#[allow(non_snake_case)]
pub struct CHOOSECOLORW {
    pub lStructSize: u32,
    pub hwndOwner: HWND,
    pub hInstance: HWND,
    pub rgbResult: COLORREF,
    pub lpCustColors: *mut COLORREF,
    pub Flags: u32,
    pub lCustData: isize,
    pub lpfnHook: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> usize>,
    pub lpTemplateName: PCWSTR,
}

pub const CC_RGBINIT: u32 = 0x00000001;
pub const CC_FULLOPEN: u32 = 0x00000002;
pub const CC_ANYCOLOR: u32 = 0x00000100;

#[link(name = "comdlg32")]
extern "system" {
    pub fn ChooseColorW(lpcc: *mut CHOOSECOLORW) -> BOOL;
}

static mut CUSTOM_COLORS: [COLORREF; 16] = [COLORREF(0x00FFFFFF); 16];

pub fn open_system_color_picker(hwnd_parent: HWND, initial_rgb: [f32; 3]) -> Option<[f32; 3]> {
    let r_u8 = (initial_rgb[0] * 255.0).round().clamp(0.0, 255.0) as u32;
    let g_u8 = (initial_rgb[1] * 255.0).round().clamp(0.0, 255.0) as u32;
    let b_u8 = (initial_rgb[2] * 255.0).round().clamp(0.0, 255.0) as u32;
    let initial_colorref = COLORREF(r_u8 | (g_u8 << 8) | (b_u8 << 16));

    unsafe {
        let mut cc = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: hwnd_parent,
            hInstance: HWND::default(),
            rgbResult: initial_colorref,
            lpCustColors: std::ptr::addr_of_mut!(CUSTOM_COLORS) as *mut COLORREF,
            Flags: CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR,
            lCustData: 0,
            lpfnHook: None,
            lpTemplateName: PCWSTR::null(),
        };

        if ChooseColorW(&mut cc).as_bool() {
            let res = cc.rgbResult.0;
            let r = (res & 0xFF) as f32 / 255.0;
            let g = ((res >> 8) & 0xFF) as f32 / 255.0;
            let b = ((res >> 16) & 0xFF) as f32 / 255.0;
            Some([r, g, b])
        } else {
            None
        }
    }
}

pub fn is_point_over_settings(pt: POINT) -> bool {
    unsafe {
        #[allow(static_mut_refs)]
        if let Some(ref s) = SETTINGS_INSTANCE {
            if IsWindowVisible(s.hwnd).as_bool() {
                let mut rect = RECT::default();
                if GetWindowRect(s.hwnd, &mut rect).is_ok() {
                    return pt.x >= rect.left && pt.x <= rect.right && pt.y >= rect.top && pt.y <= rect.bottom;
                }
            }
        }
        false
    }
}

pub fn ensure_settings_topmost() {
    unsafe {
        #[allow(static_mut_refs)]
        if let Some(ref s) = SETTINGS_INSTANCE {
            if IsWindowVisible(s.hwnd).as_bool() {
                let _ = SetWindowPos(
                    s.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Highlighter,
    Laser,
    Annotation,
    Osd,
}

static mut OLD_EDIT_WNDPROC: WNDPROC = None;

unsafe extern "system" fn edit_subclass_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_KEYDOWN && wparam.0 == VK_RETURN.0 as usize {
        if let Some(ref mut s) = SETTINGS_INSTANCE {
            s.add_emoji_from_input();
        }
        return LRESULT(0);
    }
    CallWindowProcW(OLD_EDIT_WNDPROC, hwnd, msg, wparam, lparam)
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    pub hwnd_edit_emoji: HWND,
    pub config: Arc<Mutex<AppConfig>>,
    pub on_change: Option<Box<dyn Fn() + Send + 'static>>,

    d2d_factory: ID2D1Factory,
    _dwrite_factory: IDWriteFactory,
    render_target: Option<ID2D1HwndRenderTarget>,

    text_format_title: IDWriteTextFormat,
    text_format_heading: IDWriteTextFormat,
    text_format_body: IDWriteTextFormat,
    text_format_small: IDWriteTextFormat,
    text_format_emoji: IDWriteTextFormat,
    text_format_tab: IDWriteTextFormat,

    active_tab: SettingsTab,
    width: u32,
    height: u32,
    is_dragging_slider: Option<usize>,
    pub saved_config: AppConfig,
    pub has_unapplied_changes: bool,
}

impl SettingsWindow {
    pub fn open(config: Arc<Mutex<AppConfig>>, on_change: Option<Box<dyn Fn() + Send + 'static>>) -> Result<()> {
        unsafe {
            if let Some(ref mut instance) = SETTINGS_INSTANCE {
                if IsWindow(instance.hwnd).as_bool() {
                    if let Ok(c) = instance.config.lock() {
                        instance.saved_config = c.clone();
                    }
                    instance.has_unapplied_changes = false;
                    let _ = ShowWindow(instance.hwnd, SW_SHOWNORMAL);
                    let _ = SetForegroundWindow(instance.hwnd);
                    instance.update_tab_visibility();
                    let _ = InvalidateRect(instance.hwnd, None, FALSE);
                    let _ = UpdateWindow(instance.hwnd);
                    return Ok(());
                }
            }

            let instance = GetModuleHandleW(None)?;
            let class_name = w!("ScreenToolsSettingsClass");

            let dark_brush = CreateSolidBrush(COLORREF(0x001e1817));
            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(settings_wndproc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default(),
                hbrBackground: dark_brush,
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wnd_class);

            let width = 780;
            let height = 840;

            let primary_w = GetSystemMetrics(SM_CXSCREEN);
            let primary_h = GetSystemMetrics(SM_CYSCREEN);
            let x = (primary_w - width as i32) / 2;
            let y = (primary_h - height as i32) / 2;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                class_name,
                w!("ScreenTools - Settings & Customization"),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
                x,
                y,
                width as i32,
                height as i32,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )?;

            // Create child EDIT control for custom emojis
            let edit_x = 46;
            let edit_y = 666;
            let edit_w = 120;
            let edit_h = 26;

            let hwnd_edit_emoji = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                edit_x,
                edit_y,
                edit_w,
                edit_h,
                hwnd,
                HMENU(1001 as _),
                instance,
                None,
            )?;

            let hfont = CreateFontW(
                18,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                w!("Segoe UI Emoji"),
            );
            let _ = SendMessageW(hwnd_edit_emoji, WM_SETFONT, WPARAM(hfont.0 as usize), LPARAM(1));

            // Set placeholder cue banner ("Paste emoji...")
            let _ = SendMessageW(hwnd_edit_emoji, 0x1501, WPARAM(1), LPARAM(w!("Paste emoji...").as_ptr() as _));

            // Subclass EDIT control to handle Enter key
            OLD_EDIT_WNDPROC = Some(std::mem::transmute(SetWindowLongPtrW(
                hwnd_edit_emoji,
                GWLP_WNDPROC,
                edit_subclass_wndproc as *const () as usize as isize,
            )));

            crate::tray::apply_theme_to_menu(hwnd);

            let d2d_factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let text_format_title = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                18.0,
                w!("en-US"),
            )?;

            let text_format_heading = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.5,
                w!("en-US"),
            )?;

            let text_format_body = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("en-US"),
            )?;

            let text_format_small = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0,
                w!("en-US"),
            )?;

            let text_format_emoji = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Emoji"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                17.0,
                w!("en-US"),
            )?;

            let text_format_tab = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.0,
                w!("en-US"),
            )?;
            let _ = text_format_tab.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_tab.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let saved_config = if let Ok(c) = config.lock() {
                c.clone()
            } else {
                AppConfig::default()
            };

            let mut win = Box::new(Self {
                hwnd,
                hwnd_edit_emoji,
                config,
                on_change,
                d2d_factory,
                _dwrite_factory: dwrite_factory,
                render_target: None,
                text_format_title,
                text_format_heading,
                text_format_body,
                text_format_small,
                text_format_emoji,
                text_format_tab,
                active_tab: SettingsTab::Highlighter,
                width: width as u32,
                height: height as u32,
                is_dragging_slider: None,
                saved_config,
                has_unapplied_changes: false,
            });

            win.update_tab_visibility();
            win.create_render_target()?;
            SETTINGS_INSTANCE = Some(win);

            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let _ = SetForegroundWindow(hwnd);
            let _ = UpdateWindow(hwnd);
        }
        Ok(())
    }

    fn create_render_target(&mut self) -> Result<()> {
        unsafe {
            let mut rc = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut rc);
            let w = (rc.right - rc.left).max(10) as u32;
            let h = (rc.bottom - rc.top).max(10) as u32;
            self.width = w;
            self.height = h;

            let props = D2D1_RENDER_TARGET_PROPERTIES::default();
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U { width: w, height: h },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };

            let rt = self.d2d_factory.CreateHwndRenderTarget(&props, &hwnd_props)?;
            self.render_target = Some(rt);
        }
        Ok(())
    }

    pub fn update_tab_visibility(&self) {
        unsafe {
            if self.active_tab == SettingsTab::Annotation {
                let edit_y = 666;
                let _ = SetWindowPos(
                    self.hwnd_edit_emoji,
                    HWND::default(),
                    46,
                    edit_y,
                    120,
                    26,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(self.hwnd_edit_emoji, SW_SHOW);
                let _ = InvalidateRect(self.hwnd_edit_emoji, None, TRUE);
            } else {
                let _ = ShowWindow(self.hwnd_edit_emoji, SW_HIDE);
            }
        }
    }

    pub fn apply_changes(&mut self) {
        if let Ok(c) = self.config.lock() {
            let _ = c.save();
            self.saved_config = c.clone();
        }
        self.has_unapplied_changes = false;
        if let Some(ref cb) = self.on_change {
            cb();
        }
        self.trigger_repaint();
    }

    pub fn cancel_changes(&mut self) {
        if self.has_unapplied_changes {
            if let Ok(mut c) = self.config.lock() {
                *c = self.saved_config.clone();
                let _ = c.save();
            }
            self.has_unapplied_changes = false;
            if let Some(ref cb) = self.on_change {
                cb();
            }
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn add_emoji_from_input(&mut self) {
        unsafe {
            let len = GetWindowTextLengthW(self.hwnd_edit_emoji);
            if len <= 0 {
                return;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let actual = GetWindowTextW(self.hwnd_edit_emoji, &mut buf);
            if actual <= 0 {
                return;
            }
            let text = String::from_utf16_lossy(&buf[..actual as usize]);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }

            if let Ok(mut c) = self.config.lock() {
                if c.annotation.custom_emojis.len() >= 9 {
                    return;
                }
                let emoji_str = trimmed.to_string();
                if !c.annotation.custom_emojis.iter().any(|e| e == &emoji_str) {
                    c.annotation.custom_emojis.push(emoji_str.clone());
                }
                c.annotation.selected_emoji = emoji_str;
                let _ = c.save();
            }

            let _ = SetWindowTextW(self.hwnd_edit_emoji, w!(""));
            self.notify_change();
        }
    }

    pub fn render(&mut self) -> Result<()> {
        if self.render_target.is_none() {
            self.create_render_target()?;
        }
        self.update_tab_visibility();
        let rt = self.render_target.as_ref().unwrap();

        let cfg = if let Ok(c) = self.config.lock() {
            c.clone()
        } else {
            return Ok(());
        };

        unsafe {
            rt.BeginDraw();
            // Background (#17181e)
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.09, g: 0.095, b: 0.12, a: 1.0 }));

            // 1. Top Header Bar
            let app_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.95, g: 0.95, b: 1.0, a: 1.0 }, None)?;
            let title_wide: Vec<u16> = "ScreenTools".encode_utf16().collect();
            let title_rect = D2D_RECT_F { left: 24.0, top: 16.0, right: 300.0, bottom: 42.0 };
            rt.DrawText(&title_wide, &self.text_format_title, &title_rect, &app_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            let sub_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.55, g: 0.58, b: 0.65, a: 1.0 }, None)?;
            let sub_wide: Vec<u16> = "Preferences & Style Customization".encode_utf16().collect();
            let sub_rect = D2D_RECT_F { left: 24.0, top: 42.0, right: 350.0, bottom: 62.0 };
            rt.DrawText(&sub_wide, &self.text_format_small, &sub_rect, &sub_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Reset Defaults Button (Top Right: 606..756, 18..52)
            let reset_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: 606.0, top: 18.0, right: 756.0, bottom: 52.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let reset_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.16, g: 0.17, b: 0.22, a: 1.0 }, None)?;
            rt.FillRoundedRectangle(&reset_rect, &reset_bg);
            let reset_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.28, g: 0.30, b: 0.38, a: 0.8 }, None)?;
            rt.DrawRoundedRectangle(&reset_rect, &reset_border, 1.0, None);

            let reset_text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.82, g: 0.85, b: 0.92, a: 1.0 }, None)?;
            let reset_wide: Vec<u16> = "↺ Reset Defaults".encode_utf16().collect();
            let reset_text_rect = D2D_RECT_F { left: 606.0, top: 18.0, right: 756.0, bottom: 52.0 };
            rt.DrawText(&reset_wide, &self.text_format_tab, &reset_text_rect, &reset_text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // 2. Horizontal Tabs (Top: 68.0 .. 106.0)
            let tabs = [
                (SettingsTab::Highlighter, "💡 Highlighter"),
                (SettingsTab::Laser, "⚡ Laser Pointer"),
                (SettingsTab::Annotation, "✏️ Drawing & Tools"),
                (SettingsTab::Osd, "⌨️ Keystroke OSD"),
            ];

            let tab_y = 68.0;
            let tab_h = 38.0;
            let tab_w = 177.0;
            let tab_gap = 8.0;

            for (i, (tab_type, tab_name)) in tabs.iter().enumerate() {
                let tx = 24.0 + i as f32 * (tab_w + tab_gap);
                let tab_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: tx, top: tab_y, right: tx + tab_w, bottom: tab_y + tab_h },
                    radiusX: 7.0,
                    radiusY: 7.0,
                };

                let is_active = self.active_tab == *tab_type;
                if is_active {
                    let active_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.25, b: 0.40, a: 1.0 }, None)?;
                    rt.FillRoundedRectangle(&tab_rect, &active_bg);
                    let active_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.38, g: 0.55, b: 1.0, a: 1.0 }, None)?;
                    rt.DrawRoundedRectangle(&tab_rect, &active_border, 1.5, None);
                } else {
                    let inactive_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.12, g: 0.13, b: 0.17, a: 0.8 }, None)?;
                    rt.FillRoundedRectangle(&tab_rect, &inactive_bg);
                    let inactive_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: 0.6 }, None)?;
                    rt.DrawRoundedRectangle(&tab_rect, &inactive_border, 1.0, None);
                }

                let text_color = if is_active {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
                } else {
                    D2D1_COLOR_F { r: 0.70, g: 0.73, b: 0.80, a: 1.0 }
                };
                let tab_text_brush = rt.CreateSolidColorBrush(&text_color, None)?;
                let wide_tab: Vec<u16> = tab_name.encode_utf16().collect();
                let text_bounds = D2D_RECT_F { left: tx, top: tab_y, right: tx + tab_w, bottom: tab_y + tab_h };
                rt.DrawText(&wide_tab, &self.text_format_tab, &text_bounds, &tab_text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }

            // 3. Main Content Area
            let cx = 24.0;
            let cy = 118.0;

            match self.active_tab {
                SettingsTab::Highlighter => {
                    self.render_highlighter_tab(rt, &cfg, cx, cy)?;
                }
                SettingsTab::Laser => {
                    self.render_laser_tab(rt, &cfg, cx, cy)?;
                }
                SettingsTab::Annotation => {
                    self.render_annotation_tab(rt, &cfg, cx, cy)?;
                }
                SettingsTab::Osd => {
                    self.render_osd_tab(rt, &cfg, cx, cy)?;
                }
            }

            // 4. Bottom Footer Action Bar: Apply & Cancel
            let footer_y = (self.height as f32 - 48.0).max(752.0);
            let btn_h = 32.0;

            // Subtle divider line above footer
            let div_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.25, b: 0.32, a: 0.7 }, None)?;
            rt.DrawLine(
                D2D_POINT_2F { x: 24.0, y: footer_y - 12.0 },
                D2D_POINT_2F { x: self.width as f32 - 24.0, y: footer_y - 12.0 },
                &div_brush,
                1.0,
                None,
            );

            // Left status indicator
            let status_rect = D2D_RECT_F {
                left: 24.0,
                top: footer_y + 6.0,
                right: 480.0,
                bottom: footer_y + btn_h,
            };
            if self.has_unapplied_changes {
                let pend_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.95, g: 0.75, b: 0.30, a: 1.0 }, None)?;
                let pend_msg: Vec<u16> = "● Unsaved changes pending — click Apply to save".encode_utf16().collect();
                rt.DrawText(&pend_msg, &self.text_format_small, &status_rect, &pend_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            } else {
                let saved_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.45, g: 0.75, b: 0.55, a: 0.8 }, None)?;
                let saved_msg: Vec<u16> = "✓ All settings applied".encode_utf16().collect();
                rt.DrawText(&saved_msg, &self.text_format_small, &status_rect, &saved_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }

            // Right buttons: Cancel and Apply
            let apply_w = 110.0;
            let cancel_w = 95.0;
            let apply_x = self.width as f32 - 24.0 - apply_w;
            let cancel_x = apply_x - 12.0 - cancel_w;

            // Cancel Button
            let cancel_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cancel_x, top: footer_y, right: cancel_x + cancel_w, bottom: footer_y + btn_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let c_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.16, g: 0.18, b: 0.23, a: 1.0 }, None)?;
            let c_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.32, g: 0.36, b: 0.45, a: 1.0 }, None)?;
            let c_text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
            rt.FillRoundedRectangle(&cancel_rect, &c_bg);
            rt.DrawRoundedRectangle(&cancel_rect, &c_border, 1.0, None);
            let cancel_lbl: Vec<u16> = "Cancel".encode_utf16().collect();
            let c_text_rect = D2D_RECT_F { left: cancel_x, top: footer_y + 6.0, right: cancel_x + cancel_w, bottom: footer_y + btn_h };
            rt.DrawText(&cancel_lbl, &self.text_format_tab, &c_text_rect, &c_text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Apply Button
            let apply_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: apply_x, top: footer_y, right: apply_x + apply_w, bottom: footer_y + btn_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let (a_bg_col, a_border_col, a_text_col) = if self.has_unapplied_changes {
                (
                    D2D1_COLOR_F { r: 0.20, g: 0.45, b: 0.88, a: 1.0 },
                    D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: 1.0 },
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                )
            } else {
                (
                    D2D1_COLOR_F { r: 0.14, g: 0.18, b: 0.26, a: 0.7 },
                    D2D1_COLOR_F { r: 0.22, g: 0.28, b: 0.38, a: 0.6 },
                    D2D1_COLOR_F { r: 0.50, g: 0.60, b: 0.75, a: 0.8 },
                )
            };
            let a_bg = rt.CreateSolidColorBrush(&a_bg_col, None)?;
            let a_border = rt.CreateSolidColorBrush(&a_border_col, None)?;
            let a_text_brush = rt.CreateSolidColorBrush(&a_text_col, None)?;
            rt.FillRoundedRectangle(&apply_rect, &a_bg);
            rt.DrawRoundedRectangle(&apply_rect, &a_border, 1.2, None);
            let apply_lbl: Vec<u16> = (if self.has_unapplied_changes { "Apply" } else { "Applied" }).encode_utf16().collect();
            let a_text_rect = D2D_RECT_F { left: apply_x, top: footer_y + 6.0, right: apply_x + apply_w, bottom: footer_y + btn_h };
            rt.DrawText(&apply_lbl, &self.text_format_tab, &a_text_rect, &a_text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            rt.EndDraw(None, None)?;
        }
        Ok(())
    }

    unsafe fn render_card(&self, rt: &ID2D1HwndRenderTarget, rect: D2D_RECT_F) -> Result<()> {
        let rounded = D2D1_ROUNDED_RECT { rect, radiusX: 10.0, radiusY: 10.0 };
        let bg_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.115, g: 0.12, b: 0.16, a: 1.0 }, None)?;
        let border_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: 1.0 }, None)?;
        rt.FillRoundedRectangle(&rounded, &bg_brush);
        rt.DrawRoundedRectangle(&rounded, &border_brush, 1.0, None);
        Ok(())
    }

    unsafe fn render_slider(
        &self,
        rt: &ID2D1HwndRenderTarget,
        label: &str,
        value_str: &str,
        frac: f32,
        x: f32,
        y: f32,
        w: f32,
    ) -> Result<()> {
        let text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
        let val_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.40, g: 0.65, b: 1.0, a: 1.0 }, None)?;

        let wide_label: Vec<u16> = label.encode_utf16().collect();
        let label_rect = D2D_RECT_F { left: x, top: y, right: x + w - 80.0, bottom: y + 20.0 };
        rt.DrawText(&wide_label, &self.text_format_heading, &label_rect, &text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let wide_val: Vec<u16> = value_str.encode_utf16().collect();
        let val_rect = D2D_RECT_F { left: x + w - 80.0, top: y, right: x + w, bottom: y + 20.0 };
        rt.DrawText(&wide_val, &self.text_format_heading, &val_rect, &val_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Slider track
        let track_y = y + 27.0;
        let track_h = 6.0;
        let track_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: 1.0 }, None)?;
        let track_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x, top: track_y, right: x + w, bottom: track_y + track_h },
            radiusX: 3.0,
            radiusY: 3.0,
        };
        rt.FillRoundedRectangle(&track_rect, &track_bg);

        // Active fill
        let fill_w = w * frac.clamp(0.0, 1.0);
        let fill_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.30, g: 0.55, b: 0.98, a: 1.0 }, None)?;
        let fill_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x, top: track_y, right: x + fill_w, bottom: track_y + track_h },
            radiusX: 3.0,
            radiusY: 3.0,
        };
        rt.FillRoundedRectangle(&fill_rect, &fill_bg);

        // Thumb knob
        let thumb_x = x + fill_w;
        let thumb_r = 7.5;
        let thumb_ellipse = D2D1_ELLIPSE { point: D2D_POINT_2F { x: thumb_x, y: track_y + track_h * 0.5 }, radiusX: thumb_r, radiusY: thumb_r };
        let thumb_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
        rt.FillEllipse(&thumb_ellipse, &thumb_brush);

        Ok(())
    }

    unsafe fn render_toggle(
        &self,
        rt: &ID2D1HwndRenderTarget,
        label: &str,
        enabled: bool,
        x: f32,
        y: f32,
        w: f32,
    ) -> Result<()> {
        let text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
        let wide_label: Vec<u16> = label.encode_utf16().collect();
        let label_rect = D2D_RECT_F { left: x, top: y + 2.0, right: x + w - 60.0, bottom: y + 25.0 };
        rt.DrawText(&wide_label, &self.text_format_heading, &label_rect, &text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let switch_w = 46.0;
        let switch_h = 24.0;
        let switch_x = x + w - switch_w;
        let switch_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: switch_x, top: y, right: switch_x + switch_w, bottom: y + switch_h },
            radiusX: 12.0,
            radiusY: 12.0,
        };

        let (switch_bg, knob_x) = if enabled {
            (
                rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.75, b: 0.40, a: 1.0 }, None)?,
                switch_x + switch_w - 12.0,
            )
        } else {
            (
                rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.24, g: 0.26, b: 0.32, a: 1.0 }, None)?,
                switch_x + 12.0,
            )
        };
        rt.FillRoundedRectangle(&switch_rect, &switch_bg);

        let knob_ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: knob_x, y: y + 12.0 },
            radiusX: 9.0,
            radiusY: 9.0,
        };
        let knob_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
        rt.FillEllipse(&knob_ellipse, &knob_brush);

        Ok(())
    }

    unsafe fn render_highlighter_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig, cx: f32, cy: f32) -> Result<()> {
        let card_w = 732.0;
        let card_h = 320.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: cy, right: cx + card_w, bottom: cy + card_h })?;

        let ix = cx + 22.0;
        let mut iy = cy + 16.0;
        let cw = card_w - 44.0;

        self.render_toggle(rt, "Enable Mouse Cursor Highlighter", cfg.highlighter.enabled, ix, iy, cw)?;
        iy += 38.0;

        let size_frac = ((cfg.highlighter.size - 10.0) / 90.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Size / Diameter", &format!("{:.0} px", cfg.highlighter.size), size_frac, ix, iy, cw)?;
        iy += 46.0;

        let soft_frac = cfg.highlighter.edge_softness.clamp(0.0, 1.0);
        self.render_slider(rt, "Edge Softness / Feather", &format!("{:.0} %", soft_frac * 100.0), soft_frac, ix, iy, cw)?;
        iy += 46.0;

        let op_frac = cfg.highlighter.opacity.clamp(0.0, 1.0);
        self.render_slider(rt, "Glow Opacity", &format!("{:.0} %", op_frac * 100.0), op_frac, ix, iy, cw)?;
        iy += 48.0;

        // Highlighter Color Swatches (Using cfg.annotation.color_presets)
        let heading_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
        let color_name = crate::color_names::get_color_display_name(cfg.highlighter.color);
        let hex = crate::color_names::get_hex_string(cfg.highlighter.color);
        let c_title_str = format!("Highlighter Color: {} ({})", color_name, hex);
        let c_title: Vec<u16> = c_title_str.encode_utf16().collect();
        rt.DrawText(&c_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 20.0 }, &heading_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        iy += 20.0;

        let hint_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.55, g: 0.60, b: 0.70, a: 1.0 }, None)?;
        let hint_text: Vec<u16> = "Left-click: select  •  Right-click: open Windows Color Picker".encode_utf16().collect();
        rt.DrawText(&hint_text, &self.text_format_small, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 16.0 }, &hint_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        iy += 20.0;

        let swatch_size = 34.0;
        let swatch_gap = 12.0;
        for (idx, c) in cfg.annotation.color_presets.iter().enumerate() {
            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
            let s_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: sx, top: iy, right: sx + swatch_size, bottom: iy + swatch_size },
                radiusX: 7.0,
                radiusY: 7.0,
            };

            let s_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: 1.0 }, None)?;
            rt.FillRoundedRectangle(&s_rect, &s_brush);

            let is_sel = (c[0] - cfg.highlighter.color[0]).abs() < 0.04
                && (c[1] - cfg.highlighter.color[1]).abs() < 0.04
                && (c[2] - cfg.highlighter.color[2]).abs() < 0.04;

            if is_sel {
                let border_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
                rt.DrawRoundedRectangle(&s_rect, &border_sel, 2.5, None);
            } else {
                let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }, None)?;
                rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
            }
        }
        iy += 44.0;

        self.render_toggle(rt, "Sync Color Across Modes (Highlighter, Laser & Drawing)", cfg.sync_colors, ix, iy, cw)?;

        // Live Preview Box at Bottom
        let preview_card_y = cy + card_h + 12.0;
        let preview_h = 160.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: preview_card_y, right: cx + card_w, bottom: preview_card_y + preview_h })?;

        let preview_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.60, g: 0.64, b: 0.72, a: 1.0 }, None)?;
        let preview_wide: Vec<u16> = "LIVE PREVIEW (Highlighter Dot)".encode_utf16().collect();
        rt.DrawText(&preview_wide, &self.text_format_small, &D2D_RECT_F { left: cx + 20.0, top: preview_card_y + 12.0, right: cx + card_w, bottom: preview_card_y + 30.0 }, &preview_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Draw live highlighter circle in center of preview box
        let center_pt = Point2D::new(cx + card_w * 0.5, preview_card_y + preview_h * 0.55);
        let r = cfg.highlighter.size.max(5.0);
        let softness = cfg.highlighter.edge_softness.clamp(0.01, 0.99);
        let inner_stop_pos = (1.0 - softness).clamp(0.0, 0.98);

        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { r: cfg.highlighter.color[0], g: cfg.highlighter.color[1], b: cfg.highlighter.color[2], a: cfg.highlighter.opacity } },
            D2D1_GRADIENT_STOP { position: inner_stop_pos, color: D2D1_COLOR_F { r: cfg.highlighter.color[0], g: cfg.highlighter.color[1], b: cfg.highlighter.color[2], a: cfg.highlighter.opacity * 0.95 } },
            D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { r: cfg.highlighter.color[0], g: cfg.highlighter.color[1], b: cfg.highlighter.color[2], a: 0.0 } },
        ];
        if let Ok(stop_coll) = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) {
            let brush_props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: D2D_POINT_2F { x: center_pt.x, y: center_pt.y },
                gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
                radiusX: r,
                radiusY: r,
            };
            if let Ok(brush) = rt.CreateRadialGradientBrush(&brush_props, None, &stop_coll) {
                let ellipse = D2D1_ELLIPSE { point: D2D_POINT_2F { x: center_pt.x, y: center_pt.y }, radiusX: r, radiusY: r };
                rt.FillEllipse(&ellipse, &brush);
            }
        }

        // Draw mouse pointer cursor icon in the center of preview
        let cursor_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }, None)?;
        let cursor_outline = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.9 }, None)?;
        let c_pt = D2D1_ELLIPSE { point: D2D_POINT_2F { x: center_pt.x, y: center_pt.y }, radiusX: 3.5, radiusY: 3.5 };
        rt.FillEllipse(&c_pt, &cursor_brush);
        rt.DrawEllipse(&c_pt, &cursor_outline, 1.0, None);

        Ok(())
    }

    unsafe fn render_laser_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig, cx: f32, cy: f32) -> Result<()> {
        let card_w = 732.0;
        let card_h = 245.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: cy, right: cx + card_w, bottom: cy + card_h })?;

        let ix = cx + 22.0;
        let mut iy = cy + 18.0;
        let cw = card_w - 44.0;

        let dur_frac = ((cfg.laser.trail_duration_ms as f32 - 200.0) / 1800.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Laser Trail Duration", &format!("{} ms", cfg.laser.trail_duration_ms), dur_frac, ix, iy, cw)?;
        iy += 48.0;

        let rad_frac = ((cfg.laser.radius - 3.0) / 17.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Laser Dot Radius", &format!("{:.1} px", cfg.laser.radius), rad_frac, ix, iy, cw)?;
        iy += 48.0;

        // Laser Color Swatches
        let heading_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
        let color_name = crate::color_names::get_color_display_name(cfg.laser.color);
        let hex = crate::color_names::get_hex_string(cfg.laser.color);

        if cfg.sync_colors {
            let c_title: Vec<u16> = format!("Laser Color: {} ({}) — Locked", color_name, hex).encode_utf16().collect();
            let dim_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.65, g: 0.68, b: 0.75, a: 1.0 }, None)?;
            rt.DrawText(&c_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 20.0 }, &dim_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy += 20.0;

            let lock_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.50, g: 0.54, b: 0.65, a: 1.0 }, None)?;
            let lock_text: Vec<u16> = "🔒 Color is synced with Highlighter (uncheck 'Sync Color' in Highlighter tab to edit)".encode_utf16().collect();
            rt.DrawText(&lock_text, &self.text_format_small, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 16.0 }, &lock_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy += 20.0;
        } else {
            let c_title: Vec<u16> = format!("Laser Color: {} ({})", color_name, hex).encode_utf16().collect();
            rt.DrawText(&c_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 20.0 }, &heading_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy += 20.0;

            let hint_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.55, g: 0.60, b: 0.70, a: 1.0 }, None)?;
            let hint_text: Vec<u16> = "Left-click: select  •  Right-click: open Windows Color Picker".encode_utf16().collect();
            rt.DrawText(&hint_text, &self.text_format_small, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 16.0 }, &hint_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy += 20.0;
        }

        let swatch_size = 34.0;
        let swatch_gap = 12.0;
        let swatch_alpha = if cfg.sync_colors { 0.28 } else { 1.0 };

        for (idx, c) in cfg.annotation.color_presets.iter().enumerate() {
            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
            let s_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: sx, top: iy, right: sx + swatch_size, bottom: iy + swatch_size },
                radiusX: 7.0,
                radiusY: 7.0,
            };

            let s_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: swatch_alpha }, None)?;
            rt.FillRoundedRectangle(&s_rect, &s_brush);

            if !cfg.sync_colors {
                let is_sel = (c[0] - cfg.laser.color[0]).abs() < 0.04
                    && (c[1] - cfg.laser.color[1]).abs() < 0.04
                    && (c[2] - cfg.laser.color[2]).abs() < 0.04;

                if is_sel {
                    let border_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
                    rt.DrawRoundedRectangle(&s_rect, &border_sel, 2.5, None);
                } else {
                    let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }, None)?;
                    rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
                }
            } else {
                let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.3, g: 0.3, b: 0.35, a: 0.2 }, None)?;
                rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
            }
        }
        iy += 44.0;

        let tip_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.65, g: 0.70, b: 0.80, a: 1.0 }, None)?;
        let tip_wide: Vec<u16> = "ℹ Left-click the tray icon to cycle presentation modes (Highlighter → Laser → Drawing).".encode_utf16().collect();
        let tip_rect = D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 25.0 };
        rt.DrawText(&tip_wide, &self.text_format_body, &tip_rect, &tip_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Live Preview Box (Smooth continuous laser line, zero glow, fading opacity and size)
        let preview_y = cy + card_h + 12.0;
        let preview_h = 230.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: preview_y, right: cx + card_w, bottom: preview_y + preview_h })?;

        let p_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.60, g: 0.64, b: 0.72, a: 1.0 }, None)?;
        let p_wide: Vec<u16> = "LIVE PREVIEW (Single Color Laser Line - Fades Size & Opacity)".encode_utf16().collect();
        rt.DrawText(&p_wide, &self.text_format_small, &D2D_RECT_F { left: cx + 20.0, top: preview_y + 12.0, right: cx + card_w, bottom: preview_y + 30.0 }, &p_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Draw a simulated continuous tapered laser ribbon beam in preview
        let base_r = cfg.laser.radius.max(2.5);
        let trail_center_x = cx + card_w * 0.45;
        let trail_center_y = preview_y + preview_h * 0.58;

        let mut sim_pts: Vec<(Point2D, f32)> = Vec::with_capacity(33);
        let mut last_px = trail_center_x;
        let mut last_py = trail_center_y;

        for step in 0..=32 {
            let t = step as f32 / 32.0; // 0 = tail, 1 = head
            let angle = (t * 2.8) - 1.4;
            let px = trail_center_x + angle.sin() * 145.0;
            let py = trail_center_y - angle.cos() * 52.0 + 32.0;
            last_px = px;
            last_py = py;
            sim_pts.push((Point2D::new(px, py), t));
        }

        let p_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: cfg.laser.color[0],
                g: cfg.laser.color[1],
                b: cfg.laser.color[2],
                a: 1.0,
            },
            None,
        )?;

        // Render preview ribbon segments fading size and opacity over time
        for i in 0..(sim_pts.len() - 1) {
            let p1 = sim_pts[i].0;
            let p2 = sim_pts[i + 1].0;
            let t1 = sim_pts[i].1;
            let t2 = sim_pts[i + 1].1;
            let t_avg = (t1 + t2) * 0.5;
            let alpha = (t_avg * 0.95).clamp(0.0, 1.0);
            if alpha < 0.02 {
                continue;
            }

            let dir = Point2D::new(p2.x - p1.x, p2.y - p1.y);
            let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
            let (nx, ny) = if len > 0.001 { (-dir.y / len, dir.x / len) } else { (0.0, 1.0) };

            let hw1 = (base_r * t1).max(0.5);
            let hw2 = (base_r * t2).max(0.5);

            let l1 = D2D_POINT_2F { x: p1.x + nx * hw1, y: p1.y + ny * hw1 };
            let r1 = D2D_POINT_2F { x: p1.x - nx * hw1, y: p1.y - ny * hw1 };
            let l2 = D2D_POINT_2F { x: p2.x + nx * hw2, y: p2.y + ny * hw2 };
            let r2 = D2D_POINT_2F { x: p2.x - nx * hw2, y: p2.y - ny * hw2 };

            if let Ok(path) = self.d2d_factory.CreatePathGeometry() {
                if let Ok(sink) = path.Open() {
                    sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                    sink.BeginFigure(l1, D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(l2);
                    sink.AddLine(r2);
                    sink.AddLine(r1);
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();
                    p_brush.SetOpacity(alpha);
                    rt.FillGeometry(&path, &p_brush, None);
                }
            }
        }

        // Pointer dot only at the head (Single solid circle, zero glow)
        let dot_core = D2D1_ELLIPSE { point: D2D_POINT_2F { x: last_px, y: last_py }, radiusX: base_r, radiusY: base_r };
        p_brush.SetOpacity(0.95);
        rt.FillEllipse(&dot_core, &p_brush);

        Ok(())
    }

    unsafe fn render_annotation_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig, cx: f32, cy: f32) -> Result<()> {
        let card_w = 732.0;
        let card_h = 394.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: cy, right: cx + card_w, bottom: cy + card_h })?;

        let ix = cx + 22.0;
        let mut iy = cy + 14.0;
        let cw = card_w - 44.0;

        // 1. Control panel toggle (show_toolbar)
        self.render_toggle(rt, "Show Floating Drawing Control Panel", cfg.annotation.show_toolbar, ix, iy, cw)?;
        iy += 34.0;

        // 2. Toolbar Opacity (40% .. 100%) (Slider 23)
        let tb_op_frac = ((cfg.annotation.toolbar_opacity - 0.40) / 0.60).clamp(0.0, 1.0);
        self.render_slider(rt, "Floating Control Panel Opacity", &format!("{:.0} %", cfg.annotation.toolbar_opacity * 100.0), tb_op_frac, ix, iy, cw)?;
        iy += 38.0;

        // 3. Future Stroke Opacity (5% .. 100%) (Slider 24)
        let stroke_op_frac = ((cfg.annotation.stroke_opacity - 0.05) / 0.95).clamp(0.0, 1.0);
        self.render_slider(rt, "Future Stroke & Emoji Opacity", &format!("{:.0} %", cfg.annotation.stroke_opacity * 100.0), stroke_op_frac, ix, iy, cw)?;
        iy += 38.0;

        // 4. Current Stroke Width (Slider 20)
        let min_w = cfg.annotation.min_stroke_width;
        let max_w = cfg.annotation.max_stroke_width.max(min_w + 1.0);
        let curr_frac = ((cfg.annotation.stroke_width - min_w) / (max_w - min_w)).clamp(0.0, 1.0);
        self.render_slider(rt, "Active Stroke Width", &format!("{:.1} px", cfg.annotation.stroke_width), curr_frac, ix, iy, cw)?;
        iy += 38.0;

        // 5. Text Tool Font Family Pills
        let font_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
        let font_title: Vec<u16> = format!("Text Tool Font Family: {}", cfg.annotation.font_family).encode_utf16().collect();
        rt.DrawText(&font_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy, right: ix + cw, bottom: iy + 20.0 }, &font_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        iy += 20.0;

        let font_families = ["Segoe UI", "Arial", "Consolas", "Segoe Script", "Times New Roman"];
        let pill_w = 126.0;
        let pill_h = 28.0;
        let pill_gap = 10.0;
        for (f_idx, &fam) in font_families.iter().enumerate() {
            let px = ix + f_idx as f32 * (pill_w + pill_gap);
            let p_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: px, top: iy, right: px + pill_w, bottom: iy + pill_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let is_sel = cfg.annotation.font_family == fam;
            let (bg_c, border_c, text_c) = if is_sel {
                (
                    D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 },
                    D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: 1.0 },
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                )
            } else {
                (
                    D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.9 },
                    D2D1_COLOR_F { r: 0.24, g: 0.27, b: 0.35, a: 0.7 },
                    D2D1_COLOR_F { r: 0.75, g: 0.78, b: 0.85, a: 1.0 },
                )
            };
            let b_brush = rt.CreateSolidColorBrush(&bg_c, None)?;
            rt.FillRoundedRectangle(&p_rect, &b_brush);
            let brd_brush = rt.CreateSolidColorBrush(&border_c, None)?;
            rt.DrawRoundedRectangle(&p_rect, &brd_brush, if is_sel { 1.5 } else { 1.0 }, None);

            let txt_brush = rt.CreateSolidColorBrush(&text_c, None)?;
            let fam_wide: Vec<u16> = fam.encode_utf16().collect();
            let text_box = D2D_RECT_F { left: px, top: iy + 5.0, right: px + pill_w, bottom: iy + pill_h };
            rt.DrawText(&fam_wide, &self.text_format_tab, &text_box, &txt_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
        iy += 36.0;

        // 6. Text Font Size (12.0 .. 72.0 px) (Slider 25)
        let font_sz_frac = ((cfg.annotation.font_size - 12.0) / 60.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Text Font Size", &format!("{:.0} px", cfg.annotation.font_size), font_sz_frac, ix, iy, cw)?;
        iy += 38.0;

        // 7. Min Stroke Width (0.5 .. 10.0 px) (Slider 21)
        let min_w_frac = ((cfg.annotation.min_stroke_width - 0.5) / 9.5).clamp(0.0, 1.0);
        self.render_slider(rt, "Minimum Stroke Width (0% on Control Panel slider)", &format!("{:.1} px", cfg.annotation.min_stroke_width), min_w_frac, ix, iy, cw)?;
        iy += 38.0;

        // 8. Max Stroke Width (15.0 .. 60.0 px) (Slider 22)
        let max_w_frac = ((cfg.annotation.max_stroke_width - 15.0) / 45.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Maximum Stroke Width (100% on Control Panel slider)", &format!("{:.1} px", cfg.annotation.max_stroke_width), max_w_frac, ix, iy, cw)?;
        iy += 38.0;

        // 9. Dashed Line Hyphen Width (0.0 .. 10.0 px) (Slider 26)
        let dash_frac = (cfg.annotation.dash_length / 10.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Dashed Line Hyphen Width (0.0 = Round Dots)", &format!("{:.1} px", cfg.annotation.dash_length), dash_frac, ix, iy, cw)?;

        // Card 2: Palette, Emoji Stamps, and Tools
        let card2_y = cy + card_h + 10.0;
        let card2_h = 214.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: card2_y, right: cx + card_w, bottom: card2_y + card2_h })?;

        let mut iy2 = card2_y + 12.0;

        // Drawing Color Presets
        let heading_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.96, a: 1.0 }, None)?;
        let color_name = crate::color_names::get_color_display_name(cfg.annotation.current_color);
        let hex = crate::color_names::get_hex_string(cfg.annotation.current_color);

        if cfg.sync_colors {
            let c_title: Vec<u16> = format!("Drawing Color: {} ({}) — Locked", color_name, hex).encode_utf16().collect();
            let dim_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.65, g: 0.68, b: 0.75, a: 1.0 }, None)?;
            rt.DrawText(&c_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy2, right: ix + cw, bottom: iy2 + 20.0 }, &dim_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy2 += 18.0;

            let lock_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.50, g: 0.54, b: 0.65, a: 1.0 }, None)?;
            let lock_text: Vec<u16> = "🔒 Color is synced with Highlighter (uncheck 'Sync Color' in Highlighter tab to edit)".encode_utf16().collect();
            rt.DrawText(&lock_text, &self.text_format_small, &D2D_RECT_F { left: ix, top: iy2, right: ix + cw, bottom: iy2 + 16.0 }, &lock_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy2 += 18.0;
        } else {
            let c_title: Vec<u16> = format!("Active Drawing Color: {} ({})", color_name, hex).encode_utf16().collect();
            rt.DrawText(&c_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy2, right: ix + cw, bottom: iy2 + 20.0 }, &heading_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy2 += 18.0;

            let hint_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.55, g: 0.60, b: 0.70, a: 1.0 }, None)?;
            let hint_text: Vec<u16> = "Left-click: select  •  Right-click: open Windows Color Picker".encode_utf16().collect();
            rt.DrawText(&hint_text, &self.text_format_small, &D2D_RECT_F { left: ix, top: iy2, right: ix + cw, bottom: iy2 + 16.0 }, &hint_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            iy2 += 18.0;
        }

        // Color Swatches
        let swatch_size = 32.0;
        let swatch_gap = 12.0;
        let swatch_alpha = if cfg.sync_colors { 0.28 } else { 1.0 };

        for (idx, c) in cfg.annotation.color_presets.iter().enumerate() {
            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
            let s_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: sx, top: iy2, right: sx + swatch_size, bottom: iy2 + swatch_size },
                radiusX: 6.0,
                radiusY: 6.0,
            };

            let s_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: swatch_alpha }, None)?;
            rt.FillRoundedRectangle(&s_rect, &s_brush);

            if !cfg.sync_colors {
                let is_sel = (c[0] - cfg.annotation.current_color[0]).abs() < 0.03
                    && (c[1] - cfg.annotation.current_color[1]).abs() < 0.03
                    && (c[2] - cfg.annotation.current_color[2]).abs() < 0.03;

                if is_sel {
                    let border_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
                    rt.DrawRoundedRectangle(&s_rect, &border_sel, 2.5, None);
                } else {
                    let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }, None)?;
                    rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
                }
            } else {
                let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.3, g: 0.3, b: 0.35, a: 0.2 }, None)?;
                rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
            }
        }
        iy2 += 36.0;

        // Custom Emoji Stamps Section (up to 9 custom emojis)
        let e_title: Vec<u16> = "Custom Emoji Stamps (Max 9) • Left-click: select active • Right-click: delete".encode_utf16().collect();
        rt.DrawText(&e_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy2, right: ix + cw, bottom: iy2 + 20.0 }, &heading_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        iy2 += 22.0;

        let e_size = 32.0;
        let e_gap = 10.0;
        for (idx, emoji_char) in cfg.annotation.custom_emojis.iter().enumerate().take(9) {
            let ex = ix + idx as f32 * (e_size + e_gap);
            let ey = iy2;

            let e_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ex, top: ey, right: ex + e_size, bottom: ey + e_size },
                radiusX: 7.0,
                radiusY: 7.0,
            };

            let is_sel = cfg.annotation.selected_emoji == *emoji_char;
            let bg_color = if is_sel {
                D2D1_COLOR_F { r: 0.22, g: 0.32, b: 0.55, a: 1.0 }
            } else {
                D2D1_COLOR_F { r: 0.16, g: 0.18, b: 0.23, a: 1.0 }
            };
            let e_bg = rt.CreateSolidColorBrush(&bg_color, None)?;
            rt.FillRoundedRectangle(&e_rect, &e_bg);

            if is_sel {
                let e_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.45, g: 0.65, b: 1.0, a: 1.0 }, None)?;
                rt.DrawRoundedRectangle(&e_rect, &e_border, 2.0, None);
            }

            let e_text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let wide_e: Vec<u16> = emoji_char.encode_utf16().collect();
            let e_box = D2D_RECT_F { left: ex + 4.0, top: ey + 4.0, right: ex + e_size, bottom: ey + e_size };
            rt.DrawText(&wide_e, &self.text_format_emoji, &e_box, &e_text_brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, DWRITE_MEASURING_MODE_NATURAL);
        }
        iy2 += 38.0;

        // Custom Emoji Add Row
        let btn_x = ix + 128.0;
        let btn_y = iy2;
        let btn_w = 110.0;
        let btn_h = 26.0;
        let btn_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: btn_x, top: btn_y, right: btn_x + btn_w, bottom: btn_y + btn_h },
            radiusX: 6.0,
            radiusY: 6.0,
        };

        let can_add = cfg.annotation.custom_emojis.len() < 9;
        let (btn_bg_col, btn_text_col) = if can_add {
            (D2D1_COLOR_F { r: 0.22, g: 0.45, b: 0.88, a: 1.0 }, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
        } else {
            (D2D1_COLOR_F { r: 0.18, g: 0.20, b: 0.24, a: 0.8 }, D2D1_COLOR_F { r: 0.45, g: 0.48, b: 0.55, a: 1.0 })
        };
        let btn_brush = rt.CreateSolidColorBrush(&btn_bg_col, None)?;
        let btn_tb = rt.CreateSolidColorBrush(&btn_text_col, None)?;
        rt.FillRoundedRectangle(&btn_rect, &btn_brush);

        let add_label: Vec<u16> = "+ Add Emoji".encode_utf16().collect();
        let add_text_rect = D2D_RECT_F { left: btn_x, top: btn_y + 4.0, right: btn_x + btn_w, bottom: btn_y + btn_h };
        rt.DrawText(&add_label, &self.text_format_tab, &add_text_rect, &btn_tb, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Counter and help note
        let count_msg = if can_add {
            format!("{}/9 emojis  •  Type/paste & press Enter or click Add (Win+. for picker)", cfg.annotation.custom_emojis.len())
        } else {
            "9/9 emojis (Max limit reached — right-click an emoji to delete)".to_string()
        };
        let count_wide: Vec<u16> = count_msg.encode_utf16().collect();
        let count_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.55, g: 0.58, b: 0.65, a: 1.0 }, None)?;
        let count_rect = D2D_RECT_F { left: btn_x + btn_w + 12.0, top: btn_y + 5.0, right: ix + cw, bottom: btn_y + btn_h };
        rt.DrawText(&count_wide, &self.text_format_small, &count_rect, &count_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        iy2 += 34.0;

        // Emoji Style & Silhouette Options
        let opt_h = 26.0;

        // 1. Emoji Style: Flat vs Multicolor
        let style_title: Vec<u16> = "Emoji Style:".encode_utf16().collect();
        rt.DrawText(&style_title, &self.text_format_heading, &D2D_RECT_F { left: ix, top: iy2 + 3.0, right: ix + 90.0, bottom: iy2 + opt_h }, &heading_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let style_pills = [("Flat (Single Color)", "flat", 135.0), ("Multicolor", "color", 95.0)];
        let mut curr_x = ix + 95.0;
        for (label, val, pw) in style_pills {
            let p_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: curr_x, top: iy2, right: curr_x + pw, bottom: iy2 + opt_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let is_sel = cfg.annotation.emoji_style == val;
            let (bg_c, border_c, text_c) = if is_sel {
                (
                    D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 },
                    D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: 1.0 },
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                )
            } else {
                (
                    D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.9 },
                    D2D1_COLOR_F { r: 0.24, g: 0.27, b: 0.35, a: 0.7 },
                    D2D1_COLOR_F { r: 0.75, g: 0.78, b: 0.85, a: 1.0 },
                )
            };
            let b_brush = rt.CreateSolidColorBrush(&bg_c, None)?;
            rt.FillRoundedRectangle(&p_rect, &b_brush);
            let brd_brush = rt.CreateSolidColorBrush(&border_c, None)?;
            rt.DrawRoundedRectangle(&p_rect, &brd_brush, if is_sel { 1.5 } else { 1.0 }, None);

            let txt_brush = rt.CreateSolidColorBrush(&text_c, None)?;
            let lbl_wide: Vec<u16> = label.encode_utf16().collect();
            let text_box = D2D_RECT_F { left: curr_x, top: iy2 + 4.0, right: curr_x + pw, bottom: iy2 + opt_h };
            rt.DrawText(&lbl_wide, &self.text_format_tab, &text_box, &txt_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            curr_x += pw + 8.0;
        }

        // 2. Emoji Silhouette: Filled vs Hollow
        let sil_title_x = curr_x + 20.0;
        let sil_title: Vec<u16> = "Silhouette:".encode_utf16().collect();
        rt.DrawText(&sil_title, &self.text_format_heading, &D2D_RECT_F { left: sil_title_x, top: iy2 + 3.0, right: sil_title_x + 85.0, bottom: iy2 + opt_h }, &heading_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let sil_pills = [("Filled", true, 75.0), ("Hollow (Outline)", false, 120.0)];
        let mut curr_sil_x = sil_title_x + 85.0;
        for (label, val, pw) in sil_pills {
            let p_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: curr_sil_x, top: iy2, right: curr_sil_x + pw, bottom: iy2 + opt_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let is_sel = cfg.annotation.emoji_filled == val;
            let (bg_c, border_c, text_c) = if is_sel {
                (
                    D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 },
                    D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: 1.0 },
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                )
            } else {
                (
                    D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.9 },
                    D2D1_COLOR_F { r: 0.24, g: 0.27, b: 0.35, a: 0.7 },
                    D2D1_COLOR_F { r: 0.75, g: 0.78, b: 0.85, a: 1.0 },
                )
            };
            let b_brush = rt.CreateSolidColorBrush(&bg_c, None)?;
            rt.FillRoundedRectangle(&p_rect, &b_brush);
            let brd_brush = rt.CreateSolidColorBrush(&border_c, None)?;
            rt.DrawRoundedRectangle(&p_rect, &brd_brush, if is_sel { 1.5 } else { 1.0 }, None);

            let txt_brush = rt.CreateSolidColorBrush(&text_c, None)?;
            let lbl_wide: Vec<u16> = label.encode_utf16().collect();
            let text_box = D2D_RECT_F { left: curr_sil_x, top: iy2 + 4.0, right: curr_sil_x + pw, bottom: iy2 + opt_h };
            rt.DrawText(&lbl_wide, &self.text_format_tab, &text_box, &txt_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            curr_sil_x += pw + 8.0;
        }

        Ok(())
    }

    unsafe fn render_osd_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig, cx: f32, cy: f32) -> Result<()> {
        let card_w = 732.0;
        let card_h = 280.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: cy, right: cx + card_w, bottom: cy + card_h })?;

        let ix = cx + 22.0;
        let mut iy = cy + 18.0;
        let cw = card_w - 44.0;

        self.render_toggle(rt, "Enable Keystroke OSD (Active Monitor)", cfg.osd.enabled, ix, iy, cw)?;
        iy += 44.0;

        let stay_frac = ((cfg.osd.stay_duration_ms as f32 - 500.0) / 4500.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Stay Duration", &format!("{} ms", cfg.osd.stay_duration_ms), stay_frac, ix, iy, cw)?;
        iy += 48.0;

        let fade_frac = ((cfg.osd.fade_duration_ms as f32 - 100.0) / 900.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Fade Animation Duration", &format!("{} ms", cfg.osd.fade_duration_ms), fade_frac, ix, iy, cw)?;
        iy += 48.0;

        let font_frac = ((cfg.osd.font_size - 16.0) / 28.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Badge Font Size", &format!("{:.0} pt", cfg.osd.font_size), font_frac, ix, iy, cw)?;
        iy += 48.0;

        let radius_frac = (cfg.osd.border_radius / 25.0).clamp(0.0, 1.0);
        self.render_slider(rt, "Corner Roundness", &format!("{:.0} px", cfg.osd.border_radius), radius_frac, ix, iy, cw)?;

        // Live Preview Box
        let p_box_y = cy + card_h + 14.0;
        let p_box_h = 185.0;
        self.render_card(rt, D2D_RECT_F { left: cx, top: p_box_y, right: cx + card_w, bottom: p_box_y + p_box_h })?;

        let p_title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.60, g: 0.64, b: 0.72, a: 1.0 }, None)?;
        let p_wide: Vec<u16> = "LIVE PREVIEW (Keystroke HUD Badge)".encode_utf16().collect();
        rt.DrawText(&p_wide, &self.text_format_small, &D2D_RECT_F { left: cx + 20.0, top: p_box_y + 12.0, right: cx + card_w, bottom: p_box_y + 30.0 }, &p_title_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Render live sample badges: [Ctrl] [C]
        let badge_y = p_box_y + 65.0;
        let sample_keys = ["Ctrl + C", "Alt + Tab"];
        let mut bx = cx + 80.0;

        for text in sample_keys {
            let bw = 140.0;
            let bh = 50.0;
            let b_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: bx, top: badge_y, right: bx + bw, bottom: badge_y + bh },
                radiusX: cfg.osd.border_radius,
                radiusY: cfg.osd.border_radius,
            };

            let bg = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: cfg.osd.bg_color[0], g: cfg.osd.bg_color[1], b: cfg.osd.bg_color[2], a: cfg.osd.bg_color[3] },
                None,
            )?;
            rt.FillRoundedRectangle(&b_rect, &bg);

            let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 }, None)?;
            rt.DrawRoundedRectangle(&b_rect, &border, 1.5, None);

            let fg = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: cfg.osd.fg_color[0], g: cfg.osd.fg_color[1], b: cfg.osd.fg_color[2], a: cfg.osd.fg_color[3] },
                None,
            )?;
            let wide_txt: Vec<u16> = text.encode_utf16().collect();
            let text_rect = D2D_RECT_F { left: bx + 10.0, top: badge_y + 12.0, right: bx + bw - 10.0, bottom: badge_y + bh };
            rt.DrawText(&wide_txt, &self.text_format_heading, &text_rect, &fg, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            bx += bw + 24.0;
        }

        Ok(())
    }

    pub fn handle_mouse_down(&mut self, x: f32, y: f32) {
        // 0. Footer Action Bar: Apply & Cancel
        let footer_y = (self.height as f32 - 48.0).max(752.0);
        let btn_h = 32.0;
        let apply_w = 110.0;
        let cancel_w = 95.0;
        let apply_x = self.width as f32 - 24.0 - apply_w;
        let cancel_x = apply_x - 12.0 - cancel_w;

        if y >= footer_y && y <= footer_y + btn_h {
            if x >= apply_x && x <= apply_x + apply_w {
                self.apply_changes();
                return;
            }
            if x >= cancel_x && x <= cancel_x + cancel_w {
                self.cancel_changes();
                return;
            }
        }

        // Reset Defaults button (Top Right: 606..756, 18..52)
        if y >= 18.0 && y <= 52.0 && x >= 606.0 && x <= 756.0 {
            if let Ok(mut c) = self.config.lock() {
                *c = AppConfig::default();
            }
            self.notify_change();
            return;
        }

        // Horizontal Tabs (68.0 .. 106.0)
        if y >= 68.0 && y <= 106.0 && x >= 24.0 && x <= 756.0 {
            let tabs = [
                SettingsTab::Highlighter,
                SettingsTab::Laser,
                SettingsTab::Annotation,
                SettingsTab::Osd,
            ];
            let tab_w = 177.0;
            let tab_gap = 8.0;
            for (i, tab) in tabs.iter().enumerate() {
                let tx = 24.0 + i as f32 * (tab_w + tab_gap);
                if x >= tx && x <= tx + tab_w {
                    self.active_tab = *tab;
                    self.update_tab_visibility();
                    self.trigger_repaint();
                    return;
                }
            }
        }

        let cx = 24.0;
        let cy = 118.0;
        let card_w = 732.0;
        let ix = cx + 22.0;
        let cw = card_w - 44.0;

        match self.active_tab {
            SettingsTab::Highlighter => {
                // Toggle 1: Enable highlighter
                if y >= cy + 12.0 && y <= cy + 42.0 && x >= ix && x <= ix + cw {
                    if let Ok(mut c) = self.config.lock() {
                        c.highlighter.enabled = !c.highlighter.enabled;
                        if c.highlighter.enabled {
                            c.laser.enabled = false;
                        }
                        let _ = c.save();
                    }
                    self.notify_change();
                    return;
                }
                // Slider 1: Size
                if y >= cy + 48.0 && y <= cy + 90.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(1);
                    self.update_slider(1, x, ix, cw);
                    return;
                }
                // Slider 2: Softness
                if y >= cy + 94.0 && y <= cy + 136.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(2);
                    self.update_slider(2, x, ix, cw);
                    return;
                }
                // Slider 3: Opacity
                if y >= cy + 140.0 && y <= cy + 182.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(3);
                    self.update_slider(3, x, ix, cw);
                    return;
                }
                // Color Swatches
                let swatch_y = 352.0;
                let swatch_size = 34.0;
                let swatch_gap = 12.0;
                if y >= swatch_y && y <= swatch_y + swatch_size {
                    let presets = if let Ok(c) = self.config.lock() {
                        c.annotation.color_presets.clone()
                    } else {
                        Vec::new()
                    };
                    for (idx, &col) in presets.iter().enumerate() {
                        let sx = ix + idx as f32 * (swatch_size + swatch_gap);
                        if x >= sx && x <= sx + swatch_size {
                            if let Ok(mut c) = self.config.lock() {
                                c.highlighter.color = col;
                                if c.sync_colors {
                                    c.laser.color = col;
                                    c.annotation.current_color = col;
                                }
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                }
                // Toggle 2: Sync Colors
                if y >= 392.0 && y <= 424.0 && x >= ix && x <= ix + cw {
                    if let Ok(mut c) = self.config.lock() {
                        c.sync_colors = !c.sync_colors;
                        if c.sync_colors {
                            c.laser.color = c.highlighter.color;
                            c.annotation.current_color = c.highlighter.color;
                        }
                        let _ = c.save();
                    }
                    self.notify_change();
                    return;
                }
            }
            SettingsTab::Laser => {
                // Slider 10: Trail duration
                if y >= cy + 14.0 && y <= cy + 56.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(10);
                    self.update_slider(10, x, ix, cw);
                    return;
                }
                // Slider 11: Laser radius
                if y >= cy + 62.0 && y <= cy + 104.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(11);
                    self.update_slider(11, x, ix, cw);
                    return;
                }
                // Color Swatches (Only selectable when sync_colors is false)
                let is_synced = self.config.lock().map(|c| c.sync_colors).unwrap_or(false);
                if !is_synced {
                    let swatch_y = 272.0;
                    let swatch_size = 34.0;
                    let swatch_gap = 12.0;
                    if y >= swatch_y && y <= swatch_y + swatch_size {
                        let presets = if let Ok(c) = self.config.lock() {
                            c.annotation.color_presets.clone()
                        } else {
                            Vec::new()
                        };
                        for (idx, &col) in presets.iter().enumerate() {
                            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
                            if x >= sx && x <= sx + swatch_size {
                                if let Ok(mut c) = self.config.lock() {
                                    c.laser.color = col;
                                    let _ = c.save();
                                }
                                self.notify_change();
                                return;
                            }
                        }
                    }
                }
            }
            SettingsTab::Annotation => {
                // Toggle: Show floating toolbar
                if y >= cy + 10.0 && y <= cy + 42.0 && x >= ix && x <= ix + cw {
                    if let Ok(mut c) = self.config.lock() {
                        c.annotation.show_toolbar = !c.annotation.show_toolbar;
                        let _ = c.save();
                    }
                    self.notify_change();
                    return;
                }
                // Slider 23: Toolbar opacity
                if y >= cy + 44.0 && y <= cy + 84.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(23);
                    self.update_slider(23, x, ix, cw);
                    return;
                }
                // Slider 24: Stroke opacity
                if y >= cy + 86.0 && y <= cy + 122.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(24);
                    self.update_slider(24, x, ix, cw);
                    return;
                }
                // Slider 20: Stroke width
                if y >= cy + 124.0 && y <= cy + 160.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(20);
                    self.update_slider(20, x, ix, cw);
                    return;
                }
                // Font Family Pills
                let pill_y = cy + 182.0;
                let pill_h = 28.0;
                let pill_w = 126.0;
                let pill_gap = 10.0;
                if y >= pill_y && y <= pill_y + pill_h {
                    let font_families = ["Segoe UI", "Arial", "Consolas", "Segoe Script", "Times New Roman"];
                    for (f_idx, &fam) in font_families.iter().enumerate() {
                        let px = ix + f_idx as f32 * (pill_w + pill_gap);
                        if x >= px && x <= px + pill_w {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.font_family = fam.to_string();
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                }
                // Slider 25: Text Font Size
                if y >= cy + 218.0 && y <= cy + 254.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(25);
                    self.update_slider(25, x, ix, cw);
                    return;
                }
                // Slider 21: Min stroke width
                if y >= cy + 256.0 && y <= cy + 292.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(21);
                    self.update_slider(21, x, ix, cw);
                    return;
                }
                // Slider 22: Max stroke width
                if y >= cy + 294.0 && y <= cy + 332.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(22);
                    self.update_slider(22, x, ix, cw);
                    return;
                }
                // Slider 26: Dashed line hyphen width
                if y >= cy + 332.0 && y <= cy + 370.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(26);
                    self.update_slider(26, x, ix, cw);
                    return;
                }

                // Card 2 elements (card2_y = 522.0)
                let is_synced = self.config.lock().map(|c| c.sync_colors).unwrap_or(false);
                let swatch_y = 570.0;
                let swatch_size = 32.0;
                let swatch_gap = 12.0;

                if !is_synced && y >= swatch_y && y <= swatch_y + swatch_size {
                    let presets = if let Ok(c) = self.config.lock() {
                        c.annotation.color_presets.clone()
                    } else {
                        Vec::new()
                    };
                    for (idx, &col) in presets.iter().enumerate() {
                        let sx = ix + idx as f32 * (swatch_size + swatch_gap);
                        if x >= sx && x <= sx + swatch_size {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.current_color = col;
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                }

                // Emoji Swatches (up to 9 custom emojis)
                let emoji_y = 628.0;
                let e_size = 32.0;
                let e_gap = 10.0;
                if y >= emoji_y && y <= emoji_y + e_size {
                    let emojis = if let Ok(c) = self.config.lock() {
                        c.annotation.custom_emojis.clone()
                    } else {
                        Vec::new()
                    };
                    for (idx, emoji_char) in emojis.iter().enumerate().take(9) {
                        let ex = ix + idx as f32 * (e_size + e_gap);
                        if x >= ex && x <= ex + e_size {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.selected_emoji = emoji_char.clone();
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                }

                // [+ Add Emoji] button click (ix + 128.0 .. ix + 238.0, 666.0 .. 692.0)
                let btn_x = ix + 128.0;
                let btn_y = 666.0;
                let btn_w = 110.0;
                let btn_h = 26.0;
                if x >= btn_x && x <= btn_x + btn_w && y >= btn_y && y <= btn_y + btn_h {
                    self.add_emoji_from_input();
                    return;
                }

                // Emoji Style & Silhouette Options (opts_y = 700.0 .. 726.0)
                let opts_y = 700.0;
                let opt_h = 26.0;
                if y >= opts_y && y <= opts_y + opt_h {
                    // Emoji Style: Flat vs Multicolor
                    let flat_x1 = ix + 95.0;
                    let flat_x2 = flat_x1 + 135.0;
                    let color_x1 = flat_x2 + 8.0;
                    let color_x2 = color_x1 + 95.0;

                    if x >= flat_x1 && x <= flat_x2 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_style = "flat".to_string();
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                    if x >= color_x1 && x <= color_x2 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_style = "color".to_string();
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }

                    // Emoji Silhouette: Filled vs Hollow
                    let filled_x1 = color_x2 + 28.0 + 85.0;
                    let filled_x2 = filled_x1 + 75.0;
                    let hollow_x1 = filled_x2 + 8.0;
                    let hollow_x2 = hollow_x1 + 120.0;

                    if x >= filled_x1 && x <= filled_x2 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_filled = true;
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                    if x >= hollow_x1 && x <= hollow_x2 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_filled = false;
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                }
            }
            SettingsTab::Osd => {
                // Toggle 30: Enable OSD
                if y >= cy + 14.0 && y <= cy + 44.0 && x >= ix && x <= ix + cw {
                    if let Ok(mut c) = self.config.lock() {
                        c.osd.enabled = !c.osd.enabled;
                        let _ = c.save();
                    }
                    self.notify_change();
                    return;
                }
                // Slider 31: Stay duration
                if y >= cy + 50.0 && y <= cy + 92.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(31);
                    self.update_slider(31, x, ix, cw);
                    return;
                }
                // Slider 32: Fade duration
                if y >= cy + 98.0 && y <= cy + 140.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(32);
                    self.update_slider(32, x, ix, cw);
                    return;
                }
                // Slider 33: Font size
                if y >= cy + 146.0 && y <= cy + 188.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(33);
                    self.update_slider(33, x, ix, cw);
                    return;
                }
                // Slider 34: Border radius
                if y >= cy + 194.0 && y <= cy + 236.0 && x >= ix && x <= ix + cw {
                    self.is_dragging_slider = Some(34);
                    self.update_slider(34, x, ix, cw);
                    return;
                }
            }
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32) {
        if let Some(slider_id) = self.is_dragging_slider {
            let cx = 24.0;
            let card_w = 732.0;
            let ix = cx + 22.0;
            let cw = card_w - 44.0;
            self.update_slider(slider_id, x, ix, cw);
        }
    }

    pub fn handle_right_click(&mut self, x: f32, y: f32) {
        let cx = 24.0;
        let ix = cx + 22.0;

        match self.active_tab {
            SettingsTab::Highlighter => {
                let swatch_y = 352.0;
                let swatch_size = 34.0;
                let swatch_gap = 12.0;
                if y >= swatch_y && y <= swatch_y + swatch_size {
                    let (swatch_col, target_idx) = {
                        let c = match self.config.lock() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        let mut found = None;
                        for (idx, &col) in c.annotation.color_presets.iter().enumerate() {
                            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
                            if x >= sx && x <= sx + swatch_size {
                                found = Some((col, idx));
                                break;
                            }
                        }
                        match found {
                            Some(f) => f,
                            None => return,
                        }
                    };

                    if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                        if let Ok(mut c) = self.config.lock() {
                            if target_idx < c.annotation.color_presets.len() {
                                c.annotation.color_presets[target_idx] = new_col;
                            }
                            c.highlighter.color = new_col;
                            if c.sync_colors {
                                c.laser.color = new_col;
                                c.annotation.current_color = new_col;
                            }
                            let _ = c.save();
                        }
                        self.notify_change();
                    }
                }
            }
            SettingsTab::Laser => {
                let swatch_y = 272.0;
                let swatch_size = 34.0;
                let swatch_gap = 12.0;
                if y >= swatch_y && y <= swatch_y + swatch_size {
                    let (swatch_col, target_idx) = {
                        let c = match self.config.lock() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        let mut found = None;
                        for (idx, &col) in c.annotation.color_presets.iter().enumerate() {
                            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
                            if x >= sx && x <= sx + swatch_size {
                                found = Some((col, idx));
                                break;
                            }
                        }
                        match found {
                            Some(f) => f,
                            None => return,
                        }
                    };

                    if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                        if let Ok(mut c) = self.config.lock() {
                            if target_idx < c.annotation.color_presets.len() {
                                c.annotation.color_presets[target_idx] = new_col;
                            }
                            c.laser.color = new_col;
                            if c.sync_colors {
                                c.highlighter.color = new_col;
                                c.annotation.current_color = new_col;
                            }
                            let _ = c.save();
                        }
                        self.notify_change();
                    }
                }
            }
            SettingsTab::Annotation => {
                let swatch_y = 570.0;
                let swatch_size = 32.0;
                let swatch_gap = 12.0;
                if y >= swatch_y && y <= swatch_y + swatch_size {
                    let (swatch_col, target_idx) = {
                        let c = match self.config.lock() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        let mut found = None;
                        for (idx, &col) in c.annotation.color_presets.iter().enumerate() {
                            let sx = ix + idx as f32 * (swatch_size + swatch_gap);
                            if x >= sx && x <= sx + swatch_size {
                                found = Some((col, idx));
                                break;
                            }
                        }
                        match found {
                            Some(f) => f,
                            None => return,
                        }
                    };

                    if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                        if let Ok(mut c) = self.config.lock() {
                            if target_idx < c.annotation.color_presets.len() {
                                c.annotation.color_presets[target_idx] = new_col;
                            }
                            c.annotation.current_color = new_col;
                            if c.sync_colors {
                                c.highlighter.color = new_col;
                                c.laser.color = new_col;
                            }
                            let _ = c.save();
                        }
                        self.notify_change();
                    }
                    return;
                }

                // Custom emoji slot right-click to delete emoji
                let emoji_y = 628.0;
                let e_size = 32.0;
                let e_gap = 10.0;
                if y >= emoji_y && y <= emoji_y + e_size {
                    let mut target_idx = None;
                    if let Ok(c) = self.config.lock() {
                        for (idx, _) in c.annotation.custom_emojis.iter().enumerate().take(9) {
                            let ex = ix + idx as f32 * (e_size + e_gap);
                            if x >= ex && x <= ex + e_size {
                                target_idx = Some(idx);
                                break;
                            }
                        }
                    }
                    if let Some(idx) = target_idx {
                        if let Ok(mut c) = self.config.lock() {
                            if c.annotation.custom_emojis.len() > 1 && idx < c.annotation.custom_emojis.len() {
                                let removed = c.annotation.custom_emojis.remove(idx);
                                if c.annotation.selected_emoji == removed {
                                    c.annotation.selected_emoji = c.annotation.custom_emojis[0].clone();
                                }
                                let _ = c.save();
                            }
                        }
                        self.notify_change();
                    }
                }
            }
            SettingsTab::Osd => {}
        }
    }

    pub fn handle_mouse_up(&mut self) {
        self.is_dragging_slider = None;
    }

    fn update_slider(&mut self, id: usize, mouse_x: f32, start_x: f32, width: f32) {
        let frac = ((mouse_x - start_x) / width).clamp(0.0, 1.0);
        if let Ok(mut c) = self.config.lock() {
            match id {
                1 => c.highlighter.size = 10.0 + frac * 90.0,
                2 => c.highlighter.edge_softness = frac,
                3 => c.highlighter.opacity = (frac * 0.9 + 0.1).clamp(0.1, 1.0),
                10 => c.laser.trail_duration_ms = (200.0 + frac * 1800.0) as u64,
                11 => c.laser.radius = 3.0 + frac * 17.0,
                20 => {
                    let min_w = c.annotation.min_stroke_width.min(c.annotation.max_stroke_width - 1.0);
                    let max_w = c.annotation.max_stroke_width.max(min_w + 1.0);
                    c.annotation.stroke_width = min_w + frac * (max_w - min_w);
                }
                21 => {
                    c.annotation.min_stroke_width = 0.5 + frac * 9.5;
                    if c.annotation.stroke_width < c.annotation.min_stroke_width {
                        c.annotation.stroke_width = c.annotation.min_stroke_width;
                    }
                }
                22 => {
                    c.annotation.max_stroke_width = 15.0 + frac * 45.0;
                    if c.annotation.stroke_width > c.annotation.max_stroke_width {
                        c.annotation.stroke_width = c.annotation.max_stroke_width;
                    }
                }
                23 => c.annotation.toolbar_opacity = (0.40 + frac * 0.60).clamp(0.40, 1.0),
                24 => c.annotation.stroke_opacity = (0.05 + frac * 0.95).clamp(0.05, 1.0),
                25 => c.annotation.font_size = (12.0 + frac * 60.0).round(),
                26 => c.annotation.dash_length = (frac * 10.0 * 10.0).round() / 10.0,
                31 => c.osd.stay_duration_ms = (500.0 + frac * 4500.0) as u64,
                32 => c.osd.fade_duration_ms = (100.0 + frac * 900.0) as u64,
                33 => c.osd.font_size = 16.0 + frac * 28.0,
                34 => c.osd.border_radius = frac * 25.0,
                _ => {}
            }
            let _ = c.save();
        }
        self.notify_change();
    }

    fn notify_change(&mut self) {
        self.has_unapplied_changes = true;
        if let Some(ref cb) = self.on_change {
            cb();
        }
        self.trigger_repaint();
    }

    fn trigger_repaint(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, FALSE);
        }
    }
}

unsafe extern "system" fn settings_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let settings = match SETTINGS_INSTANCE {
        Some(ref mut s) => s,
        None => return DefWindowProcW(hwnd, msg, wparam, lparam),
    };

    match msg {
        WM_ERASEBKGND => LRESULT(1),

        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as _);
            let _ = SetTextColor(hdc, COLORREF(0x00FFFFFF));
            let _ = SetBkColor(hdc, COLORREF(0x00241E1C));
            static mut DARK_BRUSH: HBRUSH = HBRUSH(std::ptr::null_mut());
            if DARK_BRUSH.0.is_null() {
                DARK_BRUSH = CreateSolidBrush(COLORREF(0x00241E1C));
            }
            LRESULT(DARK_BRUSH.0 as _)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = settings.render();
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let _ = SetCapture(hwnd);
            settings.handle_mouse_down(x, y);
            LRESULT(0)
        }

        WM_RBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            settings.handle_right_click(x, y);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            settings.handle_mouse_move(x);
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            settings.handle_mouse_up();
            LRESULT(0)
        }

        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                settings.cancel_changes();
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            settings.cancel_changes();
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_conversion_roundtrip() {
        let test_colors: [[f32; 3]; 5] = [
            [1.0, 0.0, 0.0],
            [0.22, 0.85, 0.35],
            [0.25, 0.55, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
        ];

        for &c in &test_colors {
            let r_u8 = (c[0] * 255.0).round().clamp(0.0, 255.0) as u32;
            let g_u8 = (c[1] * 255.0).round().clamp(0.0, 255.0) as u32;
            let b_u8 = (c[2] * 255.0).round().clamp(0.0, 255.0) as u32;
            let colorref = COLORREF(r_u8 | (g_u8 << 8) | (b_u8 << 16));

            let res = colorref.0;
            let r = (res & 0xFF) as f32 / 255.0;
            let g = ((res >> 8) & 0xFF) as f32 / 255.0;
            let b = ((res >> 16) & 0xFF) as f32 / 255.0;

            assert!((r - c[0]).abs() < 0.01);
            assert!((g - c[1]).abs() < 0.01);
            assert!((b - c[2]).abs() < 0.01);
        }
    }

    #[test]
    fn test_swatch_sync_logic() {
        let mut cfg = AppConfig::default();
        cfg.sync_colors = true;

        let new_col = [0.15, 0.85, 0.95]; // Cyan
        cfg.annotation.color_presets[0] = new_col;
        cfg.highlighter.color = new_col;
        if cfg.sync_colors {
            cfg.laser.color = new_col;
            cfg.annotation.current_color = new_col;
        }

        assert_eq!(cfg.highlighter.color, new_col);
        assert_eq!(cfg.laser.color, new_col);
        assert_eq!(cfg.annotation.current_color, new_col);
        assert_eq!(cfg.annotation.color_presets[0], new_col);

        // When sync is disabled, independent changes remain independent
        cfg.sync_colors = false;
        let laser_col = [0.95, 0.25, 0.25];
        cfg.laser.color = laser_col;

        assert_eq!(cfg.highlighter.color, new_col);
        assert_eq!(cfg.laser.color, laser_col);
        assert_eq!(cfg.annotation.current_color, new_col);
    }

    #[test]
    fn test_slider_percentage_mapping() {
        let min_w: f32 = 1.0;
        let max_w: f32 = 35.0;
        let frac: f32 = 0.5;
        let stroke_w: f32 = min_w + frac * (max_w - min_w);
        assert!((stroke_w - 18.0f32).abs() < 0.001);

        let toolbar_opacity_frac: f32 = 0.5;
        let opacity: f32 = 0.40 + toolbar_opacity_frac * 0.60;
        assert!((opacity - 0.70f32).abs() < 0.001);
    }

    #[test]
    fn test_custom_emojis_cap_and_delete() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.annotation.custom_emojis.len(), 9);

        // Trying to add a 10th emoji when at max 9 must be rejected
        let can_add = cfg.annotation.custom_emojis.len() < 9;
        assert!(!can_add);

        // Remove one emoji
        let removed = cfg.annotation.custom_emojis.remove(2);
        assert_eq!(cfg.annotation.custom_emojis.len(), 8);
        assert_eq!(removed, "✔️");

        // Now adding a new emoji succeeds and stays capped at 9
        let new_emoji = "🎉".to_string();
        if cfg.annotation.custom_emojis.len() < 9 {
            cfg.annotation.custom_emojis.push(new_emoji.clone());
            cfg.annotation.selected_emoji = new_emoji;
        }
        assert_eq!(cfg.annotation.custom_emojis.len(), 9);
        assert_eq!(cfg.annotation.selected_emoji, "🎉");
        assert!(cfg.annotation.custom_emojis.contains(&"🎉".to_string()));
    }

    #[test]
    fn test_annotation_new_config_options() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.annotation.dash_length, 3.0);
        assert_eq!(cfg.annotation.emoji_style, "flat");
        assert!(cfg.annotation.emoji_filled);
    }

    #[test]
    fn test_settings_apply_and_cancel_logic() {
        let original_cfg = AppConfig::default();
        let mut saved_cfg = original_cfg.clone();
        let mut working_cfg = original_cfg.clone();

        // User makes changes in settings dialog
        working_cfg.annotation.dash_length = 7.5;
        working_cfg.highlighter.size = 85.0;
        let mut has_unapplied_changes = true;

        // Cancel restores saved config
        if has_unapplied_changes {
            working_cfg = saved_cfg.clone();
            has_unapplied_changes = false;
        }
        assert_eq!(working_cfg.annotation.dash_length, original_cfg.annotation.dash_length);
        assert_eq!(working_cfg.highlighter.size, original_cfg.highlighter.size);
        assert!(!has_unapplied_changes);

        // User makes another change and clicks Apply
        working_cfg.annotation.dash_length = 5.0;
        has_unapplied_changes = true;
        assert!(has_unapplied_changes);
        saved_cfg = working_cfg.clone();
        has_unapplied_changes = false;

        assert_eq!(saved_cfg.annotation.dash_length, 5.0);
        assert_eq!(working_cfg.annotation.dash_length, 5.0);
        assert!(!has_unapplied_changes);
    }
}
