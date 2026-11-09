use crate::annotation::{ActiveTool, AnnotationManager};
use crate::config::AppConfig;
use crate::geometry::Point2D;

use windows::core::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::DirectWrite::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraggingSlider {
    Opacity,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatherIcon {
    Pen,
    Type,
    Eraser,
    Move,
    Circle { filled: bool },
    Square { filled: bool },
    Arrow { wavy: bool },
    Line { dotted: bool, wavy: bool },
    Undo,
    Redo,
    Trash,
    Close,
    ConfirmQuestion,
}

pub struct DrawingToolbar {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,

    pub is_dragging: bool,
    pub drag_offset_x: f32,
    pub drag_offset_y: f32,

    pub active_slider: Option<DraggingSlider>,
    pub emoji_flyout_open: bool,
    pub color_flyout_open: bool,

    pub confirm_clear: bool,
    pub confirm_exit: bool,
}

impl DrawingToolbar {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            // Width: 85px compact panel, height: 320px for square 35x35px buttons
            width: 85.0,
            height: 320.0,
            is_dragging: false,
            drag_offset_x: 0.0,
            drag_offset_y: 0.0,
            active_slider: None,
            emoji_flyout_open: false,
            color_flyout_open: false,
            confirm_clear: false,
            confirm_exit: false,
        }
    }

    /// Reset any pending 2-click confirmations
    pub fn reset_confirmations(&mut self) {
        self.confirm_clear = false;
        self.confirm_exit = false;
    }

    /// Calculate flyout bounding rectangle with auto left/right screen boundary management
    pub fn get_flyout_rect(&self) -> D2D_RECT_F {
        let screen_w = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
            )
        } as f32;
        let screen_w = if screen_w <= 0.0 { 1920.0 } else { screen_w };
        let flyout_w = 126.0;
        let flyout_h = 126.0;
        let flyout_y = (self.y + 100.0).max(10.0);

        let flyout_x = if self.x + self.width + 6.0 + flyout_w > screen_w {
            // Place to the left of the toolbar
            (self.x - flyout_w - 6.0).max(6.0)
        } else {
            // Place to the right of the toolbar
            self.x + self.width + 6.0
        };

        D2D_RECT_F {
            left: flyout_x,
            top: flyout_y,
            right: flyout_x + flyout_w,
            bottom: flyout_y + flyout_h,
        }
    }

    /// Check if point is inside the toolbar panel bounds or open flyouts
    pub fn hit_test(&self, p: Point2D) -> bool {
        let inside_panel = p.x >= self.x
            && p.x <= self.x + self.width
            && p.y >= self.y
            && p.y <= self.y + self.height;

        if inside_panel {
            return true;
        }

        if self.emoji_flyout_open || self.color_flyout_open {
            let r = self.get_flyout_rect();
            if p.x >= r.left && p.x <= r.right && p.y >= r.top && p.y <= r.bottom {
                return true;
            }
        }

        false
    }

    /// Handle left mouse down on the control panel
    pub fn handle_lbutton_down(
        &mut self,
        p: Point2D,
        config: &mut AppConfig,
        active_tool: &mut ActiveTool,
        annotation_mgr: &mut AnnotationManager,
        needs_exit: &mut bool,
    ) -> bool {
        *needs_exit = false;

        let flyout_rect = self.get_flyout_rect();
        let flyout_x = flyout_rect.left;
        let flyout_y = flyout_rect.top;
        let flyout_w = flyout_rect.right - flyout_rect.left;
        let flyout_h = flyout_rect.bottom - flyout_rect.top;

        let x = self.x;
        let y = self.y;

        // Check if user clicked on Button 5A (Emoji toggle) or Button 5B (Color toggle)
        let y5 = y + 135.0;
        let is_on_emoji_btn = p.y >= y5 && p.y <= y5 + 35.0 && p.x >= x + 5.0 && p.x <= x + 40.0;
        let is_on_color_btn = p.y >= y5 && p.y <= y5 + 35.0 && p.x >= x + 45.0 && p.x <= x + 80.0;

        if is_on_emoji_btn {
            self.reset_confirmations();
            *active_tool = ActiveTool::Emoji;
            config.annotation.selected_tool = "emoji".to_string();
            let _ = config.save();
            self.emoji_flyout_open = !self.emoji_flyout_open;
            self.color_flyout_open = false;
            return true;
        }

        if is_on_color_btn {
            self.reset_confirmations();
            self.color_flyout_open = !self.color_flyout_open;
            self.emoji_flyout_open = false;
            return true;
        }

        // 1. Emoji Flyout Hit-Test (if open)
        if self.emoji_flyout_open {
            if p.x >= flyout_x && p.x <= flyout_x + flyout_w && p.y >= flyout_y && p.y <= flyout_y + flyout_h {
                let e_size = 34.0;
                let e_gap = 5.0;
                let start_x = flyout_x + 6.0;
                let start_y = flyout_y + 6.0;

                for (idx, emoji_char) in config.annotation.custom_emojis.iter().enumerate().take(9) {
                    let col = idx % 3;
                    let row = idx / 3;
                    let ex = start_x + col as f32 * (e_size + e_gap);
                    let ey = start_y + row as f32 * (e_size + e_gap);

                    if p.x >= ex && p.x <= ex + e_size && p.y >= ey && p.y <= ey + e_size {
                        config.annotation.selected_emoji = emoji_char.clone();
                        *active_tool = ActiveTool::Emoji;
                        self.emoji_flyout_open = false;
                        let _ = config.save();
                        return true;
                    }
                }
                return true;
            } else {
                self.emoji_flyout_open = false;
            }
        }

        // 2. Color Flyout Hit-Test (if open)
        if self.color_flyout_open {
            if p.x >= flyout_x && p.x <= flyout_x + flyout_w && p.y >= flyout_y && p.y <= flyout_y + flyout_h {
                let c_size = 34.0;
                let c_gap = 5.0;
                let start_x = flyout_x + 6.0;
                let start_y = flyout_y + 6.0;

                for (idx, &col) in config.annotation.color_presets.iter().enumerate().take(9) {
                    let col_idx = idx % 3;
                    let row_idx = idx / 3;
                    let cx = start_x + col_idx as f32 * (c_size + c_gap);
                    let cy = start_y + row_idx as f32 * (c_size + c_gap);

                    if p.x >= cx && p.x <= cx + c_size && p.y >= cy && p.y <= cy + c_size {
                        config.annotation.current_color = col;
                        if config.sync_colors {
                            config.highlighter.color = col;
                            config.laser.color = col;
                        }
                        self.color_flyout_open = false;
                        let _ = config.save();
                        return true;
                    }
                }
                return true;
            } else {
                self.color_flyout_open = false;
            }
        }

        if !self.hit_test(p) {
            self.reset_confirmations();
            return false;
        }

        // Line 1: Top Bar Handle (move around screen)
        if p.y >= y && p.y <= y + 15.0 && p.x >= x && p.x <= x + self.width {
            self.is_dragging = true;
            self.drag_offset_x = p.x - self.x;
            self.drag_offset_y = p.y - self.y;
            self.reset_confirmations();
            return true;
        }

        // Line 2: Brush / Text Toggle (Button 1), Eraser / Transform Toggle (Button 2) (Y: y + 15.0..y + 50.0)
        let y2 = y + 15.0;
        if p.y >= y2 && p.y <= y2 + 35.0 {
            self.reset_confirmations();
            if p.x >= x + 5.0 && p.x <= x + 40.0 {
                if *active_tool == ActiveTool::Freehand {
                    *active_tool = ActiveTool::Text;
                } else {
                    *active_tool = ActiveTool::Freehand;
                }
                config.annotation.selected_tool = active_tool.as_str().to_string();
                let _ = config.save();
                return true;
            }
            if p.x >= x + 45.0 && p.x <= x + 80.0 {
                if *active_tool == ActiveTool::Eraser {
                    *active_tool = ActiveTool::Transform;
                } else {
                    *active_tool = ActiveTool::Eraser;
                }
                config.annotation.selected_tool = active_tool.as_str().to_string();
                let _ = config.save();
                return true;
            }
        }

        // Line 3: Round, Rectangle (Y: y + 55.0..y + 90.0)
        let y3 = y + 55.0;
        if p.y >= y3 && p.y <= y3 + 35.0 {
            self.reset_confirmations();
            if p.x >= x + 5.0 && p.x <= x + 40.0 {
                if *active_tool == ActiveTool::Ellipse {
                    config.annotation.ellipse_filled = !config.annotation.ellipse_filled;
                } else {
                    *active_tool = ActiveTool::Ellipse;
                }
                config.annotation.selected_tool = "ellipse".to_string();
                let _ = config.save();
                return true;
            }
            if p.x >= x + 45.0 && p.x <= x + 80.0 {
                if *active_tool == ActiveTool::Rect {
                    config.annotation.rect_filled = !config.annotation.rect_filled;
                } else {
                    *active_tool = ActiveTool::Rect;
                }
                config.annotation.selected_tool = "rect".to_string();
                let _ = config.save();
                return true;
            }
        }

        // Line 4: Arrow, Line (Y: y + 95.0..y + 130.0)
        let y4 = y + 95.0;
        if p.y >= y4 && p.y <= y4 + 35.0 {
            self.reset_confirmations();
            if p.x >= x + 5.0 && p.x <= x + 40.0 {
                if *active_tool == ActiveTool::Arrow {
                    config.annotation.arrow_wavy = !config.annotation.arrow_wavy;
                } else {
                    *active_tool = ActiveTool::Arrow;
                }
                config.annotation.selected_tool = "arrow".to_string();
                let _ = config.save();
                return true;
            }
            if p.x >= x + 45.0 && p.x <= x + 80.0 {
                if *active_tool == ActiveTool::Line {
                    // Cycle: Solid -> Dotted -> Wavy -> Solid
                    if !config.annotation.line_dotted && !config.annotation.line_wavy {
                        config.annotation.line_dotted = true;
                        config.annotation.line_wavy = false;
                    } else if config.annotation.line_dotted {
                        config.annotation.line_dotted = false;
                        config.annotation.line_wavy = true;
                    } else {
                        config.annotation.line_dotted = false;
                        config.annotation.line_wavy = false;
                    }
                } else {
                    *active_tool = ActiveTool::Line;
                }
                config.annotation.selected_tool = "line".to_string();
                let _ = config.save();
                return true;
            }
        }

        // Line 6: Opacity Slider (Y: y + 175.0..y + 203.0)
        let y6 = y + 175.0;
        if p.y >= y6 - 2.0 && p.y <= y6 + 28.0 && p.x >= x + 2.0 && p.x <= x + self.width - 2.0 {
            self.reset_confirmations();
            self.active_slider = Some(DraggingSlider::Opacity);
            self.update_opacity_slider(p.x, config);
            return true;
        }

        // Line 7: Size Slider (0 to 100%) (Y: y + 206.0..y + 234.0)
        let y7 = y + 206.0;
        if p.y >= y7 - 2.0 && p.y <= y7 + 28.0 && p.x >= x + 2.0 && p.x <= x + self.width - 2.0 {
            self.reset_confirmations();
            self.active_slider = Some(DraggingSlider::Size);
            self.update_size_slider(p.x, config);
            return true;
        }

        // Line 8: Undo, Redo (Y: y + 237.0..y + 272.0)
        let y8 = y + 237.0;
        if p.y >= y8 && p.y <= y8 + 35.0 {
            self.reset_confirmations();
            if p.x >= x + 5.0 && p.x <= x + 40.0 {
                if annotation_mgr.can_undo() {
                    annotation_mgr.undo();
                }
                return true;
            }
            if p.x >= x + 45.0 && p.x <= x + 80.0 {
                if annotation_mgr.can_redo() {
                    annotation_mgr.redo();
                }
                return true;
            }
        }

        // Line 9: Clear, Exit with Confirmation (Y: y + 277.0..y + 312.0)
        let y9 = y + 277.0;
        if p.y >= y9 && p.y <= y9 + 35.0 {
            // Button 1: Clear
            if p.x >= x + 5.0 && p.x <= x + 40.0 {
                if self.confirm_clear {
                    annotation_mgr.clear();
                    self.confirm_clear = false;
                } else {
                    self.confirm_clear = true;
                    self.confirm_exit = false;
                }
                return true;
            }

            // Button 2: Exit
            if p.x >= x + 45.0 && p.x <= x + 80.0 {
                if self.confirm_exit {
                    self.confirm_exit = false;
                    *needs_exit = true;
                } else {
                    self.confirm_exit = true;
                    self.confirm_clear = false;
                }
                return true;
            }
        }

        self.reset_confirmations();
        true
    }

    /// Handle mouse move while interacting with the toolbar
    pub fn handle_mouse_move(&mut self, p: Point2D, config: &mut AppConfig) -> bool {
        if self.is_dragging {
            self.x = (p.x - self.drag_offset_x).max(0.0);
            self.y = (p.y - self.drag_offset_y).max(0.0);
            config.annotation.toolbar_x = self.x;
            config.annotation.toolbar_y = self.y;
            return true;
        }

        if let Some(slider) = self.active_slider {
            match slider {
                DraggingSlider::Opacity => self.update_opacity_slider(p.x, config),
                DraggingSlider::Size => self.update_size_slider(p.x, config),
            }
            return true;
        }

        false
    }

    /// Handle mouse release
    pub fn handle_lbutton_up(&mut self, config: &mut AppConfig) -> bool {
        let changed = self.is_dragging || self.active_slider.is_some();
        if self.is_dragging {
            config.annotation.toolbar_x = self.x;
            config.annotation.toolbar_y = self.y;
        }
        self.is_dragging = false;
        self.active_slider = None;
        if changed {
            let _ = config.save();
        }
        changed
    }

    fn update_opacity_slider(&mut self, mouse_x: f32, config: &mut AppConfig) {
        let track_start = self.x + 6.0;
        let track_w = 73.0;
        let frac = ((mouse_x - track_start) / track_w).clamp(0.0, 1.0);
        // Range 0.05 .. 1.0
        config.annotation.stroke_opacity = 0.05 + frac * 0.95;
    }

    fn update_size_slider(&mut self, mouse_x: f32, config: &mut AppConfig) {
        let track_start = self.x + 6.0;
        let track_w = 73.0;
        let frac = ((mouse_x - track_start) / track_w).clamp(0.0, 1.0);
        let min_w = config.annotation.min_stroke_width.max(0.5);
        let max_w = config.annotation.max_stroke_width.max(min_w + 1.0);
        config.annotation.stroke_width = min_w + frac * (max_w - min_w);
    }

    /// Render the floating control panel using Direct2D
    pub unsafe fn render(
        &self,
        rt: &ID2D1RenderTarget,
        text_format_ui: &IDWriteTextFormat,
        text_format_small: &IDWriteTextFormat,
        text_format_emoji: &IDWriteTextFormat,
        config: &AppConfig,
        active_tool: ActiveTool,
        can_undo: bool,
        can_redo: bool,
    ) -> Result<()> {
        let x = self.x;
        let y = self.y;
        let w = self.width;
        let h = self.height;
        let panel_op = config.annotation.toolbar_opacity.clamp(0.3, 1.0);

        // 1. Panel Background Card (Dark sleek acrylic blur aesthetic)
        let card_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h },
            radiusX: 12.0,
            radiusY: 12.0,
        };

        let bg_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.09, g: 0.10, b: 0.14, a: panel_op },
            None,
        )?;
        rt.FillRoundedRectangle(&card_rect, &bg_brush);

        let border_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.28, g: 0.32, b: 0.44, a: panel_op * 0.9 },
            None,
        )?;
        rt.DrawRoundedRectangle(&card_rect, &border_brush, 1.2, None);

        // Line 1: Top Bar Handle (move around screen)
        let handle_pill = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 28.0, top: y + 6.0, right: x + 57.0, bottom: y + 10.0 },
            radiusX: 2.0,
            radiusY: 2.0,
        };
        let handle_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.50, g: 0.55, b: 0.68, a: panel_op * 0.8 },
            None,
        )?;
        rt.FillRoundedRectangle(&handle_pill, &handle_brush);

        // Round stroke style for Feather vector icons
        let factory = rt.GetFactory()?;
        let stroke_props = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 1.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
        };
        let round_stroke_style = factory.CreateStrokeStyle(&stroke_props, None).ok();

        // Helper closures for buttons with pure Feather Icons
        let draw_btn = |left: f32, top: f32, right: f32, bottom: f32, icon: FeatherIcon, is_active: bool, is_alert: bool, is_disabled: bool| -> Result<()> {
            let r = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left, top, right, bottom },
                radiusX: 6.0,
                radiusY: 6.0,
            };

            let (bg_color, border_color) = if is_disabled {
                (
                    D2D1_COLOR_F { r: 0.12, g: 0.13, b: 0.17, a: panel_op * 0.45 },
                    D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: panel_op * 0.35 },
                )
            } else if is_alert {
                (
                    D2D1_COLOR_F { r: 0.70, g: 0.18, b: 0.22, a: panel_op },
                    D2D1_COLOR_F { r: 1.0, g: 0.35, b: 0.40, a: panel_op },
                )
            } else if is_active {
                (
                    D2D1_COLOR_F { r: 0.20, g: 0.36, b: 0.75, a: panel_op },
                    D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: panel_op },
                )
            } else {
                (
                    D2D1_COLOR_F { r: 0.15, g: 0.17, b: 0.23, a: panel_op * 0.9 },
                    D2D1_COLOR_F { r: 0.25, g: 0.28, b: 0.36, a: panel_op * 0.7 },
                )
            };

            let b_brush = rt.CreateSolidColorBrush(&bg_color, None)?;
            rt.FillRoundedRectangle(&r, &b_brush);

            let border = rt.CreateSolidColorBrush(&border_color, None)?;
            rt.DrawRoundedRectangle(&r, &border, if is_active || is_alert { 1.5 } else { 1.0 }, None);

            let icon_alpha = if is_disabled { 0.30 } else { 1.0 };
            let icon_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: icon_alpha }, None)?;
            let cx = (left + right) * 0.5;
            let cy = (top + bottom) * 0.5;

            draw_feather_icon(rt, cx, cy, icon, &icon_brush, round_stroke_style.as_ref())?;

            Ok(())
        };

        // Line 2: Brush / Text Toggle (Button 1), Eraser / Transform Toggle (Button 2) (Square 35x35px)
        let y2 = y + 15.0;
        let pen_icon = if active_tool == ActiveTool::Text { FeatherIcon::Type } else { FeatherIcon::Pen };
        let eraser_icon = if active_tool == ActiveTool::Transform { FeatherIcon::Move } else { FeatherIcon::Eraser };
        draw_btn(x + 5.0, y2, x + 40.0, y2 + 35.0, pen_icon, active_tool == ActiveTool::Freehand || active_tool == ActiveTool::Text, false, false)?;
        draw_btn(x + 45.0, y2, x + 80.0, y2 + 35.0, eraser_icon, active_tool == ActiveTool::Eraser || active_tool == ActiveTool::Transform, false, false)?;

        // Line 3: Round (Feather circle hollow/filled), Rectangle (Feather square hollow/filled) (Square 35x35px)
        let y3 = y + 55.0;
        draw_btn(x + 5.0, y3, x + 40.0, y3 + 35.0, FeatherIcon::Circle { filled: config.annotation.ellipse_filled }, active_tool == ActiveTool::Ellipse, false, false)?;
        draw_btn(x + 45.0, y3, x + 80.0, y3 + 35.0, FeatherIcon::Square { filled: config.annotation.rect_filled }, active_tool == ActiveTool::Rect, false, false)?;

        // Line 4: Arrow (Feather arrow-up-right), Line (Feather minus) (Square 35x35px)
        let y4 = y + 95.0;
        draw_btn(x + 5.0, y4, x + 40.0, y4 + 35.0, FeatherIcon::Arrow { wavy: config.annotation.arrow_wavy }, active_tool == ActiveTool::Arrow, false, false)?;
        draw_btn(x + 45.0, y4, x + 80.0, y4 + 35.0, FeatherIcon::Line { dotted: config.annotation.line_dotted, wavy: config.annotation.line_wavy }, active_tool == ActiveTool::Line, false, false)?;

        // Line 5: Emoji (Left) and Color Palette (Right) with dropdown arrows (Square 35x35px)
        let y5 = y + 135.0;
        let is_emoji_active = active_tool == ActiveTool::Emoji;

        // Button 5A: Emoji button
        let emoji_btn_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 5.0, top: y5, right: x + 40.0, bottom: y5 + 35.0 },
            radiusX: 6.0,
            radiusY: 6.0,
        };
        let e_bg_col = if is_emoji_active || self.emoji_flyout_open {
            D2D1_COLOR_F { r: 0.20, g: 0.36, b: 0.75, a: panel_op }
        } else {
            D2D1_COLOR_F { r: 0.15, g: 0.17, b: 0.23, a: panel_op * 0.9 }
        };
        let e_border_col = if is_emoji_active || self.emoji_flyout_open {
            D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: panel_op }
        } else {
            D2D1_COLOR_F { r: 0.25, g: 0.28, b: 0.36, a: panel_op * 0.7 }
        };
        let e_bg = rt.CreateSolidColorBrush(&e_bg_col, None)?;
        rt.FillRoundedRectangle(&emoji_btn_rect, &e_bg);
        let e_border = rt.CreateSolidColorBrush(&e_border_col, None)?;
        rt.DrawRoundedRectangle(&emoji_btn_rect, &e_border, if is_emoji_active || self.emoji_flyout_open { 1.5 } else { 1.0 }, None);

        let e_wide: Vec<u16> = config.annotation.selected_emoji.encode_utf16().collect();
        let e_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
        rt.DrawText(&e_wide, text_format_ui, &D2D_RECT_F { left: x + 6.0, top: y5 + 6.0, right: x + 28.0, bottom: y5 + 34.0 }, &e_brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, DWRITE_MEASURING_MODE_NATURAL);

        let arrow_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 0.9 }, None)?;
        let arrow_wide: Vec<u16> = "▾".encode_utf16().collect();
        rt.DrawText(&arrow_wide, text_format_small, &D2D_RECT_F { left: x + 28.0, top: y5 + 8.0, right: x + 38.0, bottom: y5 + 30.0 }, &arrow_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Button 5B: Color button on Right (renders active color dot + arrow) (Square 35x35px)
        let color_btn_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 45.0, top: y5, right: x + 80.0, bottom: y5 + 35.0 },
            radiusX: 6.0,
            radiusY: 6.0,
        };
        let c_bg_color = if self.color_flyout_open {
            D2D1_COLOR_F { r: 0.20, g: 0.36, b: 0.75, a: panel_op }
        } else {
            D2D1_COLOR_F { r: 0.15, g: 0.17, b: 0.23, a: panel_op * 0.9 }
        };
        let c_border_color = if self.color_flyout_open {
            D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: panel_op }
        } else {
            D2D1_COLOR_F { r: 0.25, g: 0.28, b: 0.36, a: panel_op * 0.7 }
        };
        let c_bg = rt.CreateSolidColorBrush(&c_bg_color, None)?;
        rt.FillRoundedRectangle(&color_btn_rect, &c_bg);
        let c_border = rt.CreateSolidColorBrush(&c_border_color, None)?;
        rt.DrawRoundedRectangle(&color_btn_rect, &c_border, if self.color_flyout_open { 1.5 } else { 1.0 }, None);

        // Render current color circle dot inside button
        let cur_c = config.annotation.current_color;
        let dot_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: cur_c[0], g: cur_c[1], b: cur_c[2], a: 1.0 }, None)?;
        let dot_ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: x + 57.0, y: y5 + 17.5 },
            radiusX: 6.0,
            radiusY: 6.0,
        };
        rt.FillEllipse(&dot_ellipse, &dot_brush);
        let dot_rim = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.8 }, None)?;
        rt.DrawEllipse(&dot_ellipse, &dot_rim, 1.2, None);

        rt.DrawText(&arrow_wide, text_format_small, &D2D_RECT_F { left: x + 67.0, top: y5 + 8.0, right: x + 78.0, bottom: y5 + 30.0 }, &arrow_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        // Line 6: Opacity Slider (💧 {pct}%)
        let y6 = y + 175.0;
        let op_pct = ((config.annotation.stroke_opacity - 0.05) / 0.95 * 100.0).clamp(0.0, 100.0);
        let op_label = format!("💧 {:.0}%", op_pct);
        let label_wide: Vec<u16> = op_label.encode_utf16().collect();
        let label_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 }, None)?;
        rt.DrawText(&label_wide, text_format_small, &D2D_RECT_F { left: x + 6.0, top: y6, right: x + 79.0, bottom: y6 + 13.0 }, &label_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let op_track_y = y6 + 15.0;
        let op_track_h = 5.0;
        let op_track_w = 73.0;
        let track_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: panel_op }, None)?;
        let op_track_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 6.0, top: op_track_y, right: x + 6.0 + op_track_w, bottom: op_track_y + op_track_h },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        rt.FillRoundedRectangle(&op_track_rect, &track_bg);

        let op_fill_w = op_track_w * (op_pct / 100.0);
        let fill_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.30, g: 0.55, b: 0.98, a: 1.0 }, None)?;
        let op_fill_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 6.0, top: op_track_y, right: x + 6.0 + op_fill_w, bottom: op_track_y + op_track_h },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        rt.FillRoundedRectangle(&op_fill_rect, &fill_bg);

        let thumb_x = x + 6.0 + op_fill_w;
        let thumb_ellipse = D2D1_ELLIPSE { point: D2D_POINT_2F { x: thumb_x, y: op_track_y + op_track_h * 0.5 }, radiusX: 5.5, radiusY: 5.5 };
        let thumb_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
        rt.FillEllipse(&thumb_ellipse, &thumb_brush);

        // Line 7: Size Slider (📏 {pct}%)
        let y7 = y + 206.0;
        let min_w = config.annotation.min_stroke_width.max(0.5);
        let max_w = config.annotation.max_stroke_width.max(min_w + 1.0);
        let size_pct = ((config.annotation.stroke_width - min_w) / (max_w - min_w) * 100.0).clamp(0.0, 100.0);
        let size_label = format!("📏 {:.0}%", size_pct);
        let size_wide: Vec<u16> = size_label.encode_utf16().collect();
        rt.DrawText(&size_wide, text_format_small, &D2D_RECT_F { left: x + 6.0, top: y7, right: x + 79.0, bottom: y7 + 13.0 }, &label_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

        let size_track_y = y7 + 15.0;
        let size_track_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 6.0, top: size_track_y, right: x + 6.0 + op_track_w, bottom: size_track_y + op_track_h },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        rt.FillRoundedRectangle(&size_track_rect, &track_bg);

        let size_fill_w = op_track_w * (size_pct / 100.0);
        let size_fill_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + 6.0, top: size_track_y, right: x + 6.0 + size_fill_w, bottom: size_track_y + op_track_h },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        rt.FillRoundedRectangle(&size_fill_rect, &fill_bg);

        let thumb_x7 = x + 6.0 + size_fill_w;
        let thumb_ellipse7 = D2D1_ELLIPSE { point: D2D_POINT_2F { x: thumb_x7, y: size_track_y + op_track_h * 0.5 }, radiusX: 5.5, radiusY: 5.5 };
        rt.FillEllipse(&thumb_ellipse7, &thumb_brush);

        // Line 8: Undo, Redo (Feather rotate-ccw, Feather rotate-cw) (Square 35x35px)
        let y8 = y + 237.0;
        draw_btn(x + 5.0, y8, x + 40.0, y8 + 35.0, FeatherIcon::Undo, false, false, !can_undo)?;
        draw_btn(x + 45.0, y8, x + 80.0, y8 + 35.0, FeatherIcon::Redo, false, false, !can_redo)?;

        // Line 9: Clear, Exit with 2-click Confirmation (Square 35x35px)
        let y9 = y + 277.0;
        let clear_icon = if self.confirm_clear { FeatherIcon::ConfirmQuestion } else { FeatherIcon::Trash };
        let exit_icon = if self.confirm_exit { FeatherIcon::ConfirmQuestion } else { FeatherIcon::Close };
        draw_btn(x + 5.0, y9, x + 40.0, y9 + 35.0, clear_icon, false, self.confirm_clear, false)?;
        draw_btn(x + 45.0, y9, x + 80.0, y9 + 35.0, exit_icon, false, self.confirm_exit, false)?;

        // Render Flyouts (Emoji & Color 3x3 grids)
        let flyout_rect = self.get_flyout_rect();
        let flyout_x = flyout_rect.left;
        let flyout_y = flyout_rect.top;

        let f_rect = D2D1_ROUNDED_RECT {
            rect: flyout_rect,
            radiusX: 10.0,
            radiusY: 10.0,
        };

        // 2. Render Emoji Flyout Selector
        if self.emoji_flyout_open {
            let f_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.98 }, None)?;
            rt.FillRoundedRectangle(&f_rect, &f_bg);

            let f_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.35, g: 0.45, b: 0.70, a: 0.95 }, None)?;
            rt.DrawRoundedRectangle(&f_rect, &f_border, 1.5, None);

            let e_size = 34.0;
            let e_gap = 5.0;
            let start_x = flyout_x + 6.0;
            let start_y = flyout_y + 6.0;

            for (idx, emoji_char) in config.annotation.custom_emojis.iter().enumerate().take(9) {
                let col = idx % 3;
                let row = idx / 3;
                let ex = start_x + col as f32 * (e_size + e_gap);
                let ey = start_y + row as f32 * (e_size + e_gap);

                let is_sel = config.annotation.selected_emoji == *emoji_char;
                let item_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: ex, top: ey, right: ex + e_size, bottom: ey + e_size },
                    radiusX: 6.0,
                    radiusY: 6.0,
                };

                let item_bg = if is_sel {
                    rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 }, None)?
                } else {
                    rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.9 }, None)?
                };
                rt.FillRoundedRectangle(&item_rect, &item_bg);

                let wide_e: Vec<u16> = emoji_char.encode_utf16().collect();
                let e_box = D2D_RECT_F { left: ex + 3.0, top: ey + 3.0, right: ex + e_size, bottom: ey + e_size };
                rt.DrawText(&wide_e, text_format_emoji, &e_box, &thumb_brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, DWRITE_MEASURING_MODE_NATURAL);
            }
        }

        // 3. Render Color Flyout Selector
        if self.color_flyout_open {
            let f_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.98 }, None)?;
            rt.FillRoundedRectangle(&f_rect, &f_bg);

            let f_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.35, g: 0.45, b: 0.70, a: 0.95 }, None)?;
            rt.DrawRoundedRectangle(&f_rect, &f_border, 1.5, None);

            let c_size = 34.0;
            let c_gap = 5.0;
            let start_x = flyout_x + 6.0;
            let start_y = flyout_y + 6.0;

            for (idx, &col) in config.annotation.color_presets.iter().enumerate().take(9) {
                let col_idx = idx % 3;
                let row_idx = idx / 3;
                let cx = start_x + col_idx as f32 * (c_size + c_gap);
                let cy = start_y + row_idx as f32 * (c_size + c_gap);

                let is_sel = (col[0] - config.annotation.current_color[0]).abs() < 0.03
                    && (col[1] - config.annotation.current_color[1]).abs() < 0.03
                    && (col[2] - config.annotation.current_color[2]).abs() < 0.03;

                let item_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: cx, top: cy, right: cx + c_size, bottom: cy + c_size },
                    radiusX: 6.0,
                    radiusY: 6.0,
                };

                let col_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: col[0], g: col[1], b: col[2], a: 1.0 }, None)?;
                rt.FillRoundedRectangle(&item_rect, &col_brush);

                if is_sel {
                    let sel_border = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
                    rt.DrawRoundedRectangle(&item_rect, &sel_border, 2.5, None);
                } else {
                    let rim = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.4 }, None)?;
                    rt.DrawRoundedRectangle(&item_rect, &rim, 1.0, None);
                }
            }
        }

        Ok(())
    }
}

