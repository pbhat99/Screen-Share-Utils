use crate::annotation::{ActiveTool, AnnotationManager, ScreenEffectMode};
use crate::config::AppConfig;
use crate::geometry::Point2D;
use crate::keystroke::{KeyHookManager, KeystrokeState};
use crate::renderer::D2DRenderer;
use crate::tray::*;

use std::sync::{Arc, Mutex};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, SetCapture, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_RETURN, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;

const TIMER_RENDER: usize = 1001;

pub struct App {
    pub hwnd_overlay: HWND,
    #[allow(dead_code)]
    pub hwnd_controller: HWND,
    pub config: AppConfig,
    pub shared_config: Arc<Mutex<AppConfig>>,
    pub renderer: D2DRenderer,
    pub annotation_mgr: AnnotationManager,
    pub active_tool: ActiveTool,
    pub is_drawing_mode: bool,
    pub is_mouse_down: bool,

    pub last_rendered_cursor: Point2D,
    pub needs_render: bool,
    pub last_osd_had_tokens: bool,
    pub topmost_tick_counter: usize,

    pub keystroke_state: Arc<Mutex<KeystrokeState>>,
    pub key_hook_mgr: KeyHookManager,
    pub tray: Option<TrayIcon>,

    pub pen_cursor: Option<HCURSOR>,
    pub eraser_cursor: Option<HCURSOR>,
    pub emoji_cursor: Option<HCURSOR>,
    pub cached_emoji_str: String,
    pub toolbar: crate::toolbar::DrawingToolbar,
}

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

static mut APP_INSTANCE: Option<*mut App> = None;

