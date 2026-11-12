use std::mem::ManuallyDrop;

use crate::annotation::{ActiveTool, AnnotationManager};
use crate::config::AppConfig;
use crate::geometry::Point2D;

use windows::core::*;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraggingSlider {
    Opacity,
    Size,
    Softness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatherIcon {
    Pen,
    Type,
    Eraser,
    Move,
    Burn,
    Dodge,
    Circle { filled: bool },
    Square { filled: bool },
    Arrow { wavy: bool },
    Line { dotted: bool, wavy: bool },
    Undo,
    Redo,
    Trash,
    Eye,
    EyeOff,
    Settings,
    Close,
    ConfirmQuestion,
}

#[derive(Clone, Copy)]
struct ShapeFlyoutItem {
    tool: ActiveTool,
    line_dotted: bool,
    line_wavy: bool,
    arrow_wavy: bool,
    filled: bool,
    icon: FeatherIcon,
}

const SHAPE_FLYOUT_ITEMS: [ShapeFlyoutItem; 9] = [
    ShapeFlyoutItem {
        tool: ActiveTool::Line,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: false,
        filled: false,
        icon: FeatherIcon::Line { dotted: false, wavy: false },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Line,
        line_dotted: true,
        line_wavy: false,
        arrow_wavy: false,
        filled: false,
        icon: FeatherIcon::Line { dotted: true, wavy: false },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Line,
        line_dotted: false,
        line_wavy: true,
        arrow_wavy: false,
        filled: false,
        icon: FeatherIcon::Line { dotted: false, wavy: true },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Arrow,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: false,
        filled: false,
        icon: FeatherIcon::Arrow { wavy: false },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Arrow,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: true,
        filled: false,
        icon: FeatherIcon::Arrow { wavy: true },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Ellipse,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: false,
        filled: false,
        icon: FeatherIcon::Circle { filled: false },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Ellipse,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: false,
        filled: true,
        icon: FeatherIcon::Circle { filled: true },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Rect,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: false,
        filled: false,
        icon: FeatherIcon::Square { filled: false },
    },
    ShapeFlyoutItem {
        tool: ActiveTool::Rect,
        line_dotted: false,
        line_wavy: false,
        arrow_wavy: false,
        filled: true,
        icon: FeatherIcon::Square { filled: true },
    },
];

// Compact 3-buttons-per-row layout (36x36 borderless buttons, 128x270 card)
const BTN_SIZE: f32 = 36.0;
const COL1_X: f32 = 6.0;
const COL2_X: f32 = 46.0;
const COL3_X: f32 = 86.0;
const ROW1_Y: f32 = 14.0;
const ROW2_Y: f32 = 54.0;
const SLIDER_SOFTNESS_Y: f32 = 96.0;
const SLIDER_OPACITY_Y: f32 = 126.0;
const SLIDER_SIZE_Y: f32 = 156.0;
const ROW4_Y: f32 = 188.0;
const ROW5_Y: f32 = 228.0;

const FLYOUT_SIZE: f32 = 126.0;
const FLYOUT_PAD: f32 = 7.0;
const FLYOUT_CELL: f32 = 34.0;
const FLYOUT_GAP: f32 = 5.0;

pub struct DrawingToolbar {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,

    pub is_dragging: bool,
    pub drag_offset_x: f32,
    pub drag_offset_y: f32,

    pub active_slider: Option<DraggingSlider>,
    pub shapes_flyout_open: bool,
    pub emoji_flyout_open: bool,
    pub color_flyout_open: bool,

    pub confirm_clear: bool,
    pub confirm_exit: bool,
    pub pending_open_settings: bool,
}

impl DrawingToolbar {
    pub fn new(x: f32, y: f32) -> Self {
        let (max_w, max_h) = unsafe {
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN) as f32;
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN) as f32;
            (
                if vw > 128.0 { vw - 128.0 } else { 1800.0 },
                if vh > 270.0 { vh - 270.0 } else { 900.0 },
            )
        };
        Self {
            x: x.clamp(0.0, max_w),
            y: y.clamp(0.0, max_h),
            width: 128.0,
            height: 270.0,
            is_dragging: false,
            drag_offset_x: 0.0,
            drag_offset_y: 0.0,
            active_slider: None,
            shapes_flyout_open: false,
            emoji_flyout_open: false,
            color_flyout_open: false,
            confirm_clear: false,
            confirm_exit: false,
            pending_open_settings: false,
        }
    }

    pub fn has_open_flyout(&self) -> bool {
        self.shapes_flyout_open || self.emoji_flyout_open || self.color_flyout_open
    }

    pub fn close_flyouts(&mut self) {
        self.shapes_flyout_open = false;
        self.emoji_flyout_open = false;
        self.color_flyout_open = false;
    }

    /// Reset any pending 2-click confirmations
    pub fn reset_confirmations(&mut self) {
        self.confirm_clear = false;
        self.confirm_exit = false;
    }

    /// Calculate active flyout bounding rectangle with multi-monitor auto left/right & vertical clamping
    pub fn get_flyout_rect(&self) -> D2D_RECT_F {
        let anchor_y = self.y + ROW2_Y;

        let (work_left, work_top, work_right, work_bottom) = unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f32;
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f32;
            let center_screen = POINT {
                x: (vx + self.x + self.width * 0.5) as i32,
                y: (vy + self.y + self.height * 0.5) as i32,
            };
            let hmon = MonitorFromPoint(center_screen, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                (
                    mi.rcWork.left as f32 - vx,
                    mi.rcWork.top as f32 - vy,
                    mi.rcWork.right as f32 - vx,
                    mi.rcWork.bottom as f32 - vy,
                )
            } else {
                let sw = GetSystemMetrics(SM_CXSCREEN) as f32;
                let sh = GetSystemMetrics(SM_CYSCREEN) as f32;
                (
                    0.0,
                    0.0,
                    if sw > 0.0 { sw } else { 1920.0 },
                    if sh > 0.0 { sh } else { 1080.0 },
                )
            }
        };

        let flyout_x = if self.x + self.width + 6.0 + FLYOUT_SIZE > work_right {
            (self.x - FLYOUT_SIZE - 6.0).max(work_left + 6.0)
        } else {
            (self.x + self.width + 6.0).max(work_left + 6.0)
        };

        let max_y = (work_bottom - FLYOUT_SIZE - 6.0).max(work_top + 6.0);
        let flyout_y = (anchor_y - 16.0).clamp(work_top + 6.0, max_y);

        D2D_RECT_F {
            left: flyout_x,
            top: flyout_y,
            right: flyout_x + FLYOUT_SIZE,
            bottom: flyout_y + FLYOUT_SIZE,
        }
    }

    /// Check if point is inside the toolbar panel bounds or open flyout
    pub fn hit_test(&self, p: Point2D) -> bool {
        let inside_panel = p.x >= self.x
            && p.x <= self.x + self.width
            && p.y >= self.y
            && p.y <= self.y + self.height;

        if inside_panel {
            return true;
        }

        if self.has_open_flyout() {
            let r = self.get_flyout_rect();
            if p.x >= r.left && p.x <= r.right && p.y >= r.top && p.y <= r.bottom {
                return true;
            }
        }

        false
    }

    fn point_in_btn(&self, p: Point2D, rel_x: f32, rel_y: f32) -> bool {
        let bx = self.x + rel_x;
        let by = self.y + rel_y;
        p.x >= bx && p.x <= bx + BTN_SIZE && p.y >= by && p.y <= by + BTN_SIZE
    }

    /// Map a 3x3 Color flyout grid index (0..9) to either Invert (center = 4) or preset index (0..8)
    fn color_grid_slot(grid_idx: usize) -> Option<usize> {
        if grid_idx == 4 {
            None
        } else if grid_idx < 4 {
            Some(grid_idx)
        } else {
            Some(grid_idx - 1)
        }
    }

    fn current_shape_icon(config: &AppConfig, active_tool: ActiveTool) -> FeatherIcon {
        let shape_tool = if active_tool.is_shape() {
            active_tool
        } else {
            ActiveTool::from_str(&config.annotation.last_shape_tool)
        };
        match shape_tool {
            ActiveTool::Line => FeatherIcon::Line {
                dotted: config.annotation.line_dotted,
                wavy: config.annotation.line_wavy,
            },
            ActiveTool::Arrow => FeatherIcon::Arrow {
                wavy: config.annotation.arrow_wavy,
            },
            ActiveTool::Ellipse => FeatherIcon::Circle {
                filled: config.annotation.ellipse_filled,
            },
            _ => FeatherIcon::Square {
                filled: config.annotation.rect_filled,
            },
        }
    }

    fn is_shape_item_selected(item: ShapeFlyoutItem, config: &AppConfig, active_tool: ActiveTool) -> bool {
        let armed_tool = if active_tool.is_shape() {
            active_tool
        } else {
            ActiveTool::from_str(&config.annotation.last_shape_tool)
        };
        if item.tool != armed_tool {
            return false;
        }
        match item.tool {
            ActiveTool::Line => {
                config.annotation.line_dotted == item.line_dotted
                    && config.annotation.line_wavy == item.line_wavy
            }
            ActiveTool::Arrow => config.annotation.arrow_wavy == item.arrow_wavy,
            ActiveTool::Ellipse => config.annotation.ellipse_filled == item.filled,
            ActiveTool::Rect => config.annotation.rect_filled == item.filled,
            _ => false,
        }
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

        // While annotations visibility is OFF (locked), all drawing tools, flyouts,
        // sliders, and undo/redo/clear buttons are disabled. Only dragging the panel,
        // unlocking via Eye, opening Settings, or Exiting drawing mode stay active.
        if annotation_mgr.annotations_hidden {
            self.close_flyouts();
            if !self.hit_test(p) {
                self.reset_confirmations();
                return false;
            }
            if p.y >= self.y && p.y <= self.y + ROW1_Y && p.x >= self.x && p.x <= self.x + self.width {
                self.is_dragging = true;
                self.drag_offset_x = p.x - self.x;
                self.drag_offset_y = p.y - self.y;
                self.reset_confirmations();
                return true;
            }
            if self.point_in_btn(p, COL1_X, ROW5_Y) {
                self.reset_confirmations();
                annotation_mgr.annotations_hidden = false;
                return true;
            }
            if self.point_in_btn(p, COL2_X, ROW5_Y) {
                self.reset_confirmations();
                self.pending_open_settings = true;
                return true;
            }
            if self.point_in_btn(p, COL3_X, ROW5_Y) {
                if self.confirm_exit {
                    self.confirm_exit = false;
                    *needs_exit = true;
                } else {
                    self.confirm_exit = true;
                    self.confirm_clear = false;
                }
                return true;
            }
            self.reset_confirmations();
            return true;
        }

        let flyout_rect = self.get_flyout_rect();
        let flyout_x = flyout_rect.left;
        let flyout_y = flyout_rect.top;
        let flyout_w = flyout_rect.right - flyout_rect.left;
        let flyout_h = flyout_rect.bottom - flyout_rect.top;
        let inside_flyout = p.x >= flyout_x
            && p.x <= flyout_x + flyout_w
            && p.y >= flyout_y
            && p.y <= flyout_y + flyout_h;

        // Row 2 Flyout buttons (Shapes, Emoji, Color — open/collapse cleanly from 1st click)
        if self.point_in_btn(p, COL1_X, ROW2_Y) {
            self.reset_confirmations();
            let remembered = ActiveTool::from_str(&config.annotation.last_shape_tool);
            let next_tool = if remembered.is_shape() {
                remembered
            } else {
                ActiveTool::Rect
            };
            if *active_tool != next_tool {
                *active_tool = next_tool;
                config.annotation.selected_tool = active_tool.as_str().to_string();
                config.annotation.last_shape_tool = active_tool.as_str().to_string();
                config.annotation.load_tool_settings(active_tool.as_str());
            }
            self.shapes_flyout_open = !self.shapes_flyout_open;
            self.emoji_flyout_open = false;
            self.color_flyout_open = false;
            let _ = config.save();
            return true;
        }

        if self.point_in_btn(p, COL2_X, ROW2_Y) {
            self.reset_confirmations();
            if *active_tool != ActiveTool::Emoji {
                *active_tool = ActiveTool::Emoji;
                config.annotation.selected_tool = "emoji".to_string();
                config.annotation.load_tool_settings("emoji");
            }
            self.emoji_flyout_open = !self.emoji_flyout_open;
            self.shapes_flyout_open = false;
            self.color_flyout_open = false;
            let _ = config.save();
            return true;
        }

        if self.point_in_btn(p, COL3_X, ROW2_Y) {
            self.reset_confirmations();
            self.color_flyout_open = !self.color_flyout_open;
            self.shapes_flyout_open = false;
            self.emoji_flyout_open = false;
            return true;
        }

        // 2. Active Flyout Hit-Tests
        if self.shapes_flyout_open {
            if inside_flyout {
                let start_x = flyout_x + FLYOUT_PAD;
                let start_y = flyout_y + FLYOUT_PAD;
                for (idx, item) in SHAPE_FLYOUT_ITEMS.iter().enumerate() {
                    let col = idx % 3;
                    let row = idx / 3;
                    let ix = start_x + col as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let iy = start_y + row as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    if p.x >= ix && p.x <= ix + FLYOUT_CELL && p.y >= iy && p.y <= iy + FLYOUT_CELL {
                        *active_tool = item.tool;
                        config.annotation.selected_tool = item.tool.as_str().to_string();
                        config.annotation.last_shape_tool = item.tool.as_str().to_string();
                        config.annotation.load_tool_settings(active_tool.as_str());
                        match item.tool {
                            ActiveTool::Line => {
                                config.annotation.line_dotted = item.line_dotted;
                                config.annotation.line_wavy = item.line_wavy;
                            }
                            ActiveTool::Arrow => {
                                config.annotation.arrow_wavy = item.arrow_wavy;
                            }
                            ActiveTool::Ellipse => {
                                config.annotation.ellipse_filled = item.filled;
                            }
                            ActiveTool::Rect => {
                                config.annotation.rect_filled = item.filled;
                            }
                            _ => {}
                        }
                        self.shapes_flyout_open = false;
                        let _ = config.save();
                        return true;
                    }
                }
                return true;
            } else {
                self.shapes_flyout_open = false;
            }
        }

        if self.emoji_flyout_open {
            if inside_flyout {
                let start_x = flyout_x + FLYOUT_PAD;
                let start_y = flyout_y + FLYOUT_PAD;
                for (idx, emoji_char) in config.annotation.custom_emojis.iter().enumerate().take(9) {
                    let col = idx % 3;
                    let row = idx / 3;
                    let ex = start_x + col as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let ey = start_y + row as f32 * (FLYOUT_CELL + FLYOUT_GAP);

                    if p.x >= ex && p.x <= ex + FLYOUT_CELL && p.y >= ey && p.y <= ey + FLYOUT_CELL {
                        config.annotation.selected_emoji = emoji_char.clone();
                        if *active_tool != ActiveTool::Emoji {
                            *active_tool = ActiveTool::Emoji;
                            config.annotation.selected_tool = "emoji".to_string();
                            config.annotation.load_tool_settings("emoji");
                        }
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

        if self.color_flyout_open {
            if inside_flyout {
                let start_x = flyout_x + FLYOUT_PAD;
                let start_y = flyout_y + FLYOUT_PAD;
                for grid_idx in 0..9 {
                    let col_idx = grid_idx % 3;
                    let row_idx = grid_idx / 3;
                    let cx = start_x + col_idx as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let cy = start_y + row_idx as f32 * (FLYOUT_CELL + FLYOUT_GAP);

                    if p.x >= cx && p.x <= cx + FLYOUT_CELL && p.y >= cy && p.y <= cy + FLYOUT_CELL {
                        if let Some(preset_idx) = Self::color_grid_slot(grid_idx) {
                            if let Some(&col) = config.annotation.color_presets.get(preset_idx) {
                                config.annotation.invert_color = false;
                                config.annotation.current_color = col;
                                if config.sync_colors {
                                    config.highlighter.color = col;
                                    config.laser.color = col;
                                }
                            }
                        } else {
                            config.annotation.invert_color = true;
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

        let x = self.x;
        let y = self.y;

        // Top Bar Handle (move around screen)
        if p.y >= y && p.y <= y + ROW1_Y && p.x >= x && p.x <= x + self.width {
            self.is_dragging = true;
            self.drag_offset_x = p.x - self.x;
            self.drag_offset_y = p.y - self.y;
            self.reset_confirmations();
            return true;
        }

        // Row 1 Col 1: Pen / Text Toggle (remembers its own state independently)
        if self.point_in_btn(p, COL1_X, ROW1_Y) {
            self.reset_confirmations();
            *active_tool = if *active_tool == ActiveTool::Freehand {
                ActiveTool::Text
            } else if *active_tool == ActiveTool::Text {
                ActiveTool::Freehand
            } else if config.annotation.last_pen_text == "text" {
                ActiveTool::Text
            } else {
                ActiveTool::Freehand
            };
            config.annotation.last_pen_text = active_tool.as_str().to_string();
            config.annotation.selected_tool = active_tool.as_str().to_string();
            config.annotation.load_tool_settings(active_tool.as_str());
            let _ = config.save();
            return true;
        }

        // Row 1 Col 2: Burn / Dodge Toggle (remembers its own state independently)
        if self.point_in_btn(p, COL2_X, ROW1_Y) {
            self.reset_confirmations();
            *active_tool = if *active_tool == ActiveTool::Burn {
                ActiveTool::Dodge
            } else if *active_tool == ActiveTool::Dodge {
                ActiveTool::Burn
            } else if config.annotation.last_burn_dodge == "dodge" {
                ActiveTool::Dodge
            } else {
                ActiveTool::Burn
            };
            config.annotation.last_burn_dodge = active_tool.as_str().to_string();
            config.annotation.selected_tool = active_tool.as_str().to_string();
            config.annotation.load_tool_settings(active_tool.as_str());
            let _ = config.save();
            return true;
        }

        // Row 1 Col 3: Eraser / Transform Toggle (remembers its own state independently)
        if self.point_in_btn(p, COL3_X, ROW1_Y) {
            self.reset_confirmations();
            *active_tool = if *active_tool == ActiveTool::Eraser {
                ActiveTool::Transform
            } else if *active_tool == ActiveTool::Transform {
                ActiveTool::Eraser
            } else if config.annotation.last_eraser_transform == "transform" {
                ActiveTool::Transform
            } else {
                ActiveTool::Eraser
            };
            config.annotation.last_eraser_transform = active_tool.as_str().to_string();
            config.annotation.selected_tool = active_tool.as_str().to_string();
            let _ = config.save();
            return true;
        }

        // Slider 1 (1st place): Softness (for Freehand, Burn, Dodge; disabled for Shapes/Emoji/Text)
        let y_soft = y + SLIDER_SOFTNESS_Y;
        if p.y >= y_soft - 2.0 && p.y <= y_soft + 27.0 && p.x >= x + 2.0 && p.x <= x + self.width - 2.0 {
            self.reset_confirmations();
            if active_tool.supports_softness() {
                self.active_slider = Some(DraggingSlider::Softness);
                self.update_softness_slider(p.x, config, *active_tool);
            }
            return true;
        }

        // Slider 2: Opacity
        let y_op = y + SLIDER_OPACITY_Y;
        if p.y >= y_op - 2.0 && p.y <= y_op + 27.0 && p.x >= x + 2.0 && p.x <= x + self.width - 2.0 {
            self.reset_confirmations();
            self.active_slider = Some(DraggingSlider::Opacity);
            self.update_opacity_slider(p.x, config, *active_tool);
            return true;
        }

        // Slider 3: Size
        let y_sz = y + SLIDER_SIZE_Y;
        if p.y >= y_sz - 2.0 && p.y <= y_sz + 27.0 && p.x >= x + 2.0 && p.x <= x + self.width - 2.0 {
            self.reset_confirmations();
            self.active_slider = Some(DraggingSlider::Size);
            self.update_size_slider(p.x, config, *active_tool);
            return true;
        }

        // Row 4: Undo, Redo, Clear
        if self.point_in_btn(p, COL1_X, ROW4_Y) {
            self.reset_confirmations();
            if annotation_mgr.can_undo() {
                annotation_mgr.undo();
            }
            return true;
        }
        if self.point_in_btn(p, COL2_X, ROW4_Y) {
            self.reset_confirmations();
            if annotation_mgr.can_redo() {
                annotation_mgr.redo();
            }
            return true;
        }
        if self.point_in_btn(p, COL3_X, ROW4_Y) {
            if self.confirm_clear {
                annotation_mgr.clear();
                self.confirm_clear = false;
            } else {
                self.confirm_clear = true;
                self.confirm_exit = false;
            }
            return true;
        }

        // Row 5: Hide/Show Annotations (Eye), Settings (Gear), Exit (Close)
        if self.point_in_btn(p, COL1_X, ROW5_Y) {
            self.reset_confirmations();
            annotation_mgr.annotations_hidden = true;
            self.close_flyouts();
            self.active_slider = None;
            return true;
        }
        if self.point_in_btn(p, COL2_X, ROW5_Y) {
            self.reset_confirmations();
            self.pending_open_settings = true;
            return true;
        }
        if self.point_in_btn(p, COL3_X, ROW5_Y) {
            if self.confirm_exit {
                self.confirm_exit = false;
                *needs_exit = true;
            } else {
                self.confirm_exit = true;
                self.confirm_clear = false;
            }
            return true;
        }

        self.reset_confirmations();
        true
    }

    /// Handle mouse move while interacting with the toolbar
    pub fn handle_mouse_move(&mut self, p: Point2D, config: &mut AppConfig, active_tool: ActiveTool) -> bool {
        if self.is_dragging {
            self.x = (p.x - self.drag_offset_x).max(0.0);
            self.y = (p.y - self.drag_offset_y).max(0.0);
            config.annotation.toolbar_x = self.x;
            config.annotation.toolbar_y = self.y;
            return true;
        }

        if let Some(slider) = self.active_slider {
            match slider {
                DraggingSlider::Opacity => self.update_opacity_slider(p.x, config, active_tool),
                DraggingSlider::Size => self.update_size_slider(p.x, config, active_tool),
                DraggingSlider::Softness => self.update_softness_slider(p.x, config, active_tool),
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

    fn update_opacity_slider(&mut self, mouse_x: f32, config: &mut AppConfig, active_tool: ActiveTool) {
        let track_start = self.x + 6.0;
        let track_w = self.width - 12.0;
        let frac = ((mouse_x - track_start) / track_w).clamp(0.0, 1.0);
        config.annotation.stroke_opacity = 0.05 + frac * 0.95;
        config.annotation.save_tool_settings(active_tool.as_str());
    }

    fn update_size_slider(&mut self, mouse_x: f32, config: &mut AppConfig, active_tool: ActiveTool) {
        let track_start = self.x + 6.0;
        let track_w = self.width - 12.0;
        let frac = ((mouse_x - track_start) / track_w).clamp(0.0, 1.0);
        let min_w = config.annotation.min_stroke_width.max(0.5);
        let max_w = config.annotation.max_stroke_width.max(min_w + 1.0);
        config.annotation.stroke_width = min_w + frac * (max_w - min_w);
        config.annotation.save_tool_settings(active_tool.as_str());
    }

    fn update_softness_slider(&mut self, mouse_x: f32, config: &mut AppConfig, active_tool: ActiveTool) {
        let track_start = self.x + 6.0;
        let track_w = self.width - 12.0;
        let frac = ((mouse_x - track_start) / track_w).clamp(0.0, 1.0);
        config.annotation.stroke_softness = frac;
        config.annotation.save_tool_settings(active_tool.as_str());
    }

    /// Render the floating control panel using flat, borderless Direct2D shapes
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
        annotations_hidden: bool,
    ) -> Result<()> {
        let x = self.x;
        let y = self.y;
        let w = self.width;
        let h = self.height;
        let panel_op = config.annotation.toolbar_opacity.clamp(0.3, 1.0);

        // Panel Background Card (flat dark acrylic)
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

        // Top Bar Handle
        let handle_pill = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x + w * 0.5 - 18.0, top: y + 5.0, right: x + w * 0.5 + 18.0, bottom: y + 9.0 },
            radiusX: 2.0,
            radiusY: 2.0,
        };
        let handle_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.50, g: 0.55, b: 0.68, a: panel_op * 0.85 },
            None,
        )?;
        rt.FillRoundedRectangle(&handle_pill, &handle_brush);

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
        let locked = annotations_hidden;
        let arrow_alpha = if locked { 0.30 } else { 0.92 };
        let arrow_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.96, a: arrow_alpha }, None)?;
        let arrow_wide: Vec<u16> = "▾".encode_utf16().collect();

        // Borderless flat button renderer (no outlines, no gradients)
        let draw_btn_bg = |left: f32, top: f32, right: f32, bottom: f32, is_active: bool, is_alert: bool, is_disabled: bool| -> Result<()> {
            let r = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left, top, right, bottom },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let bg_color = if is_disabled {
                D2D1_COLOR_F { r: 0.12, g: 0.13, b: 0.17, a: panel_op * 0.45 }
            } else if is_alert {
                D2D1_COLOR_F { r: 0.72, g: 0.18, b: 0.22, a: panel_op }
            } else if is_active {
                D2D1_COLOR_F { r: 0.20, g: 0.36, b: 0.75, a: panel_op }
            } else {
                D2D1_COLOR_F { r: 0.15, g: 0.17, b: 0.23, a: panel_op * 0.92 }
            };
            let b_brush = rt.CreateSolidColorBrush(&bg_color, None)?;
            rt.FillRoundedRectangle(&r, &b_brush);
            Ok(())
        };

        let draw_icon_btn = |rel_x: f32, rel_y: f32, icon: FeatherIcon, is_active: bool, is_alert: bool, is_disabled: bool| -> Result<()> {
            let left = x + rel_x;
            let top = y + rel_y;
            let right = left + BTN_SIZE;
            let bottom = top + BTN_SIZE;
            draw_btn_bg(left, top, right, bottom, is_active, is_alert, is_disabled)?;

            let icon_alpha = if is_disabled { 0.30 } else { 1.0 };
            let icon_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: icon_alpha }, None)?;
            draw_feather_icon(rt, &factory, left + BTN_SIZE * 0.5, top + BTN_SIZE * 0.5, icon, &icon_brush, round_stroke_style.as_ref())
        };

        // Row 1: Pen/Text, Burn/Dodge, Eraser/Transform (all remember toggle state independently)
        let pen_icon = if active_tool == ActiveTool::Text
            || (active_tool != ActiveTool::Freehand && config.annotation.last_pen_text == "text")
        {
            FeatherIcon::Type
        } else {
            FeatherIcon::Pen
        };
        let burn_dodge_icon = if active_tool == ActiveTool::Dodge
            || (active_tool != ActiveTool::Burn && config.annotation.last_burn_dodge == "dodge")
        {
            FeatherIcon::Dodge
        } else {
            FeatherIcon::Burn
        };
        let eraser_icon = if active_tool == ActiveTool::Transform
            || (active_tool != ActiveTool::Eraser && config.annotation.last_eraser_transform == "transform")
        {
            FeatherIcon::Move
        } else {
            FeatherIcon::Eraser
        };
        draw_icon_btn(
            COL1_X,
            ROW1_Y,
            pen_icon,
            !locked && (active_tool == ActiveTool::Freehand || active_tool == ActiveTool::Text),
            false,
            locked,
        )?;
        draw_icon_btn(
            COL2_X,
            ROW1_Y,
            burn_dodge_icon,
            !locked && (active_tool == ActiveTool::Burn || active_tool == ActiveTool::Dodge),
            false,
            locked,
        )?;
        draw_icon_btn(
            COL3_X,
            ROW1_Y,
            eraser_icon,
            !locked && (active_tool == ActiveTool::Eraser || active_tool == ActiveTool::Transform),
            false,
            locked,
        )?;

        // Row 2: Shapes, Emoji, Color/Invert (all with overlapping corner flyout cues)
        let shapes_active = !locked && (active_tool.is_shape() || self.shapes_flyout_open);
        let shape_icon = Self::current_shape_icon(config, active_tool);
        draw_icon_btn(COL1_X, ROW2_Y, shape_icon, shapes_active, false, locked)?;
        let s_left = x + COL1_X;
        let s_top = y + ROW2_Y;
        rt.DrawText(
            &arrow_wide,
            text_format_small,
            &D2D_RECT_F {
                left: s_left + BTN_SIZE - 11.0,
                top: s_top + BTN_SIZE - 13.0,
                right: s_left + BTN_SIZE - 1.0,
                bottom: s_top + BTN_SIZE,
            },
            &arrow_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let e_left = x + COL2_X;
        let e_top = y + ROW2_Y;
        draw_btn_bg(
            e_left,
            e_top,
            e_left + BTN_SIZE,
            e_top + BTN_SIZE,
            !locked && (active_tool == ActiveTool::Emoji || self.emoji_flyout_open),
            false,
            locked,
        )?;
        let e_wide: Vec<u16> = config.annotation.selected_emoji.encode_utf16().collect();
        let white_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
        let e_rect = D2D_RECT_F {
            left: e_left + 7.0,
            top: e_top + 6.0,
            right: e_left + BTN_SIZE - 3.0,
            bottom: e_top + BTN_SIZE - 3.0,
        };
        if locked {
            let layer = rt.CreateLayer(None).ok();
            if let Some(ref l) = layer {
                let lp = D2D1_LAYER_PARAMETERS {
                    contentBounds: e_rect,
                    geometricMask: ManuallyDrop::new(None),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: windows::Foundation::Numerics::Matrix3x2 {
                        M11: 1.0,
                        M12: 0.0,
                        M21: 0.0,
                        M22: 1.0,
                        M31: 0.0,
                        M32: 0.0,
                    },
                    opacity: 0.30,
                    opacityBrush: ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS_NONE,
                };
                rt.PushLayer(&lp, l);
            }
            rt.DrawText(
                &e_wide,
                text_format_ui,
                &e_rect,
                &white_brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            if layer.is_some() {
                rt.PopLayer();
            }
        } else {
            rt.DrawText(
                &e_wide,
                text_format_ui,
                &e_rect,
                &white_brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        rt.DrawText(
            &arrow_wide,
            text_format_small,
            &D2D_RECT_F {
                left: e_left + BTN_SIZE - 11.0,
                top: e_top + BTN_SIZE - 13.0,
                right: e_left + BTN_SIZE - 1.0,
                bottom: e_top + BTN_SIZE,
            },
            &arrow_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let c_left = x + COL3_X;
        let c_top = y + ROW2_Y;
        draw_btn_bg(
            c_left,
            c_top,
            c_left + BTN_SIZE,
            c_top + BTN_SIZE,
            !locked && self.color_flyout_open,
            false,
            locked,
        )?;
        let dot_cx = c_left + BTN_SIZE * 0.5;
        let dot_cy = c_top + BTN_SIZE * 0.5;
        let swatch_alpha = if locked { 0.30 } else { 1.0 };
        if config.annotation.invert_color {
            draw_invert_symbol(rt, &factory, dot_cx, dot_cy, 8.0, swatch_alpha)?;
        } else {
            let cur_c = config.annotation.current_color;
            let dot_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: cur_c[0], g: cur_c[1], b: cur_c[2], a: swatch_alpha }, None)?;
            let dot_ellipse = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: dot_cx, y: dot_cy },
                radiusX: 7.5,
                radiusY: 7.5,
            };
            rt.FillEllipse(&dot_ellipse, &dot_brush);
        }
        rt.DrawText(
            &arrow_wide,
            text_format_small,
            &D2D_RECT_F {
                left: c_left + BTN_SIZE - 11.0,
                top: c_top + BTN_SIZE - 13.0,
                right: c_left + BTN_SIZE - 1.0,
                bottom: c_top + BTN_SIZE,
            },
            &arrow_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        // Shared dot-free rounded pill slider helper
        let draw_pill_slider = |row_y: f32, label: &str, frac: f32, disabled: bool| -> Result<()> {
            let sy = y + row_y;
            let label_alpha = if disabled { 0.35 } else { 1.0 };
            let label_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: label_alpha }, None)?;
            let label_wide: Vec<u16> = label.encode_utf16().collect();
            rt.DrawText(
                &label_wide,
                text_format_small,
                &D2D_RECT_F { left: x + 6.0, top: sy, right: x + w - 6.0, bottom: sy + 13.0 },
                &label_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let track_y = sy + 15.0;
            let track_h = 6.0;
            let track_w = w - 12.0;
            let radius = 3.0;
            let track_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.28, a: panel_op }, None)?;
            let track_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: x + 6.0, top: track_y, right: x + 6.0 + track_w, bottom: track_y + track_h },
                radiusX: radius,
                radiusY: radius,
            };
            rt.FillRoundedRectangle(&track_rect, &track_bg);

            let clamped = frac.clamp(0.0, 1.0);
            if clamped > 0.005 {
                let fill_w = (track_w * clamped).max(track_h);
                let fill_col = if disabled {
                    D2D1_COLOR_F { r: 0.32, g: 0.38, b: 0.50, a: 0.45 }
                } else {
                    D2D1_COLOR_F { r: 0.30, g: 0.55, b: 0.98, a: 1.0 }
                };
                let fill_bg = rt.CreateSolidColorBrush(&fill_col, None)?;
                let fill_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x + 6.0, top: track_y, right: x + 6.0 + fill_w, bottom: track_y + track_h },
                    radiusX: radius,
                    radiusY: radius,
                };
                rt.FillRoundedRectangle(&fill_rect, &fill_bg);
            }
            Ok(())
        };

        // Sliders (1st: Softness, 2nd: Opacity, 3rd: Size)
        let soft_enabled = !locked && active_tool.supports_softness();
        let soft_frac = if active_tool.supports_softness() {
            config.annotation.stroke_softness.clamp(0.0, 1.0)
        } else {
            0.0
        };
        draw_pill_slider(
            SLIDER_SOFTNESS_Y,
            &format!("🌫 Soft {:.0}%", soft_frac * 100.0),
            soft_frac,
            !soft_enabled,
        )?;

        let op_frac = ((config.annotation.stroke_opacity - 0.05) / 0.95).clamp(0.0, 1.0);
        draw_pill_slider(SLIDER_OPACITY_Y, &format!("💧 Opac {:.0}%", op_frac * 100.0), op_frac, locked)?;

        let min_w = config.annotation.min_stroke_width.max(0.5);
        let max_w = config.annotation.max_stroke_width.max(min_w + 1.0);
        let size_frac = ((config.annotation.stroke_width - min_w) / (max_w - min_w)).clamp(0.0, 1.0);
        draw_pill_slider(SLIDER_SIZE_Y, &format!("📏 Size {:.0}%", size_frac * 100.0), size_frac, locked)?;

        // Row 4: Undo, Redo, Clear
        draw_icon_btn(COL1_X, ROW4_Y, FeatherIcon::Undo, false, false, locked || !can_undo)?;
        draw_icon_btn(COL2_X, ROW4_Y, FeatherIcon::Redo, false, false, locked || !can_redo)?;
        let clear_icon = if self.confirm_clear { FeatherIcon::ConfirmQuestion } else { FeatherIcon::Trash };
        draw_icon_btn(COL3_X, ROW4_Y, clear_icon, false, !locked && self.confirm_clear, locked)?;

        // Row 5: Hide/Show Annotations (Eye), Settings (Gear), Exit (Close)
        let eye_icon = if annotations_hidden { FeatherIcon::EyeOff } else { FeatherIcon::Eye };
        let exit_icon = if self.confirm_exit { FeatherIcon::ConfirmQuestion } else { FeatherIcon::Close };
        draw_icon_btn(COL1_X, ROW5_Y, eye_icon, annotations_hidden, false, false)?;
        draw_icon_btn(COL2_X, ROW5_Y, FeatherIcon::Settings, false, false, false)?;
        draw_icon_btn(COL3_X, ROW5_Y, exit_icon, false, self.confirm_exit, false)?;

        // Render active flyout card
        if self.has_open_flyout() {
            let flyout_rect = self.get_flyout_rect();
            let f_rect = D2D1_ROUNDED_RECT {
                rect: flyout_rect,
                radiusX: 10.0,
                radiusY: 10.0,
            };
            let f_bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.98 }, None)?;
            rt.FillRoundedRectangle(&f_rect, &f_bg);

            let start_x = flyout_rect.left + FLYOUT_PAD;
            let start_y = flyout_rect.top + FLYOUT_PAD;

            if self.shapes_flyout_open {
                let item_bg_norm = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.92 }, None)?;
                let item_bg_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 }, None)?;
                for (idx, item) in SHAPE_FLYOUT_ITEMS.iter().enumerate() {
                    let col = idx % 3;
                    let row = idx / 3;
                    let ix = start_x + col as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let iy = start_y + row as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let item_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: ix, top: iy, right: ix + FLYOUT_CELL, bottom: iy + FLYOUT_CELL },
                        radiusX: 6.0,
                        radiusY: 6.0,
                    };
                    let is_sel = Self::is_shape_item_selected(*item, config, active_tool);
                    rt.FillRoundedRectangle(&item_rect, if is_sel { &item_bg_sel } else { &item_bg_norm });
                    draw_feather_icon(
                        rt,
                        &factory,
                        ix + FLYOUT_CELL * 0.5,
                        iy + FLYOUT_CELL * 0.5,
                        item.icon,
                        &white_brush,
                        round_stroke_style.as_ref(),
                    )?;
                }
            } else if self.emoji_flyout_open {
                let item_bg_norm = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.92 }, None)?;
                let item_bg_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 }, None)?;
                for (idx, emoji_char) in config.annotation.custom_emojis.iter().enumerate().take(9) {
                    let col = idx % 3;
                    let row = idx / 3;
                    let ex = start_x + col as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let ey = start_y + row as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let is_sel = config.annotation.selected_emoji == *emoji_char;
                    let item_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: ex, top: ey, right: ex + FLYOUT_CELL, bottom: ey + FLYOUT_CELL },
                        radiusX: 6.0,
                        radiusY: 6.0,
                    };
                    rt.FillRoundedRectangle(&item_rect, if is_sel { &item_bg_sel } else { &item_bg_norm });

                    let wide_e: Vec<u16> = emoji_char.encode_utf16().collect();
                    let e_box = D2D_RECT_F { left: ex + 3.0, top: ey + 3.0, right: ex + FLYOUT_CELL, bottom: ey + FLYOUT_CELL };
                    rt.DrawText(
                        &wide_e,
                        text_format_emoji,
                        &e_box,
                        &white_brush,
                        D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            } else if self.color_flyout_open {
                let tile_bg_norm = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.16, g: 0.18, b: 0.25, a: 0.95 }, None)?;
                let tile_bg_sel = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.38, b: 0.75, a: 1.0 }, None)?;
                for grid_idx in 0..9 {
                    let col_idx = grid_idx % 3;
                    let row_idx = grid_idx / 3;
                    let cx = start_x + col_idx as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let cy = start_y + row_idx as f32 * (FLYOUT_CELL + FLYOUT_GAP);
                    let item_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: cx, top: cy, right: cx + FLYOUT_CELL, bottom: cy + FLYOUT_CELL },
                        radiusX: 6.0,
                        radiusY: 6.0,
                    };

                    if let Some(preset_idx) = Self::color_grid_slot(grid_idx) {
                        if let Some(&col) = config.annotation.color_presets.get(preset_idx) {
                            let is_sel = !config.annotation.invert_color
                                && (col[0] - config.annotation.current_color[0]).abs() < 0.03
                                && (col[1] - config.annotation.current_color[1]).abs() < 0.03
                                && (col[2] - config.annotation.current_color[2]).abs() < 0.03;

                            let col_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: col[0], g: col[1], b: col[2], a: 1.0 }, None)?;
                            rt.FillRoundedRectangle(&item_rect, &col_brush);

                            if is_sel {
                                let lum = 0.299 * col[0] + 0.587 * col[1] + 0.114 * col[2];
                                let indicator = if lum > 0.65 {
                                    D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.95 }
                                } else {
                                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }
                                };
                                let ind_brush = rt.CreateSolidColorBrush(&indicator, None)?;
                                let center_dot = D2D1_ELLIPSE {
                                    point: D2D_POINT_2F { x: cx + FLYOUT_CELL * 0.5, y: cy + FLYOUT_CELL * 0.5 },
                                    radiusX: 4.5,
                                    radiusY: 4.5,
                                };
                                rt.FillEllipse(&center_dot, &ind_brush);
                            }
                        }
                    } else {
                        // Fixed center slot: Invert
                        rt.FillRoundedRectangle(&item_rect, if config.annotation.invert_color { &tile_bg_sel } else { &tile_bg_norm });
                        draw_invert_symbol(
                            rt,
                            &factory,
                            cx + FLYOUT_CELL * 0.5,
                            cy + FLYOUT_CELL * 0.5,
                            10.0,
                            1.0,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}

/// Flat split diagonal black/white circle for Invert (zero gradients)
unsafe fn draw_invert_symbol(
    rt: &ID2D1RenderTarget,
    factory: &ID2D1Factory,
    cx: f32,
    cy: f32,
    radius: f32,
    alpha: f32,
) -> Result<()> {
    let base_ellipse = D2D1_ELLIPSE {
        point: D2D_POINT_2F { x: cx, y: cy },
        radiusX: radius,
        radiusY: radius,
    };
    let dark_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.11, a: alpha }, None)?;
    rt.FillEllipse(&base_ellipse, &dark_brush);

    let diag = radius * std::f32::consts::FRAC_1_SQRT_2;
    let p_start = D2D_POINT_2F { x: cx - diag, y: cy + diag };
    let p_end = D2D_POINT_2F { x: cx + diag, y: cy - diag };

    if let Ok(path) = factory.CreatePathGeometry() {
        if let Ok(sink) = path.Open() {
            sink.BeginFigure(p_start, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: p_end,
                size: D2D_SIZE_F { width: radius, height: radius },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            let light_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.98, g: 0.98, b: 1.0, a: alpha }, None)?;
            rt.FillGeometry(&path, &light_brush, None);
        }
    }

    let rim_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.78, g: 0.82, b: 0.90, a: 0.85 * alpha }, None)?;
    rt.DrawEllipse(&base_ellipse, &rim_brush, 1.0, None);
    Ok(())
}