/// Helper to render vector Feather Icons with exact aesthetic balance and uniform size
unsafe fn draw_feather_icon(
    rt: &ID2D1RenderTarget,
    cx: f32,
    cy: f32,
    icon: FeatherIcon,
    brush: &ID2D1SolidColorBrush,
    stroke_style: Option<&ID2D1StrokeStyle>,
) -> Result<()> {
    match icon {
        FeatherIcon::Pen => {
            // Feather edit-2
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 4.5, y: cy + 4.5 },
                D2D_POINT_2F { x: cx + 4.5, y: cy - 4.5 },
                brush,
                2.0,
                stroke_style,
            );
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 4.5, y: cy + 4.5 },
                D2D_POINT_2F { x: cx - 6.5, y: cy + 6.5 },
                brush,
                2.0,
                stroke_style,
            );
            rt.DrawLine(
                D2D_POINT_2F { x: cx + 4.5, y: cy - 4.5 },
                D2D_POINT_2F { x: cx + 6.5, y: cy - 6.5 },
                brush,
                2.0,
                stroke_style,
            );
        }
        FeatherIcon::Type => {
            // Feather 'type' (T text icon)
            // Top crossbar
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 6.5, y: cy - 6.0 },
                D2D_POINT_2F { x: cx + 6.5, y: cy - 6.0 },
                brush,
                2.0,
                stroke_style,
            );
            // Downward serifs on top crossbar
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 6.5, y: cy - 6.0 },
                D2D_POINT_2F { x: cx - 6.5, y: cy - 3.5 },
                brush,
                1.6,
                stroke_style,
            );
            rt.DrawLine(
                D2D_POINT_2F { x: cx + 6.5, y: cy - 6.0 },
                D2D_POINT_2F { x: cx + 6.5, y: cy - 3.5 },
                brush,
                1.6,
                stroke_style,
            );
            // Center stem
            rt.DrawLine(
                D2D_POINT_2F { x: cx, y: cy - 6.0 },
                D2D_POINT_2F { x: cx, y: cy + 6.5 },
                brush,
                2.0,
                stroke_style,
            );
            // Bottom serif baseline
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 3.0, y: cy + 6.5 },
                D2D_POINT_2F { x: cx + 3.0, y: cy + 6.5 },
                brush,
                1.6,
                stroke_style,
            );
        }
        FeatherIcon::Eraser => {
            // Feather angled eraser block
            let er = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cx - 7.0, top: cy - 4.5, right: cx + 7.0, bottom: cy + 4.5 },
                radiusX: 2.0,
                radiusY: 2.0,
            };
            rt.DrawRoundedRectangle(&er, brush, 1.8, None);
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 1.5, y: cy - 4.5 },
                D2D_POINT_2F { x: cx - 1.5, y: cy + 4.5 },
                brush,
                1.8,
                None,
            );
        }
        FeatherIcon::Move => {
            // Feather 'move' (4-way directional cross arrows)
            // Vertical axis
            rt.DrawLine(
                D2D_POINT_2F { x: cx, y: cy - 6.5 },
                D2D_POINT_2F { x: cx, y: cy + 6.5 },
                brush,
                1.8,
                stroke_style,
            );
            // Horizontal axis
            rt.DrawLine(
                D2D_POINT_2F { x: cx - 6.5, y: cy },
                D2D_POINT_2F { x: cx + 6.5, y: cy },
                brush,
                1.8,
                stroke_style,
            );
            // Up arrow head
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.8, y: cy - 3.7 }, D2D_POINT_2F { x: cx, y: cy - 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 2.8, y: cy - 3.7 }, D2D_POINT_2F { x: cx, y: cy - 6.5 }, brush, 1.8, stroke_style);
            // Down arrow head
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.8, y: cy + 3.7 }, D2D_POINT_2F { x: cx, y: cy + 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 2.8, y: cy + 3.7 }, D2D_POINT_2F { x: cx, y: cy + 6.5 }, brush, 1.8, stroke_style);
            // Left arrow head
            rt.DrawLine(D2D_POINT_2F { x: cx - 3.7, y: cy - 2.8 }, D2D_POINT_2F { x: cx - 6.5, y: cy }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 3.7, y: cy + 2.8 }, D2D_POINT_2F { x: cx - 6.5, y: cy }, brush, 1.8, stroke_style);
            // Right arrow head
            rt.DrawLine(D2D_POINT_2F { x: cx + 3.7, y: cy - 2.8 }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 3.7, y: cy + 2.8 }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
        }
        FeatherIcon::Circle { filled } => {
            let ell = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: 7.0, radiusY: 7.0 };
            if filled {
                rt.FillEllipse(&ell, brush);
            } else {
                rt.DrawEllipse(&ell, brush, 2.0, None);
            }
        }
        FeatherIcon::Square { filled } => {
            let r = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cx - 6.5, top: cy - 6.5, right: cx + 6.5, bottom: cy + 6.5 },
                radiusX: 2.0,
                radiusY: 2.0,
            };
            if filled {
                rt.FillRoundedRectangle(&r, brush);
            } else {
                rt.DrawRoundedRectangle(&r, brush, 2.0, None);
            }
        }
        FeatherIcon::Arrow { wavy } => {
            if wavy {
                // Sine wave stem with arrowhead
                rt.DrawLine(D2D_POINT_2F { x: cx - 6.0, y: cy + 5.5 }, D2D_POINT_2F { x: cx - 2.0, y: cy + 1.0 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx - 2.0, y: cy + 1.0 }, D2D_POINT_2F { x: cx + 2.0, y: cy + 4.0 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx + 2.0, y: cy + 4.0 }, D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, brush, 1.8, stroke_style);
                // Wings
                rt.DrawLine(D2D_POINT_2F { x: cx - 0.5, y: cy - 5.5 }, D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, brush, 2.0, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, D2D_POINT_2F { x: cx + 5.5, y: cy + 0.5 }, brush, 2.0, stroke_style);
            } else {
                // Feather arrow-up-right
                rt.DrawLine(
                    D2D_POINT_2F { x: cx - 5.5, y: cy + 5.5 },
                    D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 },
                    brush,
                    2.0,
                    stroke_style,
                );
                rt.DrawLine(
                    D2D_POINT_2F { x: cx - 0.5, y: cy - 5.5 },
                    D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 },
                    brush,
                    2.0,
                    stroke_style,
                );
                rt.DrawLine(
                    D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 },
                    D2D_POINT_2F { x: cx + 5.5, y: cy + 0.5 },
                    brush,
                    2.0,
                    stroke_style,
                );
            }
        }
        FeatherIcon::Line { dotted, wavy } => {
            if wavy {
                // Mini sine wave indicator
                rt.DrawLine(D2D_POINT_2F { x: cx - 6.5, y: cy }, D2D_POINT_2F { x: cx - 3.5, y: cy - 3.5 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx - 3.5, y: cy - 3.5 }, D2D_POINT_2F { x: cx, y: cy }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx, y: cy }, D2D_POINT_2F { x: cx + 3.5, y: cy + 3.5 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx + 3.5, y: cy + 3.5 }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
            } else if dotted {
                // 4 circular dots along line
                for i in 0..4 {
                    let t = i as f32 / 3.0;
                    let px = (cx - 6.0) * (1.0 - t) + (cx + 6.0) * t;
                    let py = (cy + 5.0) * (1.0 - t) + (cy - 5.0) * t;
                    let dot = D2D1_ELLIPSE { point: D2D_POINT_2F { x: px, y: py }, radiusX: 1.2, radiusY: 1.2 };
                    rt.FillEllipse(&dot, brush);
                }
            } else {
                // Feather minus / ruler
                rt.DrawLine(
                    D2D_POINT_2F { x: cx - 7.0, y: cy + 5.0 },
                    D2D_POINT_2F { x: cx + 7.0, y: cy - 5.0 },
                    brush,
                    2.2,
                    stroke_style,
                );
            }
        }
        FeatherIcon::Undo => {
            // Feather rotate-ccw
            let arc_rect = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx + 0.5, y: cy + 0.5 }, radiusX: 6.0, radiusY: 6.0 };
            rt.DrawEllipse(&arc_rect, brush, 1.8, None);
            rt.DrawLine(D2D_POINT_2F { x: cx - 5.5, y: cy - 4.0 }, D2D_POINT_2F { x: cx - 5.5, y: cy + 0.5 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 5.5, y: cy - 4.0 }, D2D_POINT_2F { x: cx - 1.0, y: cy - 4.0 }, brush, 2.0, stroke_style);
        }
        FeatherIcon::Redo => {
            // Feather rotate-cw
            let arc_rect = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx - 0.5, y: cy + 0.5 }, radiusX: 6.0, radiusY: 6.0 };
            rt.DrawEllipse(&arc_rect, brush, 1.8, None);
            rt.DrawLine(D2D_POINT_2F { x: cx + 5.5, y: cy - 4.0 }, D2D_POINT_2F { x: cx + 5.5, y: cy + 0.5 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 5.5, y: cy - 4.0 }, D2D_POINT_2F { x: cx + 1.0, y: cy - 4.0 }, brush, 2.0, stroke_style);
        }
        FeatherIcon::Trash => {
            // Feather trash-2
            rt.DrawLine(D2D_POINT_2F { x: cx - 6.5, y: cy - 5.0 }, D2D_POINT_2F { x: cx + 6.5, y: cy - 5.0 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.5, y: cy - 5.0 }, D2D_POINT_2F { x: cx - 2.5, y: cy - 7.0 }, brush, 1.5, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.5, y: cy - 7.0 }, D2D_POINT_2F { x: cx + 2.5, y: cy - 7.0 }, brush, 1.5, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 2.5, y: cy - 7.0 }, D2D_POINT_2F { x: cx + 2.5, y: cy - 5.0 }, brush, 1.5, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 5.0, y: cy - 5.0 }, D2D_POINT_2F { x: cx - 4.0, y: cy + 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 4.0, y: cy + 6.5 }, D2D_POINT_2F { x: cx + 4.0, y: cy + 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 4.0, y: cy + 6.5 }, D2D_POINT_2F { x: cx + 5.0, y: cy - 5.0 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx, y: cy - 2.0 }, D2D_POINT_2F { x: cx, y: cy + 3.5 }, brush, 1.5, stroke_style);
        }
        FeatherIcon::Close => {
            // Feather x
            rt.DrawLine(D2D_POINT_2F { x: cx - 5.0, y: cy - 5.0 }, D2D_POINT_2F { x: cx + 5.0, y: cy + 5.0 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 5.0, y: cy - 5.0 }, D2D_POINT_2F { x: cx - 5.0, y: cy + 5.0 }, brush, 2.0, stroke_style);
        }
        FeatherIcon::ConfirmQuestion => {
            // Feather help-circle
            let outer = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: 7.2, radiusY: 7.2 };
            rt.DrawEllipse(&outer, brush, 1.8, None);
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.5, y: cy - 3.5 }, D2D_POINT_2F { x: cx, y: cy - 4.8 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx, y: cy - 4.8 }, D2D_POINT_2F { x: cx + 2.5, y: cy - 2.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 2.5, y: cy - 2.5 }, D2D_POINT_2F { x: cx, y: cy - 0.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx, y: cy - 0.5 }, D2D_POINT_2F { x: cx, y: cy + 1.8 }, brush, 1.8, stroke_style);
            let dot = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy + 4.2 }, radiusX: 0.9, radiusY: 0.9 };
            rt.FillEllipse(&dot, brush);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolbar_hit_test_and_actions() {
        let mut tb = DrawingToolbar::new(100.0, 100.0);
        let mut cfg = AppConfig::default();
        let mut tool = ActiveTool::None;
        let mut mgr = AnnotationManager::new();
        let mut needs_exit = false;

        // Inside panel bounds (width: 85, height: 320)
        assert!(tb.hit_test(Point2D::new(150.0, 150.0)));
        // Outside panel
        assert!(!tb.hit_test(Point2D::new(50.0, 50.0)));

        // Click Brush (Line 2, button 1: x + 5..x + 40, y + 15..y + 50)
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, 130.0), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Freehand);

        // Click Brush 2nd time: toggles to Text tool!
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, 130.0), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Text);

        // Click Brush 3rd time: toggles back to Freehand!
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, 130.0), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Freehand);

        // Click Eraser (Line 2, button 2: x + 45..x + 80, y + 15..y + 50)
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, 130.0), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Eraser);

        // Click Eraser 2nd time: toggles to Transform tool!
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, 130.0), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Transform);

        // Click Eraser 3rd time: toggles back to Eraser!
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, 130.0), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Eraser);

        // Click Clear (Line 9, button 1: x + 5..x + 40, y + 277..y + 312)
        let clear_y = 100.0 + 295.0;
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, clear_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(tb.confirm_clear);

        // Click Exit (Line 9, button 2: x + 45..x + 80, y + 277..y + 312)
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, clear_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(tb.confirm_exit);
        assert!(!tb.confirm_clear); // Cleared previous confirmation

        // Click Exit 2nd time: triggers exit!
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, clear_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(needs_exit);

        // Click Ellipse (Line 3, button 1: x + 5..x + 40, y + 55..y + 90)
        let ellipse_y = 100.0 + 72.0;
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, ellipse_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Ellipse);

        // Click Ellipse 2nd time: toggles filled!
        let filled_before = cfg.annotation.ellipse_filled;
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, ellipse_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(cfg.annotation.ellipse_filled, !filled_before);

        // Click Rectangle (Line 3, button 2: x + 45..x + 80, y + 55..y + 90)
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, ellipse_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert_eq!(tool, ActiveTool::Rect);

        // Line 5: Click Color button to open flyout (x + 45..x + 80, y + 135..y + 170)
        let color_btn_y = 100.0 + 150.0;
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, color_btn_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(tb.color_flyout_open);
        assert!(!tb.emoji_flyout_open);

        // Click Color button 2nd time: collapses flyout!
        let handled = tb.handle_lbutton_down(Point2D::new(165.0, color_btn_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(!tb.color_flyout_open);

        // Click Emoji button to open flyout
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, color_btn_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(tb.emoji_flyout_open);

        // Click Emoji button 2nd time: collapses flyout!
        let handled = tb.handle_lbutton_down(Point2D::new(125.0, color_btn_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(handled);
        assert!(!tb.emoji_flyout_open);

        // Click Arrow (Line 4, button 1: x + 5..x + 40, y + 95..y + 130)
        let arrow_y = 100.0 + 110.0;
        tb.handle_lbutton_down(Point2D::new(125.0, arrow_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert_eq!(tool, ActiveTool::Arrow);
        let wavy_before = cfg.annotation.arrow_wavy;
        // Click Arrow 2nd time: toggles arrow_wavy!
        tb.handle_lbutton_down(Point2D::new(125.0, arrow_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert_eq!(cfg.annotation.arrow_wavy, !wavy_before);

        // Click Line (Line 4, button 2: x + 45..x + 80, y + 95..y + 130)
        let line_x = 165.0;
        tb.handle_lbutton_down(Point2D::new(line_x, arrow_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert_eq!(tool, ActiveTool::Line);
        // Click Line 2nd time: cycles to dotted!
        tb.handle_lbutton_down(Point2D::new(line_x, arrow_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(cfg.annotation.line_dotted);
        assert!(!cfg.annotation.line_wavy);
        // Click Line 3rd time: cycles to wavy!
        tb.handle_lbutton_down(Point2D::new(line_x, arrow_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(!cfg.annotation.line_dotted);
        assert!(cfg.annotation.line_wavy);
        // Click Line 4th time: cycles back to solid!
        tb.handle_lbutton_down(Point2D::new(line_x, arrow_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit);
        assert!(!cfg.annotation.line_dotted);
        assert!(!cfg.annotation.line_wavy);
    }
}