impl App {
    pub fn create_and_run() -> Result<()> {
        unsafe {
            let _ = timeBeginPeriod(1);
            let instance = GetModuleHandleW(None)?;

            // 1. Controller Window Class (Handles Tray Icon & Context Menus)
            let controller_class_name = w!("ScreenToolsControllerClass");
            let controller_wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(controller_wndproc),
                hInstance: instance.into(),
                lpszClassName: controller_class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&controller_wnd_class);

            let hwnd_controller = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                controller_class_name,
                w!("ScreenTools Controller"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )?;

            // 2. Fullscreen Transparent Overlay Window Class
            let overlay_class_name = w!("ScreenToolsOverlayClass");
            let overlay_wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(overlay_wndproc),
                hInstance: instance.into(),
                hCursor: HCURSOR::default(),
                lpszClassName: overlay_class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&overlay_wnd_class);

            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            // Overlay starts with WS_EX_TRANSPARENT | WS_EX_NOACTIVATE: 100% click-through by default!
            let hwnd_overlay = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                overlay_class_name,
                w!("ScreenTools Overlay"),
                WS_POPUP,
                vx,
                vy,
                vw,
                vh,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )?;

            let mut config = AppConfig::load();
            if config.sync_colors {
                config.laser.color = config.highlighter.color;
                config.annotation.current_color = config.highlighter.color;
            }
            // Mutually exclusive: default to highlighter mode if enabled, else whichever is enabled
            if config.highlighter.enabled {
                config.laser.enabled = false;
            } else if config.laser.enabled {
                config.highlighter.enabled = false;
            } else {
                config.highlighter.enabled = true;
            }
            let shared_config = Arc::new(Mutex::new(config.clone()));
            let renderer = D2DRenderer::new(hwnd_overlay, vx, vy, vw, vh)?;
            let annotation_mgr = AnnotationManager::new();

            let keystroke_state = Arc::new(Mutex::new(KeystrokeState::new()));
            let key_hook_mgr = KeyHookManager::new(Arc::clone(&keystroke_state));

            // 3. Register System Tray Icon attached to hwnd_controller
            let initial_tray_mode = if config.laser.enabled {
                TrayMode::Laser
            } else {
                TrayMode::Highlighter
            };
            let tray = TrayIcon::new(hwnd_controller, initial_tray_mode).ok();

            // 4. Initialize hardware cursors for drawing mode (Pen, Eraser, Emoji)
            let pen_cursor = crate::cursor::create_pen_cursor();
            let eraser_cursor = crate::cursor::create_eraser_cursor();
            let emoji_cursor = crate::cursor::create_emoji_cursor(
                &renderer.d2d_factory().clone().into(),
                renderer.dwrite_factory(),
                &config.annotation.selected_emoji,
            );
            let cached_emoji_str = config.annotation.selected_emoji.clone();
            let toolbar = crate::toolbar::DrawingToolbar::new(config.annotation.toolbar_x, config.annotation.toolbar_y);

            let mut app = Box::new(App {
                hwnd_overlay,
                hwnd_controller,
                config,
                shared_config,
                renderer,
                annotation_mgr,
                active_tool: ActiveTool::None,
                is_drawing_mode: false,
                is_mouse_down: false,
                last_rendered_cursor: Point2D::new(-9999.0, -9999.0),
                needs_render: true,
                last_osd_had_tokens: false,
                topmost_tick_counter: 0,
                keystroke_state,
                key_hook_mgr,
                tray,
                pen_cursor,
                eraser_cursor,
                emoji_cursor,
                cached_emoji_str,
                toolbar,
            });
            app.update_tray_tooltip();

            APP_INSTANCE = Some(Box::into_raw(app));

            // Low FPS render timer (30 FPS / ~33ms tick, lightweight and smooth, zero mouse lag)
            let _ = SetTimer(hwnd_controller, TIMER_RENDER, 33, None);

            let _ = ShowWindow(hwnd_overlay, SW_SHOWNA);
            let _ = UpdateWindow(hwnd_overlay);

            // Standard Windows Message Loop
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // Cleanup on application exit
            #[allow(static_mut_refs)]
            if let Some(app_ptr) = APP_INSTANCE.take() {
                let mut app = Box::from_raw(app_ptr);
                app.key_hook_mgr.stop();
                let _ = KillTimer(hwnd_controller, TIMER_RENDER);
                let _ = timeEndPeriod(1);
                if let Some(c) = app.pen_cursor.take() {
                    let _ = DestroyCursor(c);
                }
                if let Some(c) = app.eraser_cursor.take() {
                    let _ = DestroyCursor(c);
                }
                if let Some(c) = app.emoji_cursor.take() {
                    let _ = DestroyCursor(c);
                }
            }
        }
        Ok(())
    }

    /// Sync active config from shared config (e.g. when modified in Settings Window)
    pub fn sync_config(&mut self) {
        if let Ok(mut c) = self.shared_config.lock() {
            // Preserve live floating panel position so Settings Apply/Cancel cannot revert it
            c.annotation.toolbar_x = self.toolbar.x;
            c.annotation.toolbar_y = self.toolbar.y;
            self.config = c.clone();
        }
        self.update_emoji_cursor_if_changed();
        self.needs_render = true;
    }

    pub fn update_emoji_cursor_if_changed(&mut self) {
        if self.config.annotation.selected_emoji != self.cached_emoji_str || self.emoji_cursor.is_none() {
            unsafe {
                if let Some(old) = self.emoji_cursor.take() {
                    let _ = DestroyCursor(old);
                }
                self.emoji_cursor = crate::cursor::create_emoji_cursor(
                    &self.renderer.d2d_factory().clone().into(),
                    self.renderer.dwrite_factory(),
                    &self.config.annotation.selected_emoji,
                );
                self.cached_emoji_str = self.config.annotation.selected_emoji.clone();
            }
        }
    }

    /// Toggle Windows input click-through dynamically on the overlay window
    pub fn set_click_through(&self, click_through: bool) {
        unsafe {
            let mut ex = GetWindowLongPtrW(self.hwnd_overlay, GWL_EXSTYLE);
            if click_through {
                ex |= (WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0) as isize;
            } else {
                ex &= !((WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0) as isize);
            }
            SetWindowLongPtrW(self.hwnd_overlay, GWL_EXSTYLE, ex);
            let _ = SetWindowPos(
                self.hwnd_overlay,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
    }

    /// Cycle presentation modes: Highlighter -> Laser -> Drawing -> Highlighter (one at a time)
    pub fn cycle_presentation_mode(&mut self) {
        if self.is_drawing_mode {
            // From Drawing -> Highlighter
            self.set_drawing_mode(false);
            self.config.laser.enabled = false;
            self.config.highlighter.enabled = true;
        } else if self.config.laser.enabled {
            // From Laser -> Drawing
            self.config.laser.enabled = false;
            self.config.highlighter.enabled = false;
            self.set_drawing_mode(true);
        } else {
            // From Highlighter (or none) -> Laser
            self.set_drawing_mode(false);
            self.config.highlighter.enabled = false;
            self.config.laser.enabled = true;
        }
        if let Ok(mut c) = self.shared_config.lock() {
            *c = self.config.clone();
            let _ = c.save();
        }
        self.needs_render = true;
        self.update_tray_tooltip();
    }

    pub fn ensure_screen_snapshot(&mut self) -> usize {
        if self.renderer.active_snapshot_id == 0 {
            self.renderer.capture_desktop_snapshot(self.hwnd_overlay)
        } else {
            self.renderer.active_snapshot_id
        }
    }

    pub fn current_effect_mode(&self) -> ScreenEffectMode {
        match self.active_tool {
            ActiveTool::Burn => ScreenEffectMode::Burn,
            ActiveTool::Dodge => ScreenEffectMode::Dodge,
            _ if self.config.annotation.invert_color => ScreenEffectMode::Invert,
            _ => ScreenEffectMode::Normal,
        }
    }

    pub fn set_drawing_mode(&mut self, enabled: bool) {
        self.is_drawing_mode = enabled;
        self.toolbar.reset_confirmations();
        self.toolbar.close_flyouts();
        self.config.annotation.toolbar_x = self.toolbar.x;
        self.config.annotation.toolbar_y = self.toolbar.y;
        if enabled {
            self.annotation_mgr.annotations_hidden = false;
            // Mutually exclusive: disable laser and turn off highlighter when entering drawing mode
            self.config.laser.enabled = false;
            self.config.highlighter.enabled = false;
            if self.active_tool == ActiveTool::None {
                self.active_tool = ActiveTool::from_str(&self.config.annotation.selected_tool);
            }
            self.config.annotation.load_tool_settings(self.active_tool.as_str());
            self.renderer.invalidate_active_snapshot();
            // Exclude taskbar so it is always 100% click-through!
            self.sync_overlay_input_mode();
            unsafe {
                let _ = SetForegroundWindow(self.hwnd_overlay);
            }
            if matches!(self.active_tool, ActiveTool::Burn | ActiveTool::Dodge)
                || self.config.annotation.invert_color
            {
                self.ensure_screen_snapshot();
            }
            if let Some(rect) = self.renderer.effect_union_rect(&self.annotation_mgr) {
                self.renderer.refresh_live_effect_backdrop(self.hwnd_overlay, rect, true);
            }
        } else {
            self.annotation_mgr.commit_active_text();
            self.annotation_mgr.clear_selection();
            self.annotation_mgr.hovered_shape_idx = None;
            if self.active_tool != ActiveTool::None {
                self.config.annotation.save_tool_settings(self.active_tool.as_str());
                self.config.annotation.selected_tool = self.active_tool.as_str().to_string();
            }
            let _ = self.config.save();
            self.active_tool = ActiveTool::None;
            self.annotation_mgr.in_progress = None;
            self.renderer.invalidate_active_snapshot();
            let refs = self.annotation_mgr.referenced_snapshot_ids();
            self.renderer.prune_snapshots(&refs);
            // Restore full region and enable click-through
            self.sync_overlay_input_mode();
            // If laser is not active, restore highlighter
            if !self.config.laser.enabled {
                self.config.highlighter.enabled = true;
            }
        }
        if let Ok(mut c) = self.shared_config.lock() {
            *c = self.config.clone();
            let _ = c.save();
        }
        self.needs_render = true;
        self.update_tray_tooltip();
    }

    /// Exclude all monitor taskbars from the overlay window so taskbar & tray icon are ALWAYS click-through!
    pub fn apply_taskbar_exclusion(&self, exclude_taskbars: bool) {
        unsafe {
            if exclude_taskbars {
                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let workareas = get_all_workareas();
                let mut combined_rgn = HRGN::default();

                for wa in workareas {
                    let rgn = CreateRectRgn(
                        wa.left - vx,
                        wa.top - vy,
                        wa.right - vx,
                        wa.bottom - vy,
                    );
                    if combined_rgn.is_invalid() {
                        combined_rgn = rgn;
                    } else {
                        let _ = CombineRgn(combined_rgn, combined_rgn, rgn, RGN_OR);
                        let _ = DeleteObject(rgn);
                    }
                }

                if !combined_rgn.is_invalid() {
                    let _ = SetWindowRgn(self.hwnd_overlay, combined_rgn, TRUE);
                }
            } else {
                let _ = SetWindowRgn(self.hwnd_overlay, HRGN::default(), TRUE);
            }
        }
    }

    /// Update overlay click-through & HRGN so turning off annotation visibility releases the mouse to the OS
    /// while keeping the floating drawing panel open and interactive.
    pub fn sync_overlay_input_mode(&self) {
        if !self.is_drawing_mode {
            self.apply_taskbar_exclusion(false);
            self.set_click_through(true);
            return;
        }
        if self.annotation_mgr.annotations_hidden {
            unsafe {
                let mut ex = GetWindowLongPtrW(self.hwnd_overlay, GWL_EXSTYLE);
                ex &= !(WS_EX_TRANSPARENT.0 as isize);
                ex |= WS_EX_NOACTIVATE.0 as isize;
                SetWindowLongPtrW(self.hwnd_overlay, GWL_EXSTYLE, ex);

                let rgn = if self.config.annotation.show_toolbar {
                    let px = self.toolbar.x.floor() as i32;
                    let py = self.toolbar.y.floor() as i32;
                    let pw = self.toolbar.width.ceil() as i32;
                    let ph = self.toolbar.height.ceil() as i32;
                    let base = CreateRectRgn(px, py, px + pw, py + ph);
                    if !base.is_invalid() && self.toolbar.has_open_flyout() {
                        let fr = self.toolbar.get_flyout_rect();
                        let frgn = CreateRectRgn(
                            fr.left.floor() as i32,
                            fr.top.floor() as i32,
                            fr.right.ceil() as i32,
                            fr.bottom.ceil() as i32,
                        );
                        if !frgn.is_invalid() {
                            let _ = CombineRgn(base, base, frgn, RGN_OR);
                            let _ = DeleteObject(frgn);
                        }
                    }
                    base
                } else {
                    CreateRectRgn(0, 0, 0, 0)
                };
                if !rgn.is_invalid() {
                    let _ = SetWindowRgn(self.hwnd_overlay, rgn, TRUE);
                }
                let _ = SetWindowPos(
                    self.hwnd_overlay,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                );
            }
        } else {
            self.set_click_through(false);
            self.apply_taskbar_exclusion(true);
        }
    }

    pub fn toggle_drawing_mode(&mut self) {
        let new_state = !self.is_drawing_mode;
        self.set_drawing_mode(new_state);
    }

    pub fn set_tool(&mut self, tool: ActiveTool) {
        if self.active_tool != ActiveTool::None && self.active_tool != tool {
            self.config.annotation.save_tool_settings(self.active_tool.as_str());
        }
        self.active_tool = tool;
        self.config.annotation.selected_tool = tool.as_str().to_string();
        match tool {
            ActiveTool::Freehand | ActiveTool::Text => {
                self.config.annotation.last_pen_text = tool.as_str().to_string();
            }
            ActiveTool::Burn | ActiveTool::Dodge => {
                self.config.annotation.last_burn_dodge = tool.as_str().to_string();
            }
            ActiveTool::Eraser | ActiveTool::Transform => {
                self.config.annotation.last_eraser_transform = tool.as_str().to_string();
            }
            _ if tool.is_shape() => {
                self.config.annotation.last_shape_tool = tool.as_str().to_string();
            }
            _ => {}
        }
        self.config.annotation.load_tool_settings(tool.as_str());
        let _ = self.config.save();
        self.set_drawing_mode(true);
    }

    pub fn toggle_laser(&mut self) {
        self.config.laser.enabled = !self.config.laser.enabled;
        if self.config.laser.enabled {
            self.set_drawing_mode(false);
            self.config.highlighter.enabled = false;
        } else {
            self.config.highlighter.enabled = true;
        }
        if let Ok(mut c) = self.shared_config.lock() {
            *c = self.config.clone();
            let _ = c.save();
        }
        self.needs_render = true;
        self.update_tray_tooltip();
    }

    pub fn toggle_highlighter(&mut self) {
        self.config.highlighter.enabled = !self.config.highlighter.enabled;
        if self.config.highlighter.enabled {
            self.config.laser.enabled = false;
            self.set_drawing_mode(false);
        }
        if let Ok(mut c) = self.shared_config.lock() {
            *c = self.config.clone();
            let _ = c.save();
        }
        self.needs_render = true;
        self.update_tray_tooltip();
    }

    pub fn toggle_osd(&mut self) {
        self.config.osd.enabled = !self.config.osd.enabled;
        if let Ok(mut c) = self.shared_config.lock() {
            *c = self.config.clone();
            let _ = c.save();
        }
        self.needs_render = true;
    }

    /// Color change: changes all modes if sync_colors is true; changes active mode if false
    pub fn set_color_idx(&mut self, idx: usize) {
        if let Some(c) = self.config.annotation.color_presets.get(idx) {
            if self.config.sync_colors {
                self.config.annotation.invert_color = false;
                self.config.annotation.current_color = *c;
                self.config.laser.color = *c;
                self.config.highlighter.color = *c;
            } else {
                if self.is_drawing_mode {
                    self.config.annotation.invert_color = false;
                    self.config.annotation.current_color = *c;
                } else if self.config.laser.enabled {
                    self.config.laser.color = *c;
                } else {
                    self.config.highlighter.color = *c;
                }
            }
            if let Ok(mut sc) = self.shared_config.lock() {
                *sc = self.config.clone();
                let _ = sc.save();
            }
            self.needs_render = true;
        }
    }

    pub fn set_selected_emoji(&mut self, emoji: &str) {
        self.config.annotation.selected_emoji = emoji.to_string();
        self.update_emoji_cursor_if_changed();
        self.set_tool(ActiveTool::Emoji);
        if let Ok(mut sc) = self.shared_config.lock() {
            *sc = self.config.clone();
            let _ = sc.save();
        }
        self.needs_render = true;
    }

    pub fn clear_annotations(&mut self) {
        self.annotation_mgr.clear();
        let refs = self.annotation_mgr.referenced_snapshot_ids();
        self.renderer.prune_snapshots(&refs);
        self.needs_render = true;
    }

    pub fn undo(&mut self) {
        if self.annotation_mgr.undo() {
            let refs = self.annotation_mgr.referenced_snapshot_ids();
            self.renderer.prune_snapshots(&refs);
            self.needs_render = true;
        }
    }

    pub fn redo(&mut self) {
        if self.annotation_mgr.redo() {
            self.needs_render = true;
        }
    }

    pub fn update_tray_tooltip(&mut self) {
        let mode_str = if self.is_drawing_mode {
            "ScreenTools [✏️ Drawing Mode] - Click for Highlighter"
        } else if self.config.laser.enabled {
            "ScreenTools [⚡ Laser Mode] - Click for Drawing"
        } else {
            "ScreenTools [💡 Highlighter Mode] - Click for Laser"
        };
        let mode = if self.is_drawing_mode {
            TrayMode::Drawing
        } else if self.config.laser.enabled {
            TrayMode::Laser
        } else {
            TrayMode::Highlighter
        };
        if let Some(ref mut tray) = self.tray {
            tray.update_tip(mode_str);
            tray.update_mode(mode);
        }
    }

    #[allow(dead_code)]
    pub fn current_tool_id(&self) -> usize {
        match self.active_tool {
            ActiveTool::Freehand => IDM_TOOL_FREEHAND,
            ActiveTool::Line => IDM_TOOL_LINE,
            ActiveTool::Arrow => IDM_TOOL_ARROW,
            ActiveTool::Rect => IDM_TOOL_RECT,
            ActiveTool::Ellipse => IDM_TOOL_ELLIPSE,
            ActiveTool::Emoji => IDM_TOOL_EMOJI,
            ActiveTool::Eraser => IDM_TOOL_ERASER,
            _ => 0,
        }
    }

    pub fn current_color_idx(&self) -> usize {
        let cur = if !self.config.sync_colors {
            if self.is_drawing_mode {
                self.config.annotation.current_color
            } else if self.config.laser.enabled {
                self.config.laser.color
            } else {
                self.config.highlighter.color
            }
        } else {
            self.config.highlighter.color
        };
        for (i, c) in self.config.annotation.color_presets.iter().enumerate() {
            if (c[0] - cur[0]).abs() < 0.03 && (c[1] - cur[1]).abs() < 0.03 && (c[2] - cur[2]).abs() < 0.03 {
                return i;
            }
        }
        0
    }

    pub fn handle_command(&mut self, cmd_id: usize) {
        match cmd_id {
            IDM_TOGGLE_DRAW => self.toggle_drawing_mode(),
            IDM_TOGGLE_DRAWING_PANEL => {
                if self.is_drawing_mode {
                    self.config.annotation.show_toolbar = !self.config.annotation.show_toolbar;
                    let _ = self.config.save();
                    if let Ok(mut sc) = self.shared_config.lock() {
                        *sc = self.config.clone();
                    }
                    self.needs_render = true;
                }
            }
            IDM_TOOL_FREEHAND => self.set_tool(ActiveTool::Freehand),
            IDM_TOOL_LINE => self.set_tool(ActiveTool::Line),
            IDM_TOOL_ARROW => self.set_tool(ActiveTool::Arrow),
            IDM_TOOL_RECT => self.set_tool(ActiveTool::Rect),
            IDM_TOOL_ELLIPSE => self.set_tool(ActiveTool::Ellipse),
            IDM_TOOL_EMOJI => self.set_tool(ActiveTool::Emoji),
            IDM_TOOL_ERASER => self.set_tool(ActiveTool::Eraser),

            IDM_TOGGLE_LASER => self.toggle_laser(),
            IDM_CLEAR_SCREEN => self.clear_annotations(),
            IDM_UNDO => self.undo(),
            IDM_REDO => self.redo(),

            IDM_TOGGLE_HIGHLIGHTER => self.toggle_highlighter(),
            IDM_TOGGLE_OSD => self.toggle_osd(),

            IDM_OPEN_CONFIG => {
                let cfg = Arc::clone(&self.shared_config);
                let _ = crate::settings_ui::SettingsWindow::open(cfg, Some(Box::new(|| {
                    unsafe {
                        if let Some(ptr) = APP_INSTANCE {
                            (*ptr).sync_config();
                        }
                    }
                })));
            }
            IDM_EXIT => unsafe {
                let _ = PostQuitMessage(0);
            },

            // Color presets (2200..2219)
            id if (IDM_COLOR_BASE..IDM_COLOR_BASE + 20).contains(&id) => {
                self.set_color_idx(id - IDM_COLOR_BASE);
            }

            // Emoji presets (2400..2419)
            id if (IDM_EMOJI_BASE..IDM_EMOJI_BASE + 20).contains(&id) => {
                let idx = id - IDM_EMOJI_BASE;
                if let Some(emoji) = self.config.annotation.custom_emojis.get(idx) {
                    let emoji = emoji.clone();
                    self.set_selected_emoji(&emoji);
                }
            }

            _ => {}
        }
    }
}

// =============================================================================
// Controller Window Procedure (System Tray & Context Menu Only - Zero Hotkeys)
// =============================================================================
unsafe extern "system" fn controller_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let app = match APP_INSTANCE {
        Some(ptr) => &mut *ptr,
        None => return DefWindowProcW(hwnd, msg, wparam, lparam),
    };

    match msg {
        WM_TRAYICON => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_RBUTTONUP {
                // Right-Click opens context menu; synchronously processes returned command ID!
                if let Some(ref tray) = app.tray {
                    let cmd_id = tray.show_context_menu(
                        app.is_drawing_mode,
                        app.config.annotation.show_toolbar,
                        app.config.laser.enabled,
                        app.config.highlighter.enabled,
                        app.config.osd.enabled,
                        app.current_color_idx(),
                        &app.config.annotation.color_presets,
                        app.config.sync_colors,
                    );
                    if cmd_id != 0 {
                        app.handle_command(cmd_id);
                    }
                }
            } else if event == WM_LBUTTONUP {
                // Left-Click cycles None -> Writing -> Laser -> None (one at a time)
                app.cycle_presentation_mode();
            }
            LRESULT(0)
        }

        WM_COMMAND => {
            let cmd_id = (wparam.0 & 0xFFFF) as usize;
            app.handle_command(cmd_id);
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == TIMER_RENDER {
                // 1. Tick laser and click ripple decay
                if app.annotation_mgr.tick_cleanup(app.config.laser.trail_duration_ms) {
                    app.needs_render = true;
                }

                // 2. Tick keystroke cleanup and fetch tokens
                let mut osd_active = false;
                let tokens_snapshot = if let Ok(mut k_state) = app.keystroke_state.lock() {
                    osd_active = k_state.tick_cleanup(app.config.osd.stay_duration_ms, app.config.osd.fade_duration_ms);
                    k_state.active_tokens.clone()
                } else {
                    Vec::new()
                };

                if app.config.osd.enabled && (osd_active || app.last_osd_had_tokens) {
                    app.needs_render = true;
                }
                app.last_osd_had_tokens = osd_active;

                // 3. Check if cursor moved across ANY screen (reduced sensitivity threshold to avoid jitter renders)
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let cur = Point2D::new(pt.x as f32, pt.y as f32);
                let cursor_moved = cur.distance_to(app.last_rendered_cursor) > 2.0;

                if cursor_moved && (app.config.highlighter.enabled || (app.is_drawing_mode && !app.annotation_mgr.annotations_hidden)) {
                    app.needs_render = true;
                }

                // 4. Laser trail tracking (reduced points: only renders when point was accepted)
                if app.config.laser.enabled && cursor_moved {
                    let client_pt = app.renderer.screen_to_client(cur);
                    if app.annotation_mgr.add_laser_point(client_pt) {
                        app.needs_render = true;
                    }
                }

                // 5. Keep overlay window topmost across all monitors without stealing focus
                app.topmost_tick_counter += 1;
                if app.topmost_tick_counter % 60 == 0 {
                    let _ = SetWindowPos(
                        app.hwnd_overlay,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                    crate::settings_ui::ensure_settings_topmost();
                }

                // 5b. Live dynamic backdrop refresh for Invert / Burn / Dodge when screen content changes
                if !(app.is_drawing_mode && app.is_mouse_down) && (app.topmost_tick_counter % 3 == 0) {
                    if let Some(rect) = app.renderer.effect_union_rect(&app.annotation_mgr) {
                        if app.renderer.refresh_live_effect_backdrop(app.hwnd_overlay, rect, false) {
                            app.needs_render = true;
                        }
                    }
                }

                // 6. If dirty, render frame
                if app.needs_render {
                    app.last_rendered_cursor = cur;
                    app.renderer.update_osd_bg_image(&app.config.osd.bg_image_path);

                    let _ = app.renderer.render_frame(
                        cur,
                        true,
                        &app.config,
                        &app.annotation_mgr,
                        app.active_tool,
                        app.is_drawing_mode,
                        &app.toolbar,
                        &tokens_snapshot,
                    );

                    // Keep rendering while animations (laser, osd) are active
                    app.needs_render = osd_active
                        || !app.annotation_mgr.laser_points.is_empty();
                }
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            app.config.annotation.toolbar_x = app.toolbar.x;
            app.config.annotation.toolbar_y = app.toolbar.y;
            let _ = app.config.save();
            let _ = PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// =============================================================================
// Monitor & Taskbar Exclusion Helpers
// =============================================================================
unsafe extern "system" fn monitor_workarea_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let regions = &mut *(lparam.0 as *mut Vec<RECT>);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
        regions.push(mi.rcWork);
    }
    BOOL(1)
}

fn get_all_workareas() -> Vec<RECT> {
    let mut rects = Vec::new();
    unsafe {
        let lparam = LPARAM(&mut rects as *mut _ as isize);
        let _ = EnumDisplayMonitors(HDC::default(), None, Some(monitor_workarea_proc), lparam);
    }
    rects
}

/// Checks whether a given screen point is over any monitor's taskbar (outside rcWork)
fn is_point_over_taskbar(pt: POINT) -> bool {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            let in_work = pt.x >= mi.rcWork.left
                && pt.x < mi.rcWork.right
                && pt.y >= mi.rcWork.top
                && pt.y < mi.rcWork.bottom;
            return !in_work;
        }
        false
    }
}

