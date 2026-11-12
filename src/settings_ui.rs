use crate::config::AppConfig;
use crate::renderer::enumerate_monitors;

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

#[derive(Clone, Copy)]
struct SliderSpec {
    label: &'static str,
    unit: &'static str,
    min: f32,
    max: f32,
    decimals: usize,
}

#[derive(Clone, Copy)]
struct SliderPlacement {
    id: usize,
    x: f32,
    y: f32,
    w: f32,
}

const SLIDER_VAL_BOX_W: f32 = 56.0;
const SLIDER_UNIT_W: f32 = 24.0;

const OSD_POSITIONS: [(&str, &str); 9] = [
    ("top_left", "Top Left"),
    ("top_center", "Top Center"),
    ("top_right", "Top Right"),
    ("middle_left", "Mid Left"),
    ("center", "Center"),
    ("middle_right", "Mid Right"),
    ("bottom_left", "Bot Left"),
    ("bottom_center", "Bot Center"),
    ("bottom_right", "Bot Right"),
];

pub fn parse_slider_input(raw: &str, min: f32, max: f32) -> Option<f32> {
    let cleaned: String = raw
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    let parsed = cleaned.parse::<f32>().ok()?;
    if !parsed.is_finite() {
        return None;
    }
    Some(parsed.clamp(min, max))
}

const CTRL_EDIT_EMOJI: usize = 1001;
const CTRL_EDIT_SLIDER: usize = 1002;
const EM_SETSEL_MSG: u32 = 0x00B1;
const EMOJI_EDIT_X: i32 = 406;
const EMOJI_EDIT_Y: i32 = 540;
const EMOJI_EDIT_W: i32 = 116;
const EMOJI_EDIT_H: i32 = 28;

static mut OLD_EMOJI_EDIT_WNDPROC: WNDPROC = None;
static mut OLD_SLIDER_EDIT_WNDPROC: WNDPROC = None;

unsafe extern "system" fn emoji_edit_subclass_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN && wparam.0 == VK_RETURN.0 as usize {
        if let Some(ref mut s) = SETTINGS_INSTANCE {
            s.add_emoji_from_input();
        }
        return LRESULT(0);
    }
    CallWindowProcW(OLD_EMOJI_EDIT_WNDPROC, hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn slider_edit_subclass_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        if wparam.0 == VK_RETURN.0 as usize || wparam.0 == VK_TAB.0 as usize {
            if let Some(ref mut s) = SETTINGS_INSTANCE {
                s.commit_slider_edit();
            }
            return LRESULT(0);
        }
        if wparam.0 == VK_ESCAPE.0 as usize {
            if let Some(ref mut s) = SETTINGS_INSTANCE {
                s.cancel_slider_edit();
            }
            return LRESULT(0);
        }
    }
    CallWindowProcW(OLD_SLIDER_EDIT_WNDPROC, hwnd, msg, wparam, lparam)
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    pub hwnd_edit_emoji: HWND,
    pub hwnd_edit_slider: HWND,
    pub config: Arc<Mutex<AppConfig>>,
    pub on_change: Option<Box<dyn Fn() + Send + 'static>>,

    d2d_factory: ID2D1Factory,
    _dwrite_factory: IDWriteFactory,
    render_target: Option<ID2D1HwndRenderTarget>,

    text_format_title: IDWriteTextFormat,
    text_format_heading: IDWriteTextFormat,
    text_format_body: IDWriteTextFormat,
    text_format_value: IDWriteTextFormat,
    text_format_small: IDWriteTextFormat,
    text_format_emoji: IDWriteTextFormat,
    text_format_tab: IDWriteTextFormat,

    active_tab: SettingsTab,
    width: u32,
    height: u32,
    is_dragging_slider: Option<usize>,
    editing_slider_id: Option<usize>,
    pub saved_config: AppConfig,
    pub has_unapplied_changes: bool,
}

impl SettingsWindow {
    pub fn open(config: Arc<Mutex<AppConfig>>, on_change: Option<Box<dyn Fn() + Send + 'static>>) -> Result<()> {
        unsafe {
            if let Some(ref mut instance) = SETTINGS_INSTANCE {
                if IsWindow(instance.hwnd).as_bool() {
                    instance.cancel_slider_edit();
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

            let dark_brush = CreateSolidBrush(COLORREF(0x001D1614));
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
            let primary_w = GetSystemMetrics(SM_CXSCREEN);
            let primary_h = GetSystemMetrics(SM_CYSCREEN);
            let height = 750.min((primary_h - 32).max(660));
            let x = (primary_w - width as i32) / 2;
            let y = (primary_h - height as i32) / 2;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                class_name,
                w!("ScreenTools - Preferences"),
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

            // Custom Emoji EDIT box
            let hwnd_edit_emoji = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                EMOJI_EDIT_X,
                EMOJI_EDIT_Y,
                EMOJI_EDIT_W,
                EMOJI_EDIT_H,
                hwnd,
                HMENU(CTRL_EDIT_EMOJI as _),
                instance,
                None,
            )?;

            let hfont_emoji = CreateFontW(
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
            let _ = SendMessageW(hwnd_edit_emoji, WM_SETFONT, WPARAM(hfont_emoji.0 as usize), LPARAM(1));
            let _ = SendMessageW(hwnd_edit_emoji, 0x1501, WPARAM(1), LPARAM(w!("Paste emoji...").as_ptr() as _));
            OLD_EMOJI_EDIT_WNDPROC = Some(std::mem::transmute(SetWindowLongPtrW(
                hwnd_edit_emoji,
                GWLP_WNDPROC,
                emoji_edit_subclass_wndproc as *const () as usize as isize,
            )));

            // Shared numeric slider EDIT box (positioned on click over any slider value box)
            let hwnd_edit_slider = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_BORDER | WINDOW_STYLE((ES_AUTOHSCROLL | ES_RIGHT) as u32),
                0,
                0,
                56,
                22,
                hwnd,
                HMENU(CTRL_EDIT_SLIDER as _),
                instance,
                None,
            )?;

            let hfont_ui = CreateFontW(
                16,
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
                w!("Segoe UI"),
            );
            let _ = SendMessageW(hwnd_edit_slider, WM_SETFONT, WPARAM(hfont_ui.0 as usize), LPARAM(1));
            OLD_SLIDER_EDIT_WNDPROC = Some(std::mem::transmute(SetWindowLongPtrW(
                hwnd_edit_slider,
                GWLP_WNDPROC,
                slider_edit_subclass_wndproc as *const () as usize as isize,
            )));

            crate::tray::apply_theme_to_menu(hwnd);

            let d2d_factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let text_format_title = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                17.0,
                w!("en-US"),
            )?;

            let text_format_heading = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.5,
                w!("en-US"),
            )?;

            let text_format_body = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.5,
                w!("en-US"),
            )?;