/// Helper to render vector Feather Icons with flat contrast
unsafe fn draw_feather_icon(
    rt: &ID2D1RenderTarget,
    factory: &ID2D1Factory,
    cx: f32,
    cy: f32,
    icon: FeatherIcon,
    brush: &ID2D1SolidColorBrush,
    stroke_style: Option<&ID2D1StrokeStyle>,
) -> Result<()> {
    match icon {
        FeatherIcon::Pen => {
            rt.DrawLine(D2D_POINT_2F { x: cx - 4.5, y: cy + 4.5 }, D2D_POINT_2F { x: cx + 4.5, y: cy - 4.5 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 4.5, y: cy + 4.5 }, D2D_POINT_2F { x: cx - 6.5, y: cy + 6.5 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 4.5, y: cy - 4.5 }, D2D_POINT_2F { x: cx + 6.5, y: cy - 6.5 }, brush, 2.0, stroke_style);
        }
        FeatherIcon::Type => {
            rt.DrawLine(D2D_POINT_2F { x: cx - 6.5, y: cy - 6.0 }, D2D_POINT_2F { x: cx + 6.5, y: cy - 6.0 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 6.5, y: cy - 6.0 }, D2D_POINT_2F { x: cx - 6.5, y: cy - 3.5 }, brush, 1.6, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 6.5, y: cy - 6.0 }, D2D_POINT_2F { x: cx + 6.5, y: cy - 3.5 }, brush, 1.6, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx, y: cy - 6.0 }, D2D_POINT_2F { x: cx, y: cy + 6.5 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 3.0, y: cy + 6.5 }, D2D_POINT_2F { x: cx + 3.0, y: cy + 6.5 }, brush, 1.6, stroke_style);
        }
        FeatherIcon::Eraser => {
            let er = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cx - 7.0, top: cy - 4.5, right: cx + 7.0, bottom: cy + 4.5 },
                radiusX: 2.0,
                radiusY: 2.0,
            };
            rt.DrawRoundedRectangle(&er, brush, 1.8, None);
            rt.DrawLine(D2D_POINT_2F { x: cx - 1.5, y: cy - 4.5 }, D2D_POINT_2F { x: cx - 1.5, y: cy + 4.5 }, brush, 1.8, None);
        }
        FeatherIcon::Move => {
            rt.DrawLine(D2D_POINT_2F { x: cx, y: cy - 6.5 }, D2D_POINT_2F { x: cx, y: cy + 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 6.5, y: cy }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.8, y: cy - 3.7 }, D2D_POINT_2F { x: cx, y: cy - 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 2.8, y: cy - 3.7 }, D2D_POINT_2F { x: cx, y: cy - 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 2.8, y: cy + 3.7 }, D2D_POINT_2F { x: cx, y: cy + 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 2.8, y: cy + 3.7 }, D2D_POINT_2F { x: cx, y: cy + 6.5 }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 3.7, y: cy - 2.8 }, D2D_POINT_2F { x: cx - 6.5, y: cy }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx - 3.7, y: cy + 2.8 }, D2D_POINT_2F { x: cx - 6.5, y: cy }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 3.7, y: cy - 2.8 }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 3.7, y: cy + 2.8 }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
        }
        FeatherIcon::Burn => {
            if let Ok(path) = factory.CreatePathGeometry() {
                if let Ok(sink) = path.Open() {
                    sink.BeginFigure(D2D_POINT_2F { x: cx, y: cy - 7.5 }, D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: cx + 6.5, y: cy - 2.5 },
                        point2: D2D_POINT_2F { x: cx + 6.5, y: cy + 6.5 },
                        point3: D2D_POINT_2F { x: cx, y: cy + 7.2 },
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: cx - 6.5, y: cy + 6.5 },
                        point2: D2D_POINT_2F { x: cx - 6.5, y: cy - 2.5 },
                        point3: D2D_POINT_2F { x: cx, y: cy - 7.5 },
                    });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();
                    rt.DrawGeometry(&path, brush, 1.8, stroke_style);
                }
            }
            if let Ok(inner) = factory.CreatePathGeometry() {
                if let Ok(sink) = inner.Open() {
                    sink.BeginFigure(D2D_POINT_2F { x: cx, y: cy + 0.5 }, D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: cx + 2.6, y: cy + 2.5 },
                        point2: D2D_POINT_2F { x: cx + 2.2, y: cy + 5.8 },
                        point3: D2D_POINT_2F { x: cx, y: cy + 6.0 },
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: cx - 2.2, y: cy + 5.8 },
                        point2: D2D_POINT_2F { x: cx - 2.6, y: cy + 2.5 },
                        point3: D2D_POINT_2F { x: cx, y: cy + 0.5 },
                    });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();
                    rt.FillGeometry(&inner, brush, None);
                }
            }
        }
        FeatherIcon::Dodge => {
            let core = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: 3.4, radiusY: 3.4 };
            rt.DrawEllipse(&core, brush, 1.8, None);
            for i in 0..8 {
                let ang = (i as f32) * std::f32::consts::FRAC_PI_4;
                let (s, c) = ang.sin_cos();
                rt.DrawLine(
                    D2D_POINT_2F { x: cx + c * 5.2, y: cy + s * 5.2 },
                    D2D_POINT_2F { x: cx + c * 7.2, y: cy + s * 7.2 },
                    brush,
                    1.7,
                    stroke_style,
                );
            }
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
                rt.DrawLine(D2D_POINT_2F { x: cx - 6.0, y: cy + 5.5 }, D2D_POINT_2F { x: cx - 2.0, y: cy + 1.0 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx - 2.0, y: cy + 1.0 }, D2D_POINT_2F { x: cx + 2.0, y: cy + 4.0 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx + 2.0, y: cy + 4.0 }, D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, brush, 1.8, stroke_style);
            } else {
                rt.DrawLine(D2D_POINT_2F { x: cx - 5.5, y: cy + 5.5 }, D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, brush, 2.0, stroke_style);
            }
            rt.DrawLine(D2D_POINT_2F { x: cx - 0.5, y: cy - 5.5 }, D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 5.5, y: cy - 5.5 }, D2D_POINT_2F { x: cx + 5.5, y: cy + 0.5 }, brush, 2.0, stroke_style);
        }
        FeatherIcon::Line { dotted, wavy } => {
            if wavy {
                rt.DrawLine(D2D_POINT_2F { x: cx - 6.5, y: cy }, D2D_POINT_2F { x: cx - 3.5, y: cy - 3.5 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx - 3.5, y: cy - 3.5 }, D2D_POINT_2F { x: cx, y: cy }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx, y: cy }, D2D_POINT_2F { x: cx + 3.5, y: cy + 3.5 }, brush, 1.8, stroke_style);
                rt.DrawLine(D2D_POINT_2F { x: cx + 3.5, y: cy + 3.5 }, D2D_POINT_2F { x: cx + 6.5, y: cy }, brush, 1.8, stroke_style);
            } else if dotted {
                for i in 0..4 {
                    let t = i as f32 / 3.0;
                    let px = (cx - 6.0) * (1.0 - t) + (cx + 6.0) * t;
                    let py = (cy + 5.0) * (1.0 - t) + (cy - 5.0) * t;
                    let dot = D2D1_ELLIPSE { point: D2D_POINT_2F { x: px, y: py }, radiusX: 1.2, radiusY: 1.2 };
                    rt.FillEllipse(&dot, brush);
                }
            } else {
                rt.DrawLine(D2D_POINT_2F { x: cx - 7.0, y: cy + 5.0 }, D2D_POINT_2F { x: cx + 7.0, y: cy - 5.0 }, brush, 2.2, stroke_style);
            }
        }
        FeatherIcon::Undo => {
            // 75% (270-degree) circular counter-clockwise arrow
            let r = 6.0;
            let p_tip = D2D_POINT_2F { x: cx - 5.2, y: cy - 2.6 };
            let p_tail = D2D_POINT_2F { x: cx - 3.2, y: cy + 5.2 };
            if let Ok(arc_geo) = factory.CreatePathGeometry() {
                if let Ok(sink) = arc_geo.Open() {
                    sink.BeginFigure(p_tip, D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: p_tail,
                        size: D2D_SIZE_F { width: r, height: r },
                        rotationAngle: 0.0,
                        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                        arcSize: D2D1_ARC_SIZE_LARGE,
                    });
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    let _ = sink.Close();
                    rt.DrawGeometry(&arc_geo, brush, 1.9, stroke_style);
                }
            }
            // Arrowhead wings at tip pointing counter-clockwise
            rt.DrawLine(p_tip, D2D_POINT_2F { x: p_tip.x, y: p_tip.y - 3.8 }, brush, 1.9, stroke_style);
            rt.DrawLine(p_tip, D2D_POINT_2F { x: p_tip.x + 3.8, y: p_tip.y }, brush, 1.9, stroke_style);
        }
        FeatherIcon::Redo => {
            // 75% (270-degree) circular clockwise arrow
            let r = 6.0;
            let p_tip = D2D_POINT_2F { x: cx + 5.2, y: cy - 2.6 };
            let p_tail = D2D_POINT_2F { x: cx + 3.2, y: cy + 5.2 };
            if let Ok(arc_geo) = factory.CreatePathGeometry() {
                if let Ok(sink) = arc_geo.Open() {
                    sink.BeginFigure(p_tip, D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: p_tail,
                        size: D2D_SIZE_F { width: r, height: r },
                        rotationAngle: 0.0,
                        sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                        arcSize: D2D1_ARC_SIZE_LARGE,
                    });
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    let _ = sink.Close();
                    rt.DrawGeometry(&arc_geo, brush, 1.9, stroke_style);
                }
            }
            // Arrowhead wings at tip pointing clockwise
            rt.DrawLine(p_tip, D2D_POINT_2F { x: p_tip.x, y: p_tip.y - 3.8 }, brush, 1.9, stroke_style);
            rt.DrawLine(p_tip, D2D_POINT_2F { x: p_tip.x - 3.8, y: p_tip.y }, brush, 1.9, stroke_style);
        }
        FeatherIcon::Trash => {
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
            rt.DrawLine(D2D_POINT_2F { x: cx - 5.0, y: cy - 5.0 }, D2D_POINT_2F { x: cx + 5.0, y: cy + 5.0 }, brush, 2.0, stroke_style);
            rt.DrawLine(D2D_POINT_2F { x: cx + 5.0, y: cy - 5.0 }, D2D_POINT_2F { x: cx - 5.0, y: cy + 5.0 }, brush, 2.0, stroke_style);
        }
        FeatherIcon::Eye | FeatherIcon::EyeOff => {
            if let Ok(eye_geo) = factory.CreatePathGeometry() {
                if let Ok(sink) = eye_geo.Open() {
                    sink.BeginFigure(D2D_POINT_2F { x: cx - 7.5, y: cy }, D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: cx - 3.5, y: cy - 5.5 },
                        point2: D2D_POINT_2F { x: cx + 3.5, y: cy - 5.5 },
                        point3: D2D_POINT_2F { x: cx + 7.5, y: cy },
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: cx + 3.5, y: cy + 5.5 },
                        point2: D2D_POINT_2F { x: cx - 3.5, y: cy + 5.5 },
                        point3: D2D_POINT_2F { x: cx - 7.5, y: cy },
                    });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();
                    rt.DrawGeometry(&eye_geo, brush, 1.7, stroke_style);
                }
            }
            let pupil = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: 2.3, radiusY: 2.3 };
            rt.DrawEllipse(&pupil, brush, 1.6, None);
            if matches!(icon, FeatherIcon::EyeOff) {
                rt.DrawLine(
                    D2D_POINT_2F { x: cx - 6.5, y: cy + 5.5 },
                    D2D_POINT_2F { x: cx + 6.5, y: cy - 5.5 },
                    brush,
                    1.9,
                    stroke_style,
                );
            }
        }
        FeatherIcon::Settings => {
            let core = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: 2.6, radiusY: 2.6 };
            rt.DrawEllipse(&core, brush, 1.7, None);
            let outer = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: 5.4, radiusY: 5.4 };
            rt.DrawEllipse(&outer, brush, 1.4, None);
            for i in 0..6 {
                let ang = (i as f32) * (std::f32::consts::PI / 3.0);
                let (s, c) = ang.sin_cos();
                rt.DrawLine(
                    D2D_POINT_2F { x: cx + c * 5.2, y: cy + s * 5.2 },
                    D2D_POINT_2F { x: cx + c * 7.4, y: cy + s * 7.4 },
                    brush,
                    1.8,
                    stroke_style,
                );
            }
        }
        FeatherIcon::ConfirmQuestion => {
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

        // Inside 3-per-row panel bounds (width: 128, height: 270)
        assert!(tb.hit_test(Point2D::new(200.0, 250.0)));
        assert!(!tb.hit_test(Point2D::new(50.0, 50.0)));

        let row1_cy = 100.0 + ROW1_Y + 18.0;
        let row2_cy = 100.0 + ROW2_Y + 18.0;
        let row4_cy = 100.0 + ROW4_Y + 18.0;
        let row5_cy = 100.0 + ROW5_Y + 18.0;
        let col1_cx = 100.0 + COL1_X + 18.0;
        let col2_cx = 100.0 + COL2_X + 18.0;
        let col3_cx = 100.0 + COL3_X + 18.0;

        // Row 1: Pen/Text, Burn/Dodge, Eraser/Transform preserve toggle state across tool changes
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Eraser);
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Transform);

        // Switch to Pen -> then switch back to Eraser/Transform -> must stay Transform!
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Freehand);
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Transform);

        // Switch back to Pen/Text (still Freehand, not toggled to Text yet!)
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Freehand);
        // Click again while already on Pen -> toggles to Text
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Text);

        // Softness slider (1st place) on Freehand (50%)
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Freehand);
        let soft_y = 100.0 + SLIDER_SOFTNESS_Y + 15.0;
        assert!(tb.handle_lbutton_down(Point2D::new(100.0 + 6.0 + 58.0, soft_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tb.active_slider, Some(DraggingSlider::Softness));
        assert!((cfg.annotation.stroke_softness - 0.5).abs() < 0.02);
        tb.handle_lbutton_up(&mut cfg);

        // Row 2 Col 1: Shapes button opens flyout immediately on 1st click
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row2_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Rect);
        assert!(tb.shapes_flyout_open);

        // Clicking softness slider while on Rect must NOT activate softness slider
        assert!(tb.handle_lbutton_down(Point2D::new(100.0 + 6.0 + 58.0, soft_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tb.active_slider, None);
        assert!(!tb.shapes_flyout_open);

        // Row 1 Col 2: Burn/Dodge preserves independent settings
        assert!(tb.handle_lbutton_down(Point2D::new(col2_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Burn);
        assert!((cfg.annotation.stroke_width - 24.0).abs() < 0.01);
        assert!(tb.handle_lbutton_down(Point2D::new(col2_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Dodge);

        // Switch back to Pen/Text -> still Freehand and restores Freehand's saved width (6.0) and softness (0.5)
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert_eq!(tool, ActiveTool::Freehand);
        assert!((cfg.annotation.stroke_width - 6.0).abs() < 0.01);
        assert!((cfg.annotation.stroke_softness - 0.5).abs() < 0.02);

        // Row 2 Col 3: Color flyout + fixed center Invert (grid col=1, row=1)
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row2_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(tb.color_flyout_open);
        let cf_rect = tb.get_flyout_rect();
        let center_x = cf_rect.left + FLYOUT_PAD + 1.0 * (FLYOUT_CELL + FLYOUT_GAP) + 12.0;
        let center_y = cf_rect.top + FLYOUT_PAD + 1.0 * (FLYOUT_CELL + FLYOUT_GAP) + 12.0;
        assert!(tb.handle_lbutton_down(Point2D::new(center_x, center_y), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(cfg.annotation.invert_color);
        assert!(!tb.color_flyout_open);

        // Row 4 Col 3: Clear confirm, Row 5: Lock/Unlock Visibility (Eye), Settings Gear, Exit
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row4_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(tb.confirm_clear);

        // Lock (hide annotations) -> closes open flyouts, clears confirmations, disables drawing buttons
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row2_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(tb.shapes_flyout_open);
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row5_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(mgr.annotations_hidden);
        assert!(!tb.confirm_clear);
        assert!(!tb.shapes_flyout_open);

        // While locked: clicking Pen/Text, Shapes, Color, Sliders, or Clear does NOT unlock or mutate state
        let tool_before_lock_clicks = tool;
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row1_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(mgr.annotations_hidden);
        assert_eq!(tool, tool_before_lock_clicks);
        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row2_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(mgr.annotations_hidden);
        assert!(!tb.shapes_flyout_open);
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row2_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(!tb.color_flyout_open);
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row4_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(!tb.confirm_clear);

        // While locked: Settings and top handle drag still work; Eye unlocks
        assert!(tb.handle_lbutton_down(Point2D::new(col2_cx, row5_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(tb.pending_open_settings);
        tb.pending_open_settings = false;

        assert!(tb.handle_lbutton_down(Point2D::new(col1_cx, row5_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(!mgr.annotations_hidden);

        assert!(tb.handle_lbutton_down(Point2D::new(col2_cx, row5_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(tb.pending_open_settings);

        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row5_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(tb.confirm_exit);
        assert!(tb.handle_lbutton_down(Point2D::new(col3_cx, row5_cy), &mut cfg, &mut tool, &mut mgr, &mut needs_exit));
        assert!(needs_exit);
    }
}