// =============================================================================
// Fullscreen Overlay Window Procedure (Captures strokes in Drawing Mode)
// =============================================================================
unsafe extern "system" fn overlay_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let app = match APP_INSTANCE {
        Some(ptr) => &mut *ptr,
        None => return DefWindowProcW(hwnd, msg, wparam, lparam),
    };

    match msg {
        WM_NCHITTEST => {
            // When in drawing mode with visibility ON: capture mouse within active desktop work areas
            // When visibility is OFF: capture ONLY the floating control panel and pass everything else to OS!
            if app.is_drawing_mode {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                if app.annotation_mgr.annotations_hidden {
                    let client_pt = app.renderer.screen_to_client(Point2D::new(pt.x as f32, pt.y as f32));
                    if app.config.annotation.show_toolbar && app.toolbar.hit_test(client_pt) {
                        LRESULT(HTCLIENT as isize)
                    } else {
                        LRESULT(HTTRANSPARENT as isize)
                    }
                } else if is_point_over_taskbar(pt) || crate::settings_ui::is_point_over_settings(pt) {
                    LRESULT(HTTRANSPARENT as isize)
                } else {
                    LRESULT(HTCLIENT as isize)
                }
            } else {
                LRESULT(HTTRANSPARENT as isize)
            }
        }

        WM_MOUSEACTIVATE => {
            if app.is_drawing_mode {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                if app.annotation_mgr.annotations_hidden {
                    LRESULT(MA_NOACTIVATE as isize)
                } else if is_point_over_taskbar(pt) || crate::settings_ui::is_point_over_settings(pt) {
                    LRESULT(MA_NOACTIVATEANDEAT as isize)
                } else {
                    LRESULT(MA_ACTIVATE as isize)
                }
            } else {
                LRESULT(MA_NOACTIVATEANDEAT as isize)
            }
        }

        WM_SETCURSOR => {
            if !app.is_drawing_mode {
                // Return DefWindowProcW so Windows manages dynamic cursors naturally (resize arrows, pointer, text I-beam)
                DefWindowProcW(hwnd, msg, wparam, lparam)
            } else {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let client_pt = app.renderer.screen_to_client(Point2D::new(pt.x as f32, pt.y as f32));
                if is_point_over_taskbar(pt) || crate::settings_ui::is_point_over_settings(pt) {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                } else if app.config.annotation.show_toolbar && app.toolbar.hit_test(client_pt) {
                    // Hovering over floating drawing toolbar: standard arrow cursor
                    let cursor = LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default();
                    SetCursor(cursor);
                    LRESULT(1)
                } else if app.annotation_mgr.annotations_hidden {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                } else if app.active_tool == ActiveTool::Eraser {
                    if let Some(c) = app.eraser_cursor {
                        SetCursor(c);
                    } else {
                        let fallback = LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default();
                        SetCursor(fallback);
                    }
                    LRESULT(1)
                } else if app.active_tool == ActiveTool::Transform {
                    let handle_opt = app.annotation_mgr.hit_test_transform_handle(client_pt);
                    let cursor_id = match handle_opt {
                        Some(crate::annotation::TransformHandle::Rotate) => IDC_HAND,
                        Some(crate::annotation::TransformHandle::ScaleTopLeft)
                        | Some(crate::annotation::TransformHandle::ScaleBottomRight) => IDC_SIZENWSE,
                        Some(crate::annotation::TransformHandle::ScaleTopRight)
                        | Some(crate::annotation::TransformHandle::ScaleBottomLeft) => IDC_SIZENESW,
                        Some(crate::annotation::TransformHandle::ScaleTop)
                        | Some(crate::annotation::TransformHandle::ScaleBottom) => IDC_SIZENS,
                        Some(crate::annotation::TransformHandle::ScaleLeft)
                        | Some(crate::annotation::TransformHandle::ScaleRight) => IDC_SIZEWE,
                        Some(crate::annotation::TransformHandle::Move) => IDC_SIZEALL,
                        None => {
                            if app.annotation_mgr.find_shape_at(client_pt, 12.0).is_some() {
                                IDC_SIZEALL
                            } else {
                                IDC_ARROW
                            }
                        }
                    };
                    let cursor = LoadCursorW(HINSTANCE::default(), cursor_id).unwrap_or_default();
                    SetCursor(cursor);
                    LRESULT(1)
                } else if app.active_tool == ActiveTool::Text {
                    let ibeam = LoadCursorW(HINSTANCE::default(), IDC_IBEAM).unwrap_or_default();
                    SetCursor(ibeam);
                    LRESULT(1)
                } else if app.active_tool == ActiveTool::Freehand {
                    if let Some(c) = app.pen_cursor {
                        SetCursor(c);
                    } else {
                        let fallback = LoadCursorW(HINSTANCE::default(), IDC_CROSS).unwrap_or_default();
                        SetCursor(fallback);
                    }
                    LRESULT(1)
                } else {
                    // For geometric shapes and emoji stamps: show precision crosshair cursor
                    // while Direct2D renders the live dot/emoji preview at exact size and opacity!
                    let cross_cursor = LoadCursorW(HINSTANCE::default(), IDC_CROSS).unwrap_or_default();
                    SetCursor(cross_cursor);
                    LRESULT(1)
                }
            }
        }

        WM_MOUSEMOVE => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let screen_pt = Point2D::new(pt.x as f32, pt.y as f32);
            let client_pt = app.renderer.screen_to_client(screen_pt);

            if app.is_drawing_mode {
                if app.config.annotation.show_toolbar
                    && app.toolbar.handle_mouse_move(client_pt, &mut app.config, app.active_tool)
                {
                    if app.annotation_mgr.annotations_hidden && app.toolbar.is_dragging {
                        app.sync_overlay_input_mode();
                    }
                    if let Ok(mut sc) = app.shared_config.lock() {
                        *sc = app.config.clone();
                    }
                    app.needs_render = true;
                    return LRESULT(0);
                }

                // Drawing in progress (only trigger render if a point was actually recorded)
                if app.is_mouse_down {
                    let shift_held = unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
                    if app.active_tool == ActiveTool::Eraser {
                        if app.annotation_mgr.erase_object_at(client_pt, 14.0) {
                            app.needs_render = true;
                        }
                    } else if app.active_tool == ActiveTool::Transform {
                        if app.annotation_mgr.active_transform_handle.is_some() {
                            if app.annotation_mgr.update_transform(client_pt, shift_held) {
                                if let Some(rect) = app.renderer.effect_union_rect(&app.annotation_mgr) {
                                    if !app.renderer.has_live_rect_covering(rect) {
                                        app.renderer.refresh_live_effect_backdrop(app.hwnd_overlay, rect, false);
                                    }
                                }
                                app.needs_render = true;
                            }
                        } else if app.annotation_mgr.marquee_start.is_some() {
                            app.annotation_mgr.update_marquee(client_pt);
                            app.needs_render = true;
                        }
                    } else if app.annotation_mgr.update_drawing(client_pt, shift_held) {
                        if let Some(rect) = app.renderer.effect_union_rect(&app.annotation_mgr) {
                            if !app.renderer.has_live_rect_covering(rect) {
                                app.renderer.refresh_live_effect_backdrop(app.hwnd_overlay, rect, false);
                            }
                        }
                        app.needs_render = true;
                    }
                } else {
                    if app.active_tool == ActiveTool::Transform {
                        let hovered = app.annotation_mgr.find_shape_at(client_pt, 14.0);
                        if app.annotation_mgr.hovered_shape_idx != hovered {
                            app.annotation_mgr.hovered_shape_idx = hovered;
                            app.needs_render = true;
                        }
                    } else {
                        // Repaint when hovering over canvas so live dot/emoji preview moves with cursor
                        app.needs_render = true;
                    }
                }
            }

            if app.config.highlighter.enabled && screen_pt.distance_to(app.last_rendered_cursor) > 2.0 {
                app.needs_render = true;
            }

            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let screen_pt = Point2D::new(pt.x as f32, pt.y as f32);
            let client_pt = app.renderer.screen_to_client(screen_pt);

            // In Drawing Mode: Check floating toolbar interaction first
            if app.is_drawing_mode {
                let flyout_was_open = app.toolbar.has_open_flyout();
                let was_hidden = app.annotation_mgr.annotations_hidden;
                if app.config.annotation.show_toolbar && app.toolbar.hit_test(client_pt) {
                    app.annotation_mgr.commit_active_text();
                    let mut needs_exit = false;
                    if app.toolbar.handle_lbutton_down(
                        client_pt,
                        &mut app.config,
                        &mut app.active_tool,
                        &mut app.annotation_mgr,
                        &mut needs_exit,
                    ) {
                        if app.toolbar.is_dragging || app.toolbar.active_slider.is_some() {
                            unsafe { let _ = SetCapture(hwnd); }
                        }
                        app.update_emoji_cursor_if_changed();
                        if app.toolbar.pending_open_settings {
                            app.toolbar.pending_open_settings = false;
                            app.handle_command(IDM_OPEN_CONFIG);
                        }
                        if needs_exit {
                            app.set_drawing_mode(false);
                        } else {
                            if was_hidden != app.annotation_mgr.annotations_hidden
                                || flyout_was_open != app.toolbar.has_open_flyout()
                            {
                                app.sync_overlay_input_mode();
                                if was_hidden && !app.annotation_mgr.annotations_hidden {
                                    unsafe { let _ = SetForegroundWindow(hwnd); }
                                }
                            }
                            if app.current_effect_mode() != ScreenEffectMode::Normal {
                                app.ensure_screen_snapshot();
                            }
                        }
                        if let Some(rect) = app.renderer.effect_union_rect(&app.annotation_mgr) {
                            app.renderer.refresh_live_effect_backdrop(app.hwnd_overlay, rect, false);
                        }
                        let refs = app.annotation_mgr.referenced_snapshot_ids();
                        app.renderer.prune_snapshots(&refs);
                        if let Ok(mut sc) = app.shared_config.lock() {
                            *sc = app.config.clone();
                        }
                        app.needs_render = true;
                        return LRESULT(0);
                    }
                }

                // If a flyout was open and click occurred outside both toolbar & flyouts,
                // dismiss the flyout cleanly without creating an unwanted mark on canvas!
                if flyout_was_open {
                    app.toolbar.reset_confirmations();
                    app.toolbar.close_flyouts();
                    if app.annotation_mgr.annotations_hidden {
                        app.sync_overlay_input_mode();
                    }
                    app.needs_render = true;
                    return LRESULT(0);
                }

                // Guard: never paint onto the canvas while annotations visibility is turned off
                if app.annotation_mgr.annotations_hidden {
                    return LRESULT(0);
                }

                // Clicked outside toolbar: reset any pending confirmations and close flyouts
                app.toolbar.reset_confirmations();
                app.toolbar.close_flyouts();

                app.is_mouse_down = true;

                // Start shape or perform tool action on canvas
                match app.active_tool {
                    ActiveTool::Text => {
                        app.annotation_mgr.commit_active_text();
                        let color_rgba = [
                            app.config.annotation.current_color[0],
                            app.config.annotation.current_color[1],
                            app.config.annotation.current_color[2],
                            app.config.annotation.stroke_opacity,
                        ];
                        let effect = app.current_effect_mode();
                        let snapshot_id = if effect != ScreenEffectMode::Normal {
                            app.ensure_screen_snapshot()
                        } else {
                            0
                        };
                        app.annotation_mgr.start_text_input(
                            client_pt,
                            app.config.annotation.font_family.clone(),
                            app.config.annotation.font_size,
                            color_rgba,
                            effect,
                            snapshot_id,
                        );
                        unsafe { let _ = SetFocus(hwnd); }
                        app.needs_render = true;
                    }
                    ActiveTool::Transform => {
                        app.annotation_mgr.commit_active_text();
                        let shift_held = unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
                        let ctrl_held = unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
                        let ctrl_or_shift = ctrl_held || shift_held;

                        // 1. Check if clicking on an active handle of current selection
                        if let Some(handle) = app.annotation_mgr.hit_test_transform_handle(client_pt) {
                            app.annotation_mgr.start_transform(handle, client_pt);
                            unsafe { let _ = SetCapture(hwnd); }
                        } else {
                            // 2. Check if clicking directly on a shape
                            if let Some(shape_idx) = app.annotation_mgr.find_shape_at(client_pt, 12.0) {
                                if ctrl_or_shift {
                                    // Multi-selection toggle
                                    if let Some(pos) = app.annotation_mgr.selected_indices.iter().position(|&i| i == shape_idx) {
                                        app.annotation_mgr.selected_indices.remove(pos);
                                    } else {
                                        app.annotation_mgr.selected_indices.push(shape_idx);
                                        app.annotation_mgr.selected_indices.sort_unstable();
                                    }
                                    if app.annotation_mgr.selected_indices.contains(&shape_idx) {
                                        app.annotation_mgr.start_transform(crate::annotation::TransformHandle::Move, client_pt);
                                        unsafe { let _ = SetCapture(hwnd); }
                                    }
                                } else {
                                    // Single selection: if not in selection, select only this shape
                                    if !app.annotation_mgr.selected_indices.contains(&shape_idx) {
                                        app.annotation_mgr.selected_indices = vec![shape_idx];
                                    }
                                    app.annotation_mgr.start_transform(crate::annotation::TransformHandle::Move, client_pt);
                                    unsafe { let _ = SetCapture(hwnd); }
                                }
                            } else {
                                // 3. Clicked empty canvas: start marquee selection box
                                if !ctrl_or_shift {
                                    app.annotation_mgr.selected_indices.clear();
                                }
                                app.annotation_mgr.start_marquee(client_pt);
                                unsafe { let _ = SetCapture(hwnd); }
                            }
                        }
                        app.needs_render = true;
                    }
                    ActiveTool::Emoji => {
                        app.annotation_mgr.commit_active_text();
                        let min_w = app.config.annotation.min_stroke_width.max(0.5);
                        let max_w = app.config.annotation.max_stroke_width.max(min_w + 1.0);
                        let size_pct = ((app.config.annotation.stroke_width - min_w) / (max_w - min_w)).clamp(0.0, 1.0);
                        let emoji_size = 20.0 + size_pct * 70.0;
                        let emoji = app.config.annotation.selected_emoji.clone();
                        let color_rgba = [
                            app.config.annotation.current_color[0],
                            app.config.annotation.current_color[1],
                            app.config.annotation.current_color[2],
                            app.config.annotation.stroke_opacity,
                        ];
                        let flat = app.config.annotation.emoji_style == "flat";
                        let filled = app.config.annotation.emoji_filled;
                        let effect = app.current_effect_mode();
                        let snapshot_id = if effect != ScreenEffectMode::Normal {
                            app.ensure_screen_snapshot()
                        } else {
                            0
                        };
                        app.annotation_mgr.add_emoji_stamp(
                            client_pt,
                            &emoji,
                            emoji_size,
                            app.config.annotation.stroke_opacity,
                            color_rgba,
                            flat,
                            filled,
                            effect,
                            snapshot_id,
                        );
                        if effect != ScreenEffectMode::Normal {
                            if let Some(rect) = app.renderer.effect_union_rect(&app.annotation_mgr) {
                                if !app.renderer.has_live_rect_covering(rect) {
                                    app.renderer.refresh_live_effect_backdrop(app.hwnd_overlay, rect, false);
                                }
                            }
                        }
                        app.needs_render = true;
                    }
                    ActiveTool::Eraser => {
                        app.annotation_mgr.commit_active_text();
                        if app.annotation_mgr.erase_object_at(client_pt, 14.0) {
                            app.needs_render = true;
                        }
                    }
                    _ => {
                        app.annotation_mgr.commit_active_text();
                        let fill = match app.active_tool {
                            ActiveTool::Rect => app.config.annotation.rect_filled,
                            ActiveTool::Ellipse => app.config.annotation.ellipse_filled,
                            _ => false,
                        };
                        let effect = app.current_effect_mode();
                        let snapshot_id = if effect != ScreenEffectMode::Normal {
                            app.ensure_screen_snapshot()
                        } else {
                            0
                        };
                        app.annotation_mgr.start_drawing(
                            app.active_tool,
                            client_pt,
                            app.config.annotation.current_color,
                            app.config.annotation.stroke_width,
                            app.config.annotation.stroke_opacity,
                            app.config.annotation.stroke_softness,
                            fill,
                            app.config.annotation.line_dotted,
                            app.config.annotation.line_wavy,
                            app.config.annotation.arrow_wavy,
                            effect,
                            snapshot_id,
                        );
                        if effect != ScreenEffectMode::Normal {
                            if let Some(rect) = app.renderer.effect_union_rect(&app.annotation_mgr) {
                                if !app.renderer.has_live_rect_covering(rect) {
                                    app.renderer.refresh_live_effect_backdrop(app.hwnd_overlay, rect, false);
                                }
                            }
                        }
                        app.needs_render = true;
                    }
                }
            }

            LRESULT(0)
        }

        WM_LBUTTONUP => {
            unsafe { let _ = ReleaseCapture(); }
            if app.is_drawing_mode {
                if app.active_tool == ActiveTool::Transform {
                    let shift_held = unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
                    let ctrl_held = unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
                    let ctrl_or_shift = ctrl_held || shift_held;

                    if app.annotation_mgr.active_transform_handle.is_some() {
                        app.annotation_mgr.finish_transform();
                        app.needs_render = true;
                    }
                    if app.annotation_mgr.marquee_start.is_some() {
                        app.annotation_mgr.finish_marquee(ctrl_or_shift);
                        app.needs_render = true;
                    }
                }
                if app.toolbar.handle_lbutton_up(&mut app.config) {
                    if app.annotation_mgr.annotations_hidden {
                        app.sync_overlay_input_mode();
                    }
                    if let Ok(mut sc) = app.shared_config.lock() {
                        *sc = app.config.clone();
                    }
                    app.needs_render = true;
                }
            }
            app.is_mouse_down = false;

            if app.is_drawing_mode {
                if app.annotation_mgr.finish_drawing() {
                    app.needs_render = true;
                }
            }

            LRESULT(0)
        }

        // Right-Click in Drawing Mode: Exits drawing mode back to click-through (drawings remain on screen!)
        WM_RBUTTONUP => {
            if app.is_drawing_mode {
                if app.annotation_mgr.active_transform_handle.is_some() || app.annotation_mgr.marquee_start.is_some() {
                    app.annotation_mgr.cancel_transform_or_marquee();
                    app.needs_render = true;
                    return LRESULT(0);
                }
                if app.annotation_mgr.active_text.is_some() {
                    app.annotation_mgr.commit_active_text();
                    app.needs_render = true;
                    return LRESULT(0);
                }
                if app.toolbar.has_open_flyout() {
                    app.toolbar.close_flyouts();
                    app.needs_render = true;
                    return LRESULT(0);
                }
                if app.annotation_mgr.in_progress.is_some() {
                    app.annotation_mgr.in_progress = None;
                } else {
                    app.set_drawing_mode(false);
                }
                app.needs_render = true;
            }
            LRESULT(0)
        }

        WM_CHAR => {
            if app.is_drawing_mode {
                let ch_code = wparam.0 as u32;
                if ch_code == 8 {
                    // Backspace
                    if let Some(ref mut input) = app.annotation_mgr.active_text {
                        input.text.pop();
                        app.needs_render = true;
                        return LRESULT(0);
                    }
                } else if ch_code == 13 {
                    // Enter -> commit
                    if app.annotation_mgr.commit_active_text() {
                        app.needs_render = true;
                        return LRESULT(0);
                    }
                } else if ch_code == 27 {
                    // Escape -> cancel
                    if app.annotation_mgr.active_text.is_some() {
                        app.annotation_mgr.active_text = None;
                        app.needs_render = true;
                        return LRESULT(0);
                    }
                } else if ch_code >= 32 {
                    if let Some(ch) = char::from_u32(ch_code) {
                        if let Some(ref mut input) = app.annotation_mgr.active_text {
                            input.text.push(ch);
                            app.needs_render = true;
                            return LRESULT(0);
                        }
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_KEYDOWN => {
            if app.is_drawing_mode && !app.annotation_mgr.annotations_hidden {
                if app.annotation_mgr.active_text.is_some() {
                    if wparam.0 == VK_ESCAPE.0 as usize {
                        app.annotation_mgr.active_text = None;
                        app.needs_render = true;
                        return LRESULT(0);
                    } else if wparam.0 == VK_RETURN.0 as usize {
                        app.annotation_mgr.commit_active_text();
                        app.needs_render = true;
                        return LRESULT(0);
                    }
                } else if app.active_tool == ActiveTool::Transform {
                    if wparam.0 == VK_DELETE.0 as usize || wparam.0 == VK_BACK.0 as usize {
                        if app.annotation_mgr.delete_selected() {
                            app.needs_render = true;
                            return LRESULT(0);
                        }
                    } else if wparam.0 == VK_ESCAPE.0 as usize {
                        app.annotation_mgr.cancel_transform_or_marquee();
                        app.annotation_mgr.clear_selection();
                        app.needs_render = true;
                        return LRESULT(0);
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