            let text_format_value = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("en-US"),
            )?;
            let _ = text_format_value.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
            let _ = text_format_value.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

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
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.5,
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
                hwnd_edit_slider,
                config,
                on_change,
                d2d_factory,
                _dwrite_factory: dwrite_factory,
                render_target: None,
                text_format_title,
                text_format_heading,
                text_format_body,
                text_format_value,
                text_format_small,
                text_format_emoji,
                text_format_tab,
                active_tab: SettingsTab::Highlighter,
                width: width as u32,
                height: height as u32,
                is_dragging_slider: None,
                editing_slider_id: None,
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
                let _ = SetWindowPos(
                    self.hwnd_edit_emoji,
                    HWND::default(),
                    EMOJI_EDIT_X,
                    EMOJI_EDIT_Y,
                    EMOJI_EDIT_W,
                    EMOJI_EDIT_H,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(self.hwnd_edit_emoji, SW_SHOW);
                let _ = InvalidateRect(self.hwnd_edit_emoji, None, TRUE);
            } else {
                let _ = ShowWindow(self.hwnd_edit_emoji, SW_HIDE);
            }
        }
    }

    fn slider_spec(id: usize, cfg: &AppConfig) -> Option<SliderSpec> {
        let min_w = cfg.annotation.min_stroke_width.min(cfg.annotation.max_stroke_width - 1.0);
        let max_w = cfg.annotation.max_stroke_width.max(min_w + 1.0);
        match id {
            1 => Some(SliderSpec { label: "Size / Diameter", unit: "px", min: 10.0, max: 100.0, decimals: 0 }),
            2 => Some(SliderSpec { label: "Edge Softness / Feather", unit: "%", min: 0.0, max: 100.0, decimals: 0 }),
            3 => Some(SliderSpec { label: "Glow Opacity", unit: "%", min: 10.0, max: 100.0, decimals: 0 }),

            10 => Some(SliderSpec { label: "Trail Duration", unit: "ms", min: 200.0, max: 2000.0, decimals: 0 }),
            11 => Some(SliderSpec { label: "Laser Dot Radius", unit: "px", min: 3.0, max: 20.0, decimals: 1 }),

            20 => Some(SliderSpec { label: "Active Stroke Width", unit: "px", min: min_w, max: max_w, decimals: 1 }),
            21 => Some(SliderSpec { label: "Min Stroke Width (0%)", unit: "px", min: 0.5, max: 10.0, decimals: 1 }),
            22 => Some(SliderSpec { label: "Max Stroke Width (100%)", unit: "px", min: 15.0, max: 60.0, decimals: 1 }),
            23 => Some(SliderSpec { label: "Control Panel Opacity", unit: "%", min: 40.0, max: 100.0, decimals: 0 }),
            24 => Some(SliderSpec { label: "Active Tool Opacity", unit: "%", min: 5.0, max: 100.0, decimals: 0 }),
            25 => Some(SliderSpec { label: "Text Font Size", unit: "pt", min: 12.0, max: 72.0, decimals: 0 }),
            26 => Some(SliderSpec { label: "Dashed Hyphen Width", unit: "px", min: 0.0, max: 10.0, decimals: 1 }),
            27 => Some(SliderSpec { label: "Edge Softness (Brush/Burn)", unit: "%", min: 0.0, max: 100.0, decimals: 0 }),

            31 => Some(SliderSpec { label: "Stay Duration", unit: "ms", min: 500.0, max: 5000.0, decimals: 0 }),
            32 => Some(SliderSpec { label: "Fade Duration", unit: "ms", min: 100.0, max: 1000.0, decimals: 0 }),
            33 => Some(SliderSpec { label: "Badge Font Size", unit: "pt", min: 16.0, max: 44.0, decimals: 0 }),
            34 => Some(SliderSpec { label: "Corner Roundness", unit: "px", min: 0.0, max: 25.0, decimals: 0 }),
            35 => Some(SliderSpec { label: "Background Opacity", unit: "%", min: 10.0, max: 100.0, decimals: 0 }),
            36 => Some(SliderSpec { label: "Screen Edge Offset", unit: "px", min: 0.0, max: 200.0, decimals: 0 }),
            _ => None,
        }
    }

    fn get_slider_value(id: usize, cfg: &AppConfig) -> f32 {
        match id {
            1 => cfg.highlighter.size,
            2 => cfg.highlighter.edge_softness * 100.0,
            3 => cfg.highlighter.opacity * 100.0,
            10 => cfg.laser.trail_duration_ms as f32,
            11 => cfg.laser.radius,
            20 => cfg.annotation.stroke_width,
            21 => cfg.annotation.min_stroke_width,
            22 => cfg.annotation.max_stroke_width,
            23 => cfg.annotation.toolbar_opacity * 100.0,
            24 => cfg.annotation.stroke_opacity * 100.0,
            25 => cfg.annotation.font_size,
            26 => cfg.annotation.dash_length,
            27 => cfg.annotation.stroke_softness * 100.0,
            31 => cfg.osd.stay_duration_ms as f32,
            32 => cfg.osd.fade_duration_ms as f32,
            33 => cfg.osd.font_size,
            34 => cfg.osd.border_radius,
            35 => cfg.osd.bg_color[3] * 100.0,
            36 => cfg.osd.edge_offset,
            _ => 0.0,
        }
    }

    fn set_slider_value(id: usize, c: &mut AppConfig, raw_val: f32) {
        let Some(spec) = Self::slider_spec(id, c) else {
            return;
        };
        let v = raw_val.clamp(spec.min, spec.max);
        match id {
            1 => c.highlighter.size = v,
            2 => c.highlighter.edge_softness = (v / 100.0).clamp(0.0, 1.0),
            3 => c.highlighter.opacity = (v / 100.0).clamp(0.1, 1.0),
            10 => c.laser.trail_duration_ms = v.round() as u64,
            11 => c.laser.radius = v,
            20 => {
                c.annotation.stroke_width = v;
                let t = c.annotation.selected_tool.clone();
                c.annotation.save_tool_settings(&t);
            }
            21 => {
                c.annotation.min_stroke_width = v;
                if c.annotation.stroke_width < c.annotation.min_stroke_width {
                    c.annotation.stroke_width = c.annotation.min_stroke_width;
                }
            }
            22 => {
                c.annotation.max_stroke_width = v;
                if c.annotation.stroke_width > c.annotation.max_stroke_width {
                    c.annotation.stroke_width = c.annotation.max_stroke_width;
                }
            }
            23 => c.annotation.toolbar_opacity = (v / 100.0).clamp(0.40, 1.0),
            24 => {
                c.annotation.stroke_opacity = (v / 100.0).clamp(0.05, 1.0);
                let t = c.annotation.selected_tool.clone();
                c.annotation.save_tool_settings(&t);
            }
            25 => c.annotation.font_size = v.round(),
            26 => c.annotation.dash_length = (v * 10.0).round() / 10.0,
            27 => {
                c.annotation.stroke_softness = (v / 100.0).clamp(0.0, 1.0);
                let t = c.annotation.selected_tool.clone();
                c.annotation.save_tool_settings(&t);
            }
            31 => c.osd.stay_duration_ms = v.round() as u64,
            32 => c.osd.fade_duration_ms = v.round() as u64,
            33 => c.osd.font_size = v,
            34 => c.osd.border_radius = v,
            35 => c.osd.bg_color[3] = (v / 100.0).clamp(0.10, 1.0),
            36 => c.osd.edge_offset = v,
            _ => {}
        }
    }

    fn format_slider_number(spec: &SliderSpec, val: f32) -> String {
        if spec.decimals == 0 {
            format!("{:.0}", val)
        } else {
            format!("{:.1}", val)
        }
    }

    fn tab_sliders(tab: SettingsTab) -> Vec<SliderPlacement> {
        let ix = 44.0f32;
        let cw = 692.0f32;
        let col_w = 334.0f32;
        let col2_x = ix + col_w + 24.0;
        match tab {
            SettingsTab::Highlighter => vec![
                SliderPlacement { id: 1, x: ix, y: 162.0, w: cw },
                SliderPlacement { id: 2, x: ix, y: 208.0, w: cw },
                SliderPlacement { id: 3, x: ix, y: 254.0, w: cw },
            ],
            SettingsTab::Laser => vec![
                SliderPlacement { id: 10, x: ix, y: 148.0, w: cw },
                SliderPlacement { id: 11, x: ix, y: 194.0, w: cw },
            ],
            SettingsTab::Annotation => vec![
                SliderPlacement { id: 23, x: ix, y: 156.0, w: col_w },
                SliderPlacement { id: 24, x: col2_x, y: 156.0, w: col_w },
                SliderPlacement { id: 20, x: ix, y: 200.0, w: col_w },
                SliderPlacement { id: 27, x: col2_x, y: 200.0, w: col_w },
                SliderPlacement { id: 21, x: ix, y: 244.0, w: col_w },
                SliderPlacement { id: 22, x: col2_x, y: 244.0, w: col_w },
                SliderPlacement { id: 26, x: ix, y: 288.0, w: col_w },
                SliderPlacement { id: 25, x: ix, y: 422.0, w: col_w },
            ],
            SettingsTab::Osd => vec![
                SliderPlacement { id: 31, x: ix, y: 156.0, w: col_w },
                SliderPlacement { id: 32, x: col2_x, y: 156.0, w: col_w },
                SliderPlacement { id: 33, x: ix, y: 204.0, w: col_w },
                SliderPlacement { id: 34, x: col2_x, y: 204.0, w: col_w },
                SliderPlacement { id: 35, x: ix, y: 286.0, w: cw },
                SliderPlacement { id: 36, x: 368.0, y: 428.0, w: 368.0 },
            ],
        }
    }

    fn begin_slider_edit(&mut self, sp: SliderPlacement) {
        self.commit_slider_edit();
        let val_str = if let Ok(c) = self.config.lock() {
            if let Some(spec) = Self::slider_spec(sp.id, &c) {
                Self::format_slider_number(&spec, Self::get_slider_value(sp.id, &c))
            } else {
                return;
            }
        } else {
            return;
        };

        self.editing_slider_id = Some(sp.id);
        let box_x = (sp.x + sp.w - SLIDER_UNIT_W - SLIDER_VAL_BOX_W).round() as i32;
        let box_y = sp.y.round() as i32;
        let box_w = SLIDER_VAL_BOX_W as i32;
        let box_h = 21;

        unsafe {
            let wide: Vec<u16> = val_str.encode_utf16().chain(Some(0)).collect();
            let _ = SetWindowTextW(self.hwnd_edit_slider, PCWSTR(wide.as_ptr()));
            let _ = SetWindowPos(
                self.hwnd_edit_slider,
                HWND::default(),
                box_x,
                box_y,
                box_w,
                box_h,
                SWP_NOZORDER,
            );
            let _ = ShowWindow(self.hwnd_edit_slider, SW_SHOW);
            let _ = SetFocus(self.hwnd_edit_slider);
            let _ = SendMessageW(self.hwnd_edit_slider, EM_SETSEL_MSG, WPARAM(0), LPARAM(-1));
        }
        self.trigger_repaint();
    }

    pub fn commit_slider_edit(&mut self) {
        let Some(slider_id) = self.editing_slider_id.take() else {
            return;
        };
        let mut updated = false;
        unsafe {
            let len = GetWindowTextLengthW(self.hwnd_edit_slider);
            if len > 0 {
                let mut buf = vec![0u16; (len + 1) as usize];
                let actual = GetWindowTextW(self.hwnd_edit_slider, &mut buf);
                if actual > 0 {
                    let text = String::from_utf16_lossy(&buf[..actual as usize]);
                    if let Ok(mut c) = self.config.lock() {
                        if let Some(spec) = Self::slider_spec(slider_id, &c) {
                            if let Some(clamped) = parse_slider_input(&text, spec.min, spec.max) {
                                Self::set_slider_value(slider_id, &mut c, clamped);
                                let _ = c.save();
                                updated = true;
                            }
                        }
                    }
                }
            }
            let _ = ShowWindow(self.hwnd_edit_slider, SW_HIDE);
            let _ = SetFocus(self.hwnd);
        }
        if updated {
            self.notify_change();
        } else {
            self.trigger_repaint();
        }
    }

    pub fn cancel_slider_edit(&mut self) {
        if self.editing_slider_id.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd_edit_slider, SW_HIDE);
                let _ = SetFocus(self.hwnd);
            }
            self.trigger_repaint();
        }
    }

    pub fn apply_changes(&mut self) {
        self.commit_slider_edit();
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
        self.cancel_slider_edit();
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

    fn footer_y(&self) -> f32 {
        (self.height as f32 - 48.0).max(648.0)
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
            // Calm slate dark background (#14161D)
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.078, g: 0.086, b: 0.114, a: 1.0 }));

            // 1. Header Bar
            let title_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.92, g: 0.94, b: 0.97, a: 1.0 }, None)?;
            let title_wide: Vec<u16> = "ScreenTools".encode_utf16().collect();
            rt.DrawText(
                &title_wide,
                &self.text_format_title,
                &D2D_RECT_F { left: 24.0, top: 14.0, right: 280.0, bottom: 38.0 },
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let sub_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.50, g: 0.54, b: 0.63, a: 1.0 }, None)?;
            let sub_wide: Vec<u16> = "Preferences & Tool Customization".encode_utf16().collect();
            rt.DrawText(
                &sub_wide,
                &self.text_format_small,
                &D2D_RECT_F { left: 24.0, top: 38.0, right: 320.0, bottom: 54.0 },
                &sub_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Reset Defaults Button (Top Right: 616..756, 18..48)
            let reset_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: 616.0, top: 18.0, right: 756.0, bottom: 48.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let reset_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.118, g: 0.133, b: 0.176, a: 1.0 }, None)?;
            let reset_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.18, g: 0.21, b: 0.28, a: 1.0 }, None)?;
            let reset_txt = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.78, g: 0.82, b: 0.90, a: 1.0 }, None)?;
            rt.FillRoundedRectangle(&reset_rect, &reset_bg);
            rt.DrawRoundedRectangle(&reset_rect, &reset_border, 1.0, None);
            let reset_wide: Vec<u16> = "↺ Reset Defaults".encode_utf16().collect();
            rt.DrawText(
                &reset_wide,
                &self.text_format_tab,
                &reset_rect.rect,
                &reset_txt,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 2. Navigation Tabs (62..96)
            let tabs = [
                (SettingsTab::Highlighter, "Highlighter"),
                (SettingsTab::Laser, "Laser Pointer"),
                (SettingsTab::Annotation, "Drawing & Tools"),
                (SettingsTab::Osd, "Keystroke OSD"),
            ];

            let tab_y = 62.0;
            let tab_h = 34.0;
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
                let (bg_col, brd_col, txt_col) = if is_active {
                    (
                        D2D1_COLOR_F { r: 0.16, g: 0.22, b: 0.36, a: 1.0 },
                        D2D1_COLOR_F { r: 0.32, g: 0.50, b: 0.88, a: 1.0 },
                        D2D1_COLOR_F { r: 0.94, g: 0.96, b: 1.0, a: 1.0 },
                    )
                } else {
                    (
                        D2D1_COLOR_F { r: 0.102, g: 0.114, b: 0.149, a: 1.0 },
                        D2D1_COLOR_F { r: 0.15, g: 0.17, b: 0.22, a: 1.0 },
                        D2D1_COLOR_F { r: 0.64, g: 0.68, b: 0.77, a: 1.0 },
                    )
                };
                let t_bg = rt.CreateSolidColorBrush(&bg_col, None)?;
                let t_brd = rt.CreateSolidColorBrush(&brd_col, None)?;
                let t_txt = rt.CreateSolidColorBrush(&txt_col, None)?;
                rt.FillRoundedRectangle(&tab_rect, &t_bg);
                rt.DrawRoundedRectangle(&tab_rect, &t_brd, if is_active { 1.3 } else { 1.0 }, None);

                let wide_tab: Vec<u16> = tab_name.encode_utf16().collect();
                rt.DrawText(
                    &wide_tab,
                    &self.text_format_tab,
                    &tab_rect.rect,
                    &t_txt,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // 3. Tab Content
            match self.active_tab {
                SettingsTab::Highlighter => self.render_highlighter_tab(rt, &cfg)?,
                SettingsTab::Laser => self.render_laser_tab(rt, &cfg)?,
                SettingsTab::Annotation => self.render_annotation_tab(rt, &cfg)?,
                SettingsTab::Osd => self.render_osd_tab(rt, &cfg)?,
            }

            // 4. Footer Bar: Status + Cancel + Apply
            let footer_y = self.footer_y();
            let btn_h = 32.0;

            let div_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.16, g: 0.18, b: 0.24, a: 1.0 }, None)?;
            rt.DrawLine(
                D2D_POINT_2F { x: 24.0, y: footer_y - 12.0 },
                D2D_POINT_2F { x: self.width as f32 - 24.0, y: footer_y - 12.0 },
                &div_brush,
                1.0,
                None,
            );

            let status_rect = D2D_RECT_F {
                left: 24.0,
                top: footer_y + 7.0,
                right: 500.0,
                bottom: footer_y + btn_h,
            };
            if self.has_unapplied_changes {
                let pend_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.90, g: 0.72, b: 0.32, a: 1.0 }, None)?;
                let pend_msg: Vec<u16> = "● Unsaved changes pending — click Apply to save".encode_utf16().collect();
                rt.DrawText(&pend_msg, &self.text_format_small, &status_rect, &pend_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            } else {
                let saved_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.45, g: 0.74, b: 0.56, a: 0.85 }, None)?;
                let saved_msg: Vec<u16> = "✓ All settings applied".encode_utf16().collect();
                rt.DrawText(&saved_msg, &self.text_format_small, &status_rect, &saved_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }

            let apply_w = 108.0;
            let cancel_w = 92.0;
            let apply_x = self.width as f32 - 24.0 - apply_w;
            let cancel_x = apply_x - 10.0 - cancel_w;

            let cancel_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cancel_x, top: footer_y, right: cancel_x + cancel_w, bottom: footer_y + btn_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let c_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.13, g: 0.15, b: 0.20, a: 1.0 }, None)?;
            let c_brd = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.24, g: 0.27, b: 0.36, a: 1.0 }, None)?;
            let c_txt = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.86, g: 0.89, b: 0.94, a: 1.0 }, None)?;
            rt.FillRoundedRectangle(&cancel_rect, &c_bg);
            rt.DrawRoundedRectangle(&cancel_rect, &c_brd, 1.0, None);
            let cancel_lbl: Vec<u16> = "Cancel".encode_utf16().collect();
            rt.DrawText(&cancel_lbl, &self.text_format_tab, &cancel_rect.rect, &c_txt, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            let apply_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: apply_x, top: footer_y, right: apply_x + apply_w, bottom: footer_y + btn_h },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let (a_bg_col, a_brd_col, a_txt_col) = if self.has_unapplied_changes {
                (
                    D2D1_COLOR_F { r: 0.22, g: 0.45, b: 0.86, a: 1.0 },
                    D2D1_COLOR_F { r: 0.40, g: 0.65, b: 0.98, a: 1.0 },
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                )
            } else {
                (
                    D2D1_COLOR_F { r: 0.12, g: 0.15, b: 0.22, a: 0.8 },
                    D2D1_COLOR_F { r: 0.20, g: 0.24, b: 0.32, a: 0.8 },
                    D2D1_COLOR_F { r: 0.52, g: 0.58, b: 0.68, a: 0.9 },
                )
            };
            let a_bg = rt.CreateSolidColorBrush(&a_bg_col, None)?;
            let a_brd = rt.CreateSolidColorBrush(&a_brd_col, None)?;
            let a_txt = rt.CreateSolidColorBrush(&a_txt_col, None)?;
            rt.FillRoundedRectangle(&apply_rect, &a_bg);
            rt.DrawRoundedRectangle(&apply_rect, &a_brd, 1.2, None);
            let apply_lbl: Vec<u16> = (if self.has_unapplied_changes { "Apply" } else { "Applied" }).encode_utf16().collect();
            rt.DrawText(&apply_lbl, &self.text_format_tab, &apply_rect.rect, &a_txt, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            rt.EndDraw(None, None)?;
        }
        Ok(())
    }

    unsafe fn render_card(&self, rt: &ID2D1HwndRenderTarget, rect: D2D_RECT_F) -> Result<()> {
        let rounded = D2D1_ROUNDED_RECT { rect, radiusX: 10.0, radiusY: 10.0 };
        let bg_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.102, g: 0.114, b: 0.149, a: 1.0 }, None)?;
        let border_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.155, g: 0.175, b: 0.23, a: 1.0 }, None)?;
        rt.FillRoundedRectangle(&rounded, &bg_brush);
        rt.DrawRoundedRectangle(&rounded, &border_brush, 1.0, None);
        Ok(())
    }

    unsafe fn render_section_title(&self, rt: &ID2D1HwndRenderTarget, title: &str, x: f32, y: f32, w: f32) -> Result<()> {
        let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.72, g: 0.79, b: 0.92, a: 1.0 }, None)?;
        let wide: Vec<u16> = title.encode_utf16().collect();
        rt.DrawText(
            &wide,
            &self.text_format_heading,
            &D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + 20.0 },
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        Ok(())
    }

    unsafe fn render_slider_by_placement(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig, sp: SliderPlacement) -> Result<()> {
        let Some(spec) = Self::slider_spec(sp.id, cfg) else {
            return Ok(());
        };
        let val = Self::get_slider_value(sp.id, cfg);
        let span = (spec.max - spec.min).max(1e-4);
        let frac = ((val - spec.min) / span).clamp(0.0, 1.0);
        let val_str = Self::format_slider_number(&spec, val);

        let text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.875, b: 0.925, a: 1.0 }, None)?;
        let unit_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.50, g: 0.54, b: 0.62, a: 1.0 }, None)?;

        let box_x = sp.x + sp.w - SLIDER_UNIT_W - SLIDER_VAL_BOX_W;
        let wide_label: Vec<u16> = spec.label.encode_utf16().collect();
        let label_rect = D2D_RECT_F { left: sp.x, top: sp.y + 1.0, right: box_x - 8.0, bottom: sp.y + 21.0 };
        rt.DrawText(&wide_label, &self.text_format_body, &label_rect, &text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Editable numeric input pill
        let is_editing = self.editing_slider_id == Some(sp.id);
        let val_box = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: box_x, top: sp.y, right: box_x + SLIDER_VAL_BOX_W, bottom: sp.y + 21.0 },
            radiusX: 5.0,
            radiusY: 5.0,
        };
        let box_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.071, g: 0.078, b: 0.106, a: 1.0 }, None)?;
        let box_brd = rt.CreateSolidColorBrush(
            &if is_editing {
                D2D1_COLOR_F { r: 0.30, g: 0.55, b: 0.96, a: 1.0 }
            } else {
                D2D1_COLOR_F { r: 0.17, g: 0.20, b: 0.27, a: 1.0 }
            },
            None,
        )?;
        rt.FillRoundedRectangle(&val_box, &box_bg);
        rt.DrawRoundedRectangle(&val_box, &box_brd, if is_editing { 1.4 } else { 1.0 }, None);

        if !is_editing {
            let val_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.56, g: 0.74, b: 0.98, a: 1.0 }, None)?;
            let wide_val: Vec<u16> = val_str.encode_utf16().collect();
            let text_pad = D2D_RECT_F {
                left: val_box.rect.left + 4.0,
                top: val_box.rect.top,
                right: val_box.rect.right - 6.0,
                bottom: val_box.rect.bottom,
            };
            rt.DrawText(&wide_val, &self.text_format_value, &text_pad, &val_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }

        let wide_unit: Vec<u16> = spec.unit.encode_utf16().collect();
        let unit_rect = D2D_RECT_F {
            left: sp.x + sp.w - SLIDER_UNIT_W + 4.0,
            top: sp.y + 2.0,
            right: sp.x + sp.w,
            bottom: sp.y + 21.0,
        };
        rt.DrawText(&wide_unit, &self.text_format_small, &unit_rect, &unit_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Slider track (rounded pill bar without circular thumb dot)
        let track_y = sp.y + 25.0;
        let track_h = 6.0;
        let track_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.16, g: 0.18, b: 0.24, a: 1.0 }, None)?;
        let track_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: sp.x, top: track_y, right: sp.x + sp.w, bottom: track_y + track_h },
            radiusX: 3.0,
            radiusY: 3.0,
        };
        rt.FillRoundedRectangle(&track_rect, &track_bg);

        let fill_w = (sp.w * frac).max(6.0).min(sp.w);
        let fill_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.27, g: 0.48, b: 0.88, a: 1.0 }, None)?;
        let fill_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: sp.x, top: track_y, right: sp.x + fill_w, bottom: track_y + track_h },
            radiusX: 3.0,
            radiusY: 3.0,
        };
        rt.FillRoundedRectangle(&fill_rect, &fill_bg);

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
        let text_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.875, b: 0.925, a: 1.0 }, None)?;
        let wide_label: Vec<u16> = label.encode_utf16().collect();
        let label_rect = D2D_RECT_F { left: x, top: y + 2.0, right: x + w - 54.0, bottom: y + 24.0 };
        rt.DrawText(&wide_label, &self.text_format_body, &label_rect, &text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let switch_w = 44.0;
        let switch_h = 22.0;
        let switch_x = x + w - switch_w;
        let switch_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: switch_x, top: y + 1.0, right: switch_x + switch_w, bottom: y + 1.0 + switch_h },
            radiusX: 11.0,
            radiusY: 11.0,
        };

        let (switch_bg, knob_x) = if enabled {
            (
                rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.24, g: 0.68, b: 0.42, a: 1.0 }, None)?,
                switch_x + switch_w - 11.0,
            )
        } else {
            (
                rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: 1.0 }, None)?,
                switch_x + 11.0,
            )
        };
        rt.FillRoundedRectangle(&switch_rect, &switch_bg);

        let knob_ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: knob_x, y: y + 12.0 },
            radiusX: 8.0,
            radiusY: 8.0,
        };
        let knob_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.96, g: 0.97, b: 1.0, a: 1.0 }, None)?;
        rt.FillEllipse(&knob_ellipse, &knob_brush);

        Ok(())
    }

    unsafe fn render_pill(
        &self,
        rt: &ID2D1HwndRenderTarget,
        rect: D2D_RECT_F,
        label: &str,
        selected: bool,
    ) -> Result<()> {
        let rounded = D2D1_ROUNDED_RECT { rect, radiusX: 6.0, radiusY: 6.0 };
        let (bg_c, brd_c, txt_c) = if selected {
            (
                D2D1_COLOR_F { r: 0.20, g: 0.36, b: 0.72, a: 1.0 },
                D2D1_COLOR_F { r: 0.40, g: 0.64, b: 0.96, a: 1.0 },
                D2D1_COLOR_F { r: 0.98, g: 0.99, b: 1.0, a: 1.0 },
            )
        } else {
            (
                D2D1_COLOR_F { r: 0.13, g: 0.15, b: 0.20, a: 1.0 },
                D2D1_COLOR_F { r: 0.21, g: 0.24, b: 0.32, a: 1.0 },
                D2D1_COLOR_F { r: 0.74, g: 0.78, b: 0.86, a: 1.0 },
            )
        };
        let b = rt.CreateSolidColorBrush(&bg_c, None)?;
        let brd = rt.CreateSolidColorBrush(&brd_c, None)?;
        let txt = rt.CreateSolidColorBrush(&txt_c, None)?;
        rt.FillRoundedRectangle(&rounded, &b);
        rt.DrawRoundedRectangle(&rounded, &brd, if selected { 1.3 } else { 1.0 }, None);
        let wide: Vec<u16> = label.encode_utf16().collect();
        rt.DrawText(&wide, &self.text_format_tab, &rect, &txt, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        Ok(())
    }

    unsafe fn render_color_swatches(
        &self,
        rt: &ID2D1HwndRenderTarget,
        presets: &[[f32; 3]],
        active_rgb: [f32; 3],
        locked: bool,
        start_x: f32,
        y: f32,
        size: f32,
        gap: f32,
    ) -> Result<()> {
        let alpha = if locked { 0.28 } else { 1.0 };
        for (idx, c) in presets.iter().enumerate() {
            let sx = start_x + idx as f32 * (size + gap);
            let s_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: sx, top: y, right: sx + size, bottom: y + size },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let s_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: alpha }, None)?;
            rt.FillRoundedRectangle(&s_rect, &s_brush);

            if !locked {
                let is_sel = (c[0] - active_rgb[0]).abs() < 0.035
                    && (c[1] - active_rgb[1]).abs() < 0.035
                    && (c[2] - active_rgb[2]).abs() < 0.035;
                if is_sel {
                    let border_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
                    rt.DrawRoundedRectangle(&s_rect, &border_sel, 2.2, None);
                } else {
                    let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }, None)?;
                    rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
                }
            } else {
                let border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.3, g: 0.3, b: 0.35, a: 0.2 }, None)?;
                rt.DrawRoundedRectangle(&s_rect, &border, 1.0, None);
            }
        }
        Ok(())
    }

    unsafe fn render_highlighter_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig) -> Result<()> {
        let cx = 24.0;
        let card_w = 732.0;
        let ix = 44.0;
        let cw = 692.0;

        // Card 1: Highlighter Controls (108..318)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 108.0, right: cx + card_w, bottom: 318.0 })?;
        self.render_toggle(rt, "Enable Mouse Cursor Highlighter", cfg.highlighter.enabled, ix, 124.0, cw)?;
        for sp in Self::tab_sliders(SettingsTab::Highlighter) {
            self.render_slider_by_placement(rt, cfg, sp)?;
        }

        // Card 2: Color & Sync (330..486)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 330.0, right: cx + card_w, bottom: 486.0 })?;
        let c_name = crate::color_names::get_color_display_name(cfg.highlighter.color);
        let hex = crate::color_names::get_hex_string(cfg.highlighter.color);
        self.render_section_title(rt, &format!("Highlighter Color — {} ({})", c_name, hex), ix, 344.0, cw)?;

        let hint_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.52, g: 0.56, b: 0.65, a: 1.0 }, None)?;
        let hint: Vec<u16> = "Left-click a swatch to select  •  Right-click or click Custom to open color picker".encode_utf16().collect();
        rt.DrawText(&hint, &self.text_format_small, &D2D_RECT_F { left: ix, top: 364.0, right: ix + cw, bottom: 380.0 }, &hint_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        self.render_color_swatches(rt, &cfg.annotation.color_presets, cfg.highlighter.color, false, ix, 386.0, 34.0, 12.0)?;
        self.render_pill(
            rt,
            D2D_RECT_F { left: ix + 8.0 * 46.0 + 4.0, top: 387.0, right: ix + 8.0 * 46.0 + 114.0, bottom: 419.0 },
            "Custom Color...",
            false,
        )?;

        self.render_toggle(rt, "Sync Color Across Modes (Highlighter, Laser & Drawing)", cfg.sync_colors, ix, 438.0, cw)?;
        Ok(())
    }

    unsafe fn render_laser_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig) -> Result<()> {
        let cx = 24.0;
        let card_w = 732.0;
        let ix = 44.0;
        let cw = 692.0;

        // Card 1: Beam & Trail (108..248)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 108.0, right: cx + card_w, bottom: 248.0 })?;
        self.render_section_title(rt, "Beam & Trail Timing", ix, 122.0, cw)?;
        for sp in Self::tab_sliders(SettingsTab::Laser) {
            self.render_slider_by_placement(rt, cfg, sp)?;
        }

        // Card 2: Color & Mode Tip (260..404)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 260.0, right: cx + card_w, bottom: 404.0 })?;
        let c_name = crate::color_names::get_color_display_name(cfg.laser.color);
        let hex = crate::color_names::get_hex_string(cfg.laser.color);
        let hint_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.52, g: 0.56, b: 0.65, a: 1.0 }, None)?;

        if cfg.sync_colors {
            self.render_section_title(rt, &format!("Laser Color — {} ({}) [Synced]", c_name, hex), ix, 274.0, cw)?;
            let lock_txt: Vec<u16> = "Color is synced with Highlighter (disable 'Sync Color' in Highlighter tab to customize separately)".encode_utf16().collect();
            rt.DrawText(&lock_txt, &self.text_format_small, &D2D_RECT_F { left: ix, top: 294.0, right: ix + cw, bottom: 310.0 }, &hint_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        } else {
            self.render_section_title(rt, &format!("Laser Color — {} ({})", c_name, hex), ix, 274.0, cw)?;
            let hint: Vec<u16> = "Left-click a swatch to select  •  Right-click or click Custom to open color picker".encode_utf16().collect();
            rt.DrawText(&hint, &self.text_format_small, &D2D_RECT_F { left: ix, top: 294.0, right: ix + cw, bottom: 310.0 }, &hint_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }

        self.render_color_swatches(rt, &cfg.annotation.color_presets, cfg.laser.color, cfg.sync_colors, ix, 316.0, 34.0, 12.0)?;
        if !cfg.sync_colors {
            self.render_pill(
                rt,
                D2D_RECT_F { left: ix + 8.0 * 46.0 + 4.0, top: 317.0, right: ix + 8.0 * 46.0 + 114.0, bottom: 349.0 },
                "Custom Color...",
                false,
            )?;
        }

        let tip_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.64, g: 0.69, b: 0.78, a: 1.0 }, None)?;
        let tip: Vec<u16> = "Tip: Left-click the tray icon to cycle Highlighter → Laser → Drawing mode.".encode_utf16().collect();
        rt.DrawText(&tip, &self.text_format_body, &D2D_RECT_F { left: ix, top: 366.0, right: ix + cw, bottom: 388.0 }, &tip_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        Ok(())
    }

    unsafe fn render_annotation_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig) -> Result<()> {
        let cx = 24.0;
        let card_w = 732.0;
        let ix = 44.0;
        let cw = 692.0;
        let col_w = 334.0;
        let col2_x = ix + col_w + 24.0;

        // Card 1: Panel & Brush Settings (108..336)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 108.0, right: cx + card_w, bottom: 336.0 })?;
        self.render_toggle(rt, "Show Floating Control Panel", cfg.annotation.show_toolbar, ix, 120.0, col_w)?;
        self.render_toggle(rt, "Sync Size / Opacity / Softness", cfg.annotation.sync_tool_settings, col2_x, 120.0, col_w)?;

        for sp in Self::tab_sliders(SettingsTab::Annotation) {
            if sp.id != 25 {
                self.render_slider_by_placement(rt, cfg, sp)?;
            }
        }

        let note_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.52, g: 0.56, b: 0.65, a: 1.0 }, None)?;
        let dash_note: Vec<u16> = "0.0 px draws round dots  •  > 0.2 px draws dashes".encode_utf16().collect();
        rt.DrawText(
            &dash_note,
            &self.text_format_small,
            &D2D_RECT_F { left: col2_x, top: 298.0, right: col2_x + col_w, bottom: 318.0 },
            &note_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        // Card 2: Text & Drawing Color (346..492)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 346.0, right: cx + card_w, bottom: 492.0 })?;
        self.render_section_title(rt, "Text Tool & Active Drawing Color", ix, 356.0, cw)?;

        let font_families = ["Segoe UI", "Arial", "Consolas", "Segoe Script", "Times New Roman"];
        let pill_w = 130.0;
        let pill_gap = 10.0;
        for (f_idx, &fam) in font_families.iter().enumerate() {
            let px = ix + f_idx as f32 * (pill_w + pill_gap);
            self.render_pill(
                rt,
                D2D_RECT_F { left: px, top: 380.0, right: px + pill_w, bottom: 406.0 },
                fam,
                cfg.annotation.font_family == fam,
            )?;
        }

        // Left half: Font size slider (id 25)
        self.render_slider_by_placement(rt, cfg, SliderPlacement { id: 25, x: ix, y: 422.0, w: col_w })?;

        // Right half: Drawing color swatches
        let label_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.875, b: 0.925, a: 1.0 }, None)?;
        let c_name = crate::color_names::get_color_display_name(cfg.annotation.current_color);
        let hex = crate::color_names::get_hex_string(cfg.annotation.current_color);
        let c_label = if cfg.sync_colors {
            format!("Color: {} ({}) [Synced]", c_name, hex)
        } else {
            format!("Color: {} ({})", c_name, hex)
        };
        let c_wide: Vec<u16> = c_label.encode_utf16().collect();
        rt.DrawText(
            &c_wide,
            &self.text_format_body,
            &D2D_RECT_F { left: col2_x, top: 418.0, right: col2_x + col_w, bottom: 438.0 },
            &label_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        self.render_color_swatches(rt, &cfg.annotation.color_presets, cfg.annotation.current_color, cfg.sync_colors, col2_x, 443.0, 26.0, 7.0)?;
        if !cfg.sync_colors {
            self.render_pill(
                rt,
                D2D_RECT_F { left: col2_x + 266.0, top: 443.0, right: col2_x + col_w, bottom: 469.0 },
                "Custom...",
                false,
            )?;
        }

        // Card 3: Emoji Stamps (502..642)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 502.0, right: cx + card_w, bottom: 642.0 })?;
        self.render_section_title(rt, "Emoji Stamps (Max 9) • Left-click: select • Right-click: remove", ix, 514.0, cw)?;

        let e_size = 32.0;
        let e_gap = 7.0;
        for (idx, emoji_char) in cfg.annotation.custom_emojis.iter().enumerate().take(9) {
            let ex = ix + idx as f32 * (e_size + e_gap);
            let ey = 538.0;
            let e_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ex, top: ey, right: ex + e_size, bottom: ey + e_size },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let is_sel = cfg.annotation.selected_emoji == *emoji_char;
            let bg_color = if is_sel {
                D2D1_COLOR_F { r: 0.20, g: 0.36, b: 0.72, a: 1.0 }
            } else {
                D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.21, a: 1.0 }
            };
            let e_bg = rt.CreateSolidColorBrush(&bg_color, None)?;
            rt.FillRoundedRectangle(&e_rect, &e_bg);
            if is_sel {
                let e_brd = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.42, g: 0.66, b: 0.98, a: 1.0 }, None)?;
                rt.DrawRoundedRectangle(&e_rect, &e_brd, 1.8, None);
            }
            let e_txt = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let wide_e: Vec<u16> = emoji_char.encode_utf16().collect();
            let e_box = D2D_RECT_F { left: ex + 4.0, top: ey + 4.0, right: ex + e_size, bottom: ey + e_size };
            rt.DrawText(&wide_e, &self.text_format_emoji, &e_box, &e_txt, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, DWRITE_MEASURING_MODE_NATURAL);
        }

        // Add Emoji Button & Counter next to hwnd_edit_emoji (x=406..522, y=540..568)
        let can_add = cfg.annotation.custom_emojis.len() < 9;
        let btn_x = 530.0;
        let btn_y = 540.0;
        let btn_w = 96.0;
        let btn_h = 28.0;
        let add_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: btn_x, top: btn_y, right: btn_x + btn_w, bottom: btn_y + btn_h },
            radiusX: 6.0,
            radiusY: 6.0,
        };
        let (add_bg, add_fg) = if can_add {
            (D2D1_COLOR_F { r: 0.22, g: 0.45, b: 0.86, a: 1.0 }, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
        } else {
            (D2D1_COLOR_F { r: 0.15, g: 0.17, b: 0.22, a: 1.0 }, D2D1_COLOR_F { r: 0.46, g: 0.50, b: 0.58, a: 1.0 })
        };
        let add_b = rt.CreateSolidColorBrush(&add_bg, None)?;
        let add_t = rt.CreateSolidColorBrush(&add_fg, None)?;
        rt.FillRoundedRectangle(&add_rect, &add_b);
        let add_lbl: Vec<u16> = "+ Add Emoji".encode_utf16().collect();
        rt.DrawText(&add_lbl, &self.text_format_tab, &add_rect.rect, &add_t, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let count_str = format!("{}/9  (Win+.)", cfg.annotation.custom_emojis.len());
        let count_w: Vec<u16> = count_str.encode_utf16().collect();
        rt.DrawText(
            &count_w,
            &self.text_format_small,
            &D2D_RECT_F { left: btn_x + btn_w + 10.0, top: btn_y + 6.0, right: ix + cw, bottom: btn_y + btn_h },
            &note_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        // Style & Silhouette Row (y = 590..618)
        let opt_y = 590.0;
        let opt_h = 28.0;
        let style_w: Vec<u16> = "Style:".encode_utf16().collect();
        rt.DrawText(&style_w, &self.text_format_body, &D2D_RECT_F { left: ix, top: opt_y + 4.0, right: ix + 48.0, bottom: opt_y + opt_h }, &label_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        self.render_pill(rt, D2D_RECT_F { left: ix + 48.0, top: opt_y, right: ix + 180.0, bottom: opt_y + opt_h }, "Flat (Single)", cfg.annotation.emoji_style == "flat")?;
        self.render_pill(rt, D2D_RECT_F { left: ix + 188.0, top: opt_y, right: ix + 286.0, bottom: opt_y + opt_h }, "Multicolor", cfg.annotation.emoji_style == "color")?;

        let sil_x = ix + 318.0;
        let sil_w: Vec<u16> = "Silhouette:".encode_utf16().collect();
        rt.DrawText(&sil_w, &self.text_format_body, &D2D_RECT_F { left: sil_x, top: opt_y + 4.0, right: sil_x + 76.0, bottom: opt_y + opt_h }, &label_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        self.render_pill(rt, D2D_RECT_F { left: sil_x + 76.0, top: opt_y, right: sil_x + 150.0, bottom: opt_y + opt_h }, "Filled", cfg.annotation.emoji_filled)?;
        self.render_pill(rt, D2D_RECT_F { left: sil_x + 158.0, top: opt_y, right: sil_x + 278.0, bottom: opt_y + opt_h }, "Hollow Outline", !cfg.annotation.emoji_filled)?;

        Ok(())
    }

    unsafe fn render_osd_tab(&self, rt: &ID2D1HwndRenderTarget, cfg: &AppConfig) -> Result<()> {
        let cx = 24.0;
        let card_w = 732.0;
        let ix = 44.0;
        let cw = 692.0;

        // Card 1: Visibility, Timing & Typography (108..262)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 108.0, right: cx + card_w, bottom: 262.0 })?;
        self.render_toggle(rt, "Enable Keystroke OSD Overlay", cfg.osd.enabled, ix, 120.0, cw)?;
        for sp in Self::tab_sliders(SettingsTab::Osd) {
            if sp.id != 35 && sp.id != 36 {
                self.render_slider_by_placement(rt, cfg, sp)?;
            }
        }

        // Card 2: Background Color & Opacity (274..404)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 274.0, right: cx + card_w, bottom: 404.0 })?;
        self.render_slider_by_placement(rt, cfg, SliderPlacement { id: 35, x: ix, y: 286.0, w: cw })?;

        let bg_rgb = [cfg.osd.bg_color[0], cfg.osd.bg_color[1], cfg.osd.bg_color[2]];
        let bg_name = crate::color_names::get_color_display_name(bg_rgb);
        let bg_hex = crate::color_names::get_hex_string(bg_rgb);
        self.render_section_title(rt, &format!("Background Color — {} ({})", bg_name, bg_hex), ix, 330.0, cw)?;

        // Slot 0 = Default Dark Slate (#14141A), Slots 1..=8 = color_presets, + Custom picker button
        let sw_y = 356.0;
        let sw_size = 32.0;
        let sw_gap = 10.0;
        let dark_default = [0.08f32, 0.08, 0.10];
        let mut all_swatches = Vec::with_capacity(1 + cfg.annotation.color_presets.len());
        all_swatches.push(dark_default);
        all_swatches.extend_from_slice(&cfg.annotation.color_presets);
        self.render_color_swatches(rt, &all_swatches, bg_rgb, false, ix, sw_y, sw_size, sw_gap)?;

        let custom_x = ix + all_swatches.len() as f32 * (sw_size + sw_gap) + 6.0;
        self.render_pill(
            rt,
            D2D_RECT_F { left: custom_x, top: sw_y, right: custom_x + 124.0, bottom: sw_y + sw_size },
            "Custom Color...",
            false,
        )?;

        // Card 3: Position, Edge Offset & Monitor (414..616)
        self.render_card(rt, D2D_RECT_F { left: cx, top: 414.0, right: cx + card_w, bottom: 616.0 })?;
        self.render_section_title(rt, "Screen Position", ix, 428.0, 294.0)?;

        let pos_grid_x = ix;
        let pos_grid_y = 454.0;
        let pos_cell_w = 94.0;
        let pos_cell_h = 36.0;
        let pos_gap = 6.0;
        for (idx, (key, label)) in OSD_POSITIONS.iter().enumerate() {
            let col = (idx % 3) as f32;
            let row = (idx / 3) as f32;
            let px = pos_grid_x + col * (pos_cell_w + pos_gap);
            let py = pos_grid_y + row * (pos_cell_h + pos_gap);
            self.render_pill(
                rt,
                D2D_RECT_F { left: px, top: py, right: px + pos_cell_w, bottom: py + pos_cell_h },
                label,
                cfg.osd.position == *key,
            )?;
        }

        // Right column of Card 3: Edge Offset slider + Target Monitor pills
        let right_x = 368.0;
        let right_w = 368.0;
        self.render_slider_by_placement(rt, cfg, SliderPlacement { id: 36, x: right_x, y: 428.0, w: right_w })?;
        self.render_section_title(rt, "Target Monitor", right_x, 476.0, right_w)?;

        let monitor_items = Self::build_monitor_options();
        let mon_pill_w = 179.0;
        let mon_pill_h = 32.0;
        let mon_col_gap = 10.0;
        let mon_row_gap = 8.0;
        let mon_start_y = 502.0;
        for (idx, (key, label)) in monitor_items.iter().enumerate().take(6) {
            let col = (idx % 2) as f32;
            let row = (idx / 2) as f32;
            let mx = right_x + col * (mon_pill_w + mon_col_gap);
            let my = mon_start_y + row * (mon_pill_h + mon_row_gap);
            self.render_pill(
                rt,
                D2D_RECT_F { left: mx, top: my, right: mx + mon_pill_w, bottom: my + mon_pill_h },
                label,
                cfg.osd.monitor == *key,
            )?;
        }

        Ok(())
    }

    fn build_monitor_options() -> Vec<(String, String)> {
        let mut opts = vec![
            ("active".to_string(), "Active (Cursor)".to_string()),
            ("primary".to_string(), "Primary Display".to_string()),
        ];
        for m in enumerate_monitors().into_iter().take(4) {
            let w = m.work_rect.right - m.work_rect.left;
            let h = m.work_rect.bottom - m.work_rect.top;
            let tag = if m.is_primary { " *" } else { "" };
            opts.push((
                format!("monitor_{}", m.index),
                format!("Display {}{} ({}×{})", m.index + 1, tag, w, h),
            ));
        }
        opts
    }

    pub fn handle_mouse_down(&mut self, x: f32, y: f32) {
        self.commit_slider_edit();

        // 0. Footer Action Bar: Cancel & Apply
        let footer_y = self.footer_y();
        let btn_h = 32.0;
        let apply_w = 108.0;
        let cancel_w = 92.0;
        let apply_x = self.width as f32 - 24.0 - apply_w;
        let cancel_x = apply_x - 10.0 - cancel_w;

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

        // Reset Defaults Button (616..756, 18..48)
        if y >= 18.0 && y <= 48.0 && x >= 616.0 && x <= 756.0 {
            self.cancel_slider_edit();
            if let Ok(mut c) = self.config.lock() {
                *c = AppConfig::default();
            }
            self.notify_change();
            return;
        }

        // Horizontal Tabs (62..96)
        if y >= 62.0 && y <= 96.0 && x >= 24.0 && x <= 756.0 {
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

        // Unified Slider Editable Value Box + Track Drag Hit-Testing
        for sp in Self::tab_sliders(self.active_tab) {
            let box_x = sp.x + sp.w - SLIDER_UNIT_W - SLIDER_VAL_BOX_W;
            if x >= box_x && x <= box_x + SLIDER_VAL_BOX_W && y >= sp.y && y <= sp.y + 21.0 {
                self.begin_slider_edit(sp);
                return;
            }
            if x >= sp.x && x <= sp.x + sp.w && y >= sp.y + 20.0 && y <= sp.y + 38.0 {
                self.is_dragging_slider = Some(sp.id);
                self.update_slider_from_mouse(sp.id, x, sp.x, sp.w);
                return;
            }
        }

        let ix = 44.0;
        let cw = 692.0;
        let col_w = 334.0;
        let col2_x = ix + col_w + 24.0;

        match self.active_tab {
            SettingsTab::Highlighter => {
                // Toggle 1: Enable Highlighter
                if y >= 120.0 && y <= 148.0 && x >= ix && x <= ix + cw {
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
                // Color Swatches + Custom Color button (y = 386..420)
                if y >= 386.0 && y <= 420.0 {
                    let presets = self.config.lock().map(|c| c.annotation.color_presets.clone()).unwrap_or_default();
                    for (idx, &col) in presets.iter().enumerate() {
                        let sx = ix + idx as f32 * 46.0;
                        if x >= sx && x <= sx + 34.0 {
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
                    let custom_x = ix + 8.0 * 46.0 + 4.0;
                    if x >= custom_x && x <= custom_x + 110.0 {
                        let init = self.config.lock().map(|c| c.highlighter.color).unwrap_or([1.0, 1.0, 1.0]);
                        if let Some(new_col) = open_system_color_picker(self.hwnd, init) {
                            if let Ok(mut c) = self.config.lock() {
                                c.highlighter.color = new_col;
                                if c.sync_colors {
                                    c.laser.color = new_col;
                                    c.annotation.current_color = new_col;
                                }
                                let _ = c.save();
                            }
                            self.notify_change();
                        }
                        return;
                    }
                }
                // Toggle 2: Sync Color
                if y >= 434.0 && y <= 464.0 && x >= ix && x <= ix + cw {
                    if let Ok(mut c) = self.config.lock() {
                        c.sync_colors = !c.sync_colors;
                        if c.sync_colors {
                            c.laser.color = c.highlighter.color;
                            c.annotation.current_color = c.highlighter.color;
                        }
                        let _ = c.save();
                    }
                    self.notify_change();
                }
            }
            SettingsTab::Laser => {
                let is_synced = self.config.lock().map(|c| c.sync_colors).unwrap_or(false);
                if !is_synced && y >= 316.0 && y <= 350.0 {
                    let presets = self.config.lock().map(|c| c.annotation.color_presets.clone()).unwrap_or_default();
                    for (idx, &col) in presets.iter().enumerate() {
                        let sx = ix + idx as f32 * 46.0;
                        if x >= sx && x <= sx + 34.0 {
                            if let Ok(mut c) = self.config.lock() {
                                c.laser.color = col;
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                    let custom_x = ix + 8.0 * 46.0 + 4.0;
                    if x >= custom_x && x <= custom_x + 110.0 {
                        let init = self.config.lock().map(|c| c.laser.color).unwrap_or([1.0, 1.0, 1.0]);
                        if let Some(new_col) = open_system_color_picker(self.hwnd, init) {
                            if let Ok(mut c) = self.config.lock() {
                                c.laser.color = new_col;
                                let _ = c.save();
                            }
                            self.notify_change();
                        }
                    }
                }
            }
            SettingsTab::Annotation => {
                // Row 1 side-by-side toggles (y = 120..146)
                if y >= 116.0 && y <= 146.0 {
                    if x >= ix && x <= ix + col_w {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.show_toolbar = !c.annotation.show_toolbar;
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                    if x >= col2_x && x <= col2_x + col_w {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.sync_tool_settings = !c.annotation.sync_tool_settings;
                            let tool_key = c.annotation.selected_tool.clone();
                            if !c.annotation.sync_tool_settings {
                                c.annotation.load_tool_settings(&tool_key);
                            } else {
                                c.annotation.save_tool_settings(&tool_key);
                            }
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                }

                // Font Family Pills (y = 380..406)
                if y >= 380.0 && y <= 406.0 {
                    let font_families = ["Segoe UI", "Arial", "Consolas", "Segoe Script", "Times New Roman"];
                    for (f_idx, &fam) in font_families.iter().enumerate() {
                        let px = ix + f_idx as f32 * 140.0;
                        if x >= px && x <= px + 130.0 {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.font_family = fam.to_string();
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                }

                // Drawing Color Swatches (right column of Card 2: y = 443..469)
                let is_synced = self.config.lock().map(|c| c.sync_colors).unwrap_or(false);
                if !is_synced && y >= 443.0 && y <= 469.0 && x >= col2_x {
                    let presets = self.config.lock().map(|c| c.annotation.color_presets.clone()).unwrap_or_default();
                    for (idx, &col) in presets.iter().enumerate() {
                        let sx = col2_x + idx as f32 * 33.0;
                        if x >= sx && x <= sx + 26.0 {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.invert_color = false;
                                c.annotation.current_color = col;
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                    if x >= col2_x + 266.0 && x <= col2_x + col_w {
                        let init = self.config.lock().map(|c| c.annotation.current_color).unwrap_or([1.0, 1.0, 1.0]);
                        if let Some(new_col) = open_system_color_picker(self.hwnd, init) {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.invert_color = false;
                                c.annotation.current_color = new_col;
                                let _ = c.save();
                            }
                            self.notify_change();
                        }
                        return;
                    }
                }

                // Custom Emoji Swatches (y = 538..570)
                if y >= 538.0 && y <= 570.0 {
                    let emojis = self.config.lock().map(|c| c.annotation.custom_emojis.clone()).unwrap_or_default();
                    for (idx, emoji_char) in emojis.iter().enumerate().take(9) {
                        let ex = ix + idx as f32 * 39.0;
                        if x >= ex && x <= ex + 32.0 {
                            if let Ok(mut c) = self.config.lock() {
                                c.annotation.selected_emoji = emoji_char.clone();
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }
                    // + Add Emoji button (x = 530..626, y = 540..568)
                    if x >= 530.0 && x <= 626.0 && y >= 540.0 && y <= 568.0 {
                        self.add_emoji_from_input();
                        return;
                    }
                }

                // Style & Silhouette Options (y = 590..618)
                if y >= 590.0 && y <= 618.0 {
                    if x >= ix + 48.0 && x <= ix + 180.0 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_style = "flat".to_string();
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                    if x >= ix + 188.0 && x <= ix + 286.0 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_style = "color".to_string();
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                    let sil_x = ix + 318.0;
                    if x >= sil_x + 76.0 && x <= sil_x + 150.0 {
                        if let Ok(mut c) = self.config.lock() {
                            c.annotation.emoji_filled = true;
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                    if x >= sil_x + 158.0 && x <= sil_x + 278.0 {
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
                // Toggle: Enable OSD (y = 120..146)
                if y >= 116.0 && y <= 146.0 && x >= ix && x <= ix + cw {
                    if let Ok(mut c) = self.config.lock() {
                        c.osd.enabled = !c.osd.enabled;
                        let _ = c.save();
                    }
                    self.notify_change();
                    return;
                }

                // Background Color Swatches (y = 356..388)
                if y >= 356.0 && y <= 388.0 {
                    let presets = self.config.lock().map(|c| c.annotation.color_presets.clone()).unwrap_or_default();
                    let mut all_swatches = Vec::with_capacity(1 + presets.len());
                    all_swatches.push([0.08f32, 0.08, 0.10]);
                    all_swatches.extend_from_slice(&presets);

                    for (idx, &col) in all_swatches.iter().enumerate() {
                        let sx = ix + idx as f32 * 42.0;
                        if x >= sx && x <= sx + 32.0 {
                            if let Ok(mut c) = self.config.lock() {
                                c.osd.bg_color[0] = col[0];
                                c.osd.bg_color[1] = col[1];
                                c.osd.bg_color[2] = col[2];
                                let _ = c.save();
                            }
                            self.notify_change();
                            return;
                        }
                    }

                    let custom_x = ix + all_swatches.len() as f32 * 42.0 + 6.0;
                    if x >= custom_x && x <= custom_x + 124.0 {
                        let init = self
                            .config
                            .lock()
                            .map(|c| [c.osd.bg_color[0], c.osd.bg_color[1], c.osd.bg_color[2]])
                            .unwrap_or([0.08, 0.08, 0.10]);
                        if let Some(new_col) = open_system_color_picker(self.hwnd, init) {
                            if let Ok(mut c) = self.config.lock() {
                                c.osd.bg_color[0] = new_col[0];
                                c.osd.bg_color[1] = new_col[1];
                                c.osd.bg_color[2] = new_col[2];
                                let _ = c.save();
                            }
                            self.notify_change();
                        }
                        return;
                    }
                }

                // 3x3 Screen Position Grid (x = 44..338, y = 454..574)
                for (idx, (key, _)) in OSD_POSITIONS.iter().enumerate() {
                    let col = (idx % 3) as f32;
                    let row = (idx / 3) as f32;
                    let px = ix + col * 100.0;
                    let py = 454.0 + row * 42.0;
                    if x >= px && x <= px + 94.0 && y >= py && y <= py + 36.0 {
                        if let Ok(mut c) = self.config.lock() {
                            c.osd.position = (*key).to_string();
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                }

                // Target Monitor Pills (right_x = 368, y = 502..)
                let monitor_items = Self::build_monitor_options();
                for (idx, (key, _)) in monitor_items.iter().enumerate().take(6) {
                    let col = (idx % 2) as f32;
                    let row = (idx / 2) as f32;
                    let mx = 368.0 + col * 189.0;
                    let my = 502.0 + row * 40.0;
                    if x >= mx && x <= mx + 179.0 && y >= my && y <= my + 32.0 {
                        if let Ok(mut c) = self.config.lock() {
                            c.osd.monitor = key.clone();
                            let _ = c.save();
                        }
                        self.notify_change();
                        return;
                    }
                }
            }
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32) {
        if let Some(slider_id) = self.is_dragging_slider {
            for sp in Self::tab_sliders(self.active_tab) {
                if sp.id == slider_id {
                    self.update_slider_from_mouse(slider_id, x, sp.x, sp.w);
                    break;
                }
            }
        }
    }

    pub fn handle_right_click(&mut self, x: f32, y: f32) {
        let ix = 44.0;
        let col2_x = ix + 358.0;

        match self.active_tab {
            SettingsTab::Highlighter => {
                if y >= 386.0 && y <= 420.0 {
                    let target = {
                        let Ok(guard) = self.config.lock() else { return };
                        guard.annotation.color_presets.iter().enumerate().find_map(|(idx, &col)| {
                            let sx = ix + idx as f32 * 46.0;
                            (x >= sx && x <= sx + 34.0).then_some((col, idx))
                        })
                    };
                    if let Some((swatch_col, idx)) = target {
                        if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                            if let Ok(mut c) = self.config.lock() {
                                if idx < c.annotation.color_presets.len() {
                                    c.annotation.color_presets[idx] = new_col;
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
            }
            SettingsTab::Laser => {
                if y >= 316.0 && y <= 350.0 {
                    let (target, is_synced) = {
                        let Ok(guard) = self.config.lock() else { return };
                        let found = guard.annotation.color_presets.iter().enumerate().find_map(|(idx, &col)| {
                            let sx = ix + idx as f32 * 46.0;
                            (x >= sx && x <= sx + 34.0).then_some((col, idx))
                        });
                        (found, guard.sync_colors)
                    };
                    if !is_synced {
                        if let Some((swatch_col, idx)) = target {
                            if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                                if let Ok(mut c) = self.config.lock() {
                                    if idx < c.annotation.color_presets.len() {
                                        c.annotation.color_presets[idx] = new_col;
                                    }
                                    c.laser.color = new_col;
                                    let _ = c.save();
                                }
                                self.notify_change();
                            }
                        }
                    }
                }
            }
            SettingsTab::Annotation => {
                if y >= 443.0 && y <= 469.0 {
                    let (target, is_synced) = {
                        let Ok(guard) = self.config.lock() else { return };
                        let found = guard.annotation.color_presets.iter().enumerate().find_map(|(idx, &col)| {
                            let sx = col2_x + idx as f32 * 33.0;
                            (x >= sx && x <= sx + 26.0).then_some((col, idx))
                        });
                        (found, guard.sync_colors)
                    };
                    if !is_synced {
                        if let Some((swatch_col, idx)) = target {
                            if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                                if let Ok(mut c) = self.config.lock() {
                                    if idx < c.annotation.color_presets.len() {
                                        c.annotation.color_presets[idx] = new_col;
                                    }
                                    c.annotation.invert_color = false;
                                    c.annotation.current_color = new_col;
                                    let _ = c.save();
                                }
                                self.notify_change();
                            }
                        }
                    }
                    return;
                }

                // Right-click custom emoji slot to delete
                if y >= 538.0 && y <= 570.0 {
                    let target_idx = if let Ok(c) = self.config.lock() {
                        (0..c.annotation.custom_emojis.len().min(9)).find(|&idx| {
                            let ex = ix + idx as f32 * 39.0;
                            x >= ex && x <= ex + 32.0
                        })
                    } else {
                        None
                    };
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
            SettingsTab::Osd => {
                // Right-click OSD preset swatch (idx 1..=8) to customize preset + OSD bg color
                if y >= 356.0 && y <= 388.0 {
                    let target = {
                        let Ok(guard) = self.config.lock() else { return };
                        (1..=guard.annotation.color_presets.len()).find_map(|slot| {
                            let sx = ix + slot as f32 * 42.0;
                            (x >= sx && x <= sx + 32.0).then_some((guard.annotation.color_presets[slot - 1], slot - 1))
                        })
                    };
                    if let Some((swatch_col, preset_idx)) = target {
                        if let Some(new_col) = open_system_color_picker(self.hwnd, swatch_col) {
                            if let Ok(mut c) = self.config.lock() {
                                if preset_idx < c.annotation.color_presets.len() {
                                    c.annotation.color_presets[preset_idx] = new_col;
                                }
                                c.osd.bg_color[0] = new_col[0];
                                c.osd.bg_color[1] = new_col[1];
                                c.osd.bg_color[2] = new_col[2];
                                let _ = c.save();
                            }
                            self.notify_change();
                        }
                    }
                }
            }
        }
    }

    pub fn handle_mouse_up(&mut self) {
        self.is_dragging_slider = None;
    }

    fn update_slider_from_mouse(&mut self, id: usize, mouse_x: f32, start_x: f32, width: f32) {
        let frac = ((mouse_x - start_x) / width.max(1.0)).clamp(0.0, 1.0);
        if let Ok(mut c) = self.config.lock() {
            if let Some(spec) = Self::slider_spec(id, &c) {
                let mut val = spec.min + frac * (spec.max - spec.min);
                if spec.decimals == 0 {
                    val = val.round();
                } else {
                    val = (val * 10.0).round() / 10.0;
                }
                Self::set_slider_value(id, &mut c, val);
                let _ = c.save();
            }
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
    #[allow(static_mut_refs)]
    let settings = match SETTINGS_INSTANCE {
        Some(ref mut s) => s,
        None => return DefWindowProcW(hwnd, msg, wparam, lparam),
    };

    match msg {
        WM_ERASEBKGND => LRESULT(1),

        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as _);
            let _ = SetTextColor(hdc, COLORREF(0x00F0EAE4));
            let _ = SetBkColor(hdc, COLORREF(0x001B1412));
            static mut DARK_BRUSH: HBRUSH = HBRUSH(std::ptr::null_mut());
            if DARK_BRUSH.0.is_null() {
                DARK_BRUSH = CreateSolidBrush(COLORREF(0x001B1412));
            }
            LRESULT(DARK_BRUSH.0 as _)
        }

        WM_COMMAND => {
            let ctrl_id = (wparam.0 & 0xFFFF) as usize;
            let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if ctrl_id == CTRL_EDIT_SLIDER && notify_code == EN_KILLFOCUS {
                settings.commit_slider_edit();
            }
            LRESULT(0)
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

        let new_col = [0.15, 0.85, 0.95];
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
        assert!(!(cfg.annotation.custom_emojis.len() < 9));

        let removed = cfg.annotation.custom_emojis.remove(2);
        assert_eq!(cfg.annotation.custom_emojis.len(), 8);
        assert_eq!(removed, "✔️");

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

        working_cfg.annotation.dash_length = 7.5;
        working_cfg.highlighter.size = 85.0;
        let mut has_unapplied_changes = true;

        if has_unapplied_changes {
            working_cfg = saved_cfg.clone();
            has_unapplied_changes = false;
        }
        assert_eq!(working_cfg.annotation.dash_length, original_cfg.annotation.dash_length);
        assert_eq!(working_cfg.highlighter.size, original_cfg.highlighter.size);
        assert!(!has_unapplied_changes);

        working_cfg.annotation.dash_length = 5.0;
        has_unapplied_changes = true;
        assert!(has_unapplied_changes);
        saved_cfg = working_cfg.clone();
        has_unapplied_changes = false;

        assert_eq!(saved_cfg.annotation.dash_length, 5.0);
        assert_eq!(working_cfg.annotation.dash_length, 5.0);
        assert!(!has_unapplied_changes);
    }

    #[test]
    fn test_slider_numeric_input_parsing_and_setters() {
        assert_eq!(parse_slider_input("42.5 px", 10.0, 100.0), Some(42.5));
        assert_eq!(parse_slider_input("150", 10.0, 100.0), Some(100.0));
        assert_eq!(parse_slider_input("-5%", 10.0, 100.0), Some(10.0));
        assert_eq!(parse_slider_input("abc", 10.0, 100.0), None);

        let mut cfg = AppConfig::default();
        SettingsWindow::set_slider_value(35, &mut cfg, 60.0);
        assert!((cfg.osd.bg_color[3] - 0.60).abs() < 0.001);

        SettingsWindow::set_slider_value(36, &mut cfg, 48.0);
        assert!((cfg.osd.edge_offset - 48.0).abs() < 0.001);
    }
}
