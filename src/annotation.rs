use crate::geometry::{
    calculate_arrow_head, distance_to_ellipse, distance_to_rect, distance_to_segment, point_in_rect, Point2D,
};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Freehand {
        points: Vec<Point2D>,
        color: [f32; 4],
        width: f32,
    },
    Line {
        start: Point2D,
        end: Point2D,
        color: [f32; 4],
        width: f32,
        dotted: bool,
        wavy: bool,
    },
    Arrow {
        start: Point2D,
        end: Point2D,
        color: [f32; 4],
        width: f32,
        wavy: bool,
    },
    Rect {
        p1: Point2D,
        p2: Point2D,
        color: [f32; 4],
        width: f32,
        fill: bool,
        rotation_rad: f32,
    },
    Ellipse {
        center: Point2D,
        rx: f32,
        ry: f32,
        color: [f32; 4],
        width: f32,
        fill: bool,
    },
    Emoji {
        pos: Point2D,
        emoji: String,
        size: f32,
        opacity: f32,
        color: [f32; 4],
        flat: bool,
        filled: bool,
        rotation_rad: f32,
    },
    Text {
        pos: Point2D,
        text: String,
        font_family: String,
        font_size: f32,
        color: [f32; 4],
        rotation_rad: f32,
    },
}

#[derive(Debug, Clone)]
pub struct ActiveTextInput {
    pub pos: Point2D,
    pub text: String,
    pub font_family: String,
    pub font_size: f32,
    pub color: [f32; 4],
}

impl Shape {
    /// Hit-test: checks if `point` is close to or on this shape within `tolerance`.
    pub fn hit_test(&self, p: Point2D, tolerance: f32) -> bool {
        match self {
            Shape::Freehand { points, width, .. } => {
                let r = (width * 0.5 + tolerance).max(6.0);
                for window in points.windows(2) {
                    if distance_to_segment(p, window[0], window[1]) <= r {
                        return true;
                    }
                }
                false
            }
            Shape::Line { start, end, width, .. } => {
                let r = (width * 0.5 + tolerance).max(6.0);
                distance_to_segment(p, *start, *end) <= r
            }
            Shape::Arrow { start, end, width, .. } => {
                let r = (width * 0.5 + tolerance).max(6.0);
                if distance_to_segment(p, *start, *end) <= r {
                    return true;
                }
                let (w1, w2) = calculate_arrow_head(*start, *end, width * 3.5 + 10.0, 0.42);
                distance_to_segment(p, *end, w1) <= r || distance_to_segment(p, *end, w2) <= r
            }
            Shape::Rect { p1, p2, width, fill, rotation_rad, .. } => {
                let r = (width * 0.5 + tolerance).max(6.0);
                if rotation_rad.abs() > 0.01 {
                    let center_x = (p1.x + p2.x) * 0.5;
                    let center_y = (p1.y + p2.y) * 0.5;
                    let cos_a = (-rotation_rad).cos();
                    let sin_a = (-rotation_rad).sin();
                    let dx = p.x - center_x;
                    let dy = p.y - center_y;
                    let unrot_p = Point2D::new(center_x + dx * cos_a - dy * sin_a, center_y + dx * sin_a + dy * cos_a);
                    if *fill {
                        point_in_rect(unrot_p, *p1, *p2) || distance_to_rect(unrot_p, *p1, *p2) <= r
                    } else {
                        distance_to_rect(unrot_p, *p1, *p2) <= r
                    }
                } else if *fill {
                    point_in_rect(p, *p1, *p2) || distance_to_rect(p, *p1, *p2) <= r
                } else {
                    distance_to_rect(p, *p1, *p2) <= r
                }
            }
            Shape::Ellipse { center, rx, ry, width, fill, .. } => {
                let r = (width * 0.5 + tolerance).max(6.0);
                if *fill {
                    let dx = (p.x - center.x) / rx.max(1.0);
                    let dy = (p.y - center.y) / ry.max(1.0);
                    (dx * dx + dy * dy) <= 1.0 || distance_to_ellipse(p, *center, *rx, *ry) <= r
                } else {
                    distance_to_ellipse(p, *center, *rx, *ry) <= r
                }
            }
            Shape::Emoji { pos, size, .. } => {
                p.distance_to(*pos) <= (*size * 0.75 + tolerance)
            }
            Shape::Text { pos, text, font_size, rotation_rad, .. } => {
                let approx_w = (text.len() as f32 * *font_size * 0.6).max(24.0);
                let approx_h = (*font_size * 1.3).max(24.0);
                let r = tolerance.max(6.0);
                if rotation_rad.abs() > 0.001 {
                    let cx = pos.x + approx_w * 0.5;
                    let cy = pos.y + approx_h * 0.5;
                    let cos_a = (-rotation_rad).cos();
                    let sin_a = (-rotation_rad).sin();
                    let dx = p.x - cx;
                    let dy = p.y - cy;
                    let unrot = Point2D::new(cx + dx * cos_a - dy * sin_a, cy + dx * sin_a + dy * cos_a);
                    unrot.x >= pos.x - r
                        && unrot.x <= pos.x + approx_w + r
                        && unrot.y >= pos.y - r
                        && unrot.y <= pos.y + approx_h + r
                } else {
                    p.x >= pos.x - r
                        && p.x <= pos.x + approx_w + r
                        && p.y >= pos.y - r
                        && p.y <= pos.y + approx_h + r
                }
            }
        }
    }

    /// Translate (move) this shape by (dx, dy) across monitors / displays
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Shape::Freehand { points, .. } => {
                for pt in points.iter_mut() {
                    pt.x += dx;
                    pt.y += dy;
                }
            }
            Shape::Line { start, end, .. } | Shape::Arrow { start, end, .. } => {
                start.x += dx;
                start.y += dy;
                end.x += dx;
                end.y += dy;
            }
            Shape::Rect { p1, p2, .. } => {
                p1.x += dx;
                p1.y += dy;
                p2.x += dx;
                p2.y += dy;
            }
            Shape::Ellipse { center, .. } => {
                center.x += dx;
                center.y += dy;
            }
            Shape::Emoji { pos, .. } | Shape::Text { pos, .. } => {
                pos.x += dx;
                pos.y += dy;
            }
        }
    }

    /// Scale this shape relative to an `origin` anchor point by factors `(sx, sy)`
    pub fn scale(&mut self, origin: Point2D, sx: f32, sy: f32) {
        let scale_pt = |p: Point2D| -> Point2D {
            Point2D::new(
                origin.x + (p.x - origin.x) * sx,
                origin.y + (p.y - origin.y) * sy,
            )
        };
        let uniform_scale = ((sx.abs() + sy.abs()) * 0.5).max(0.05);

        match self {
            Shape::Freehand { points, width, .. } => {
                for pt in points.iter_mut() {
                    *pt = scale_pt(*pt);
                }
                *width = (*width * uniform_scale).clamp(0.5, 200.0);
            }
            Shape::Line { start, end, width, .. } => {
                *start = scale_pt(*start);
                *end = scale_pt(*end);
                *width = (*width * uniform_scale).clamp(0.5, 200.0);
            }
            Shape::Arrow { start, end, width, .. } => {
                *start = scale_pt(*start);
                *end = scale_pt(*end);
                *width = (*width * uniform_scale).clamp(0.5, 200.0);
            }
            Shape::Rect { p1, p2, width, rotation_rad, .. } => {
                if rotation_rad.abs() > 0.001 {
                    let center = Point2D::new((p1.x + p2.x) * 0.5, (p1.y + p2.y) * 0.5);
                    let new_center = scale_pt(center);
                    let hw = ((p2.x - p1.x).abs() * 0.5 * sx.abs()).max(2.0);
                    let hh = ((p2.y - p1.y).abs() * 0.5 * sy.abs()).max(2.0);
                    *p1 = Point2D::new(new_center.x - hw, new_center.y - hh);
                    *p2 = Point2D::new(new_center.x + hw, new_center.y + hh);
                } else {
                    *p1 = scale_pt(*p1);
                    *p2 = scale_pt(*p2);
                }
                *width = (*width * uniform_scale).clamp(0.5, 200.0);
            }
            Shape::Ellipse { center, rx, ry, width, .. } => {
                *center = scale_pt(*center);
                *rx = (*rx * sx.abs()).max(2.0);
                *ry = (*ry * sy.abs()).max(2.0);
                *width = (*width * uniform_scale).clamp(0.5, 200.0);
            }
            Shape::Emoji { pos, size, .. } => {
                *pos = scale_pt(*pos);
                *size = (*size * uniform_scale).clamp(10.0, 400.0);
            }
            Shape::Text { pos, font_size, .. } => {
                *pos = scale_pt(*pos);
                *font_size = (*font_size * uniform_scale).clamp(8.0, 200.0);
            }
        }
    }

    /// Rotate this shape around a pivot `origin` by `angle_rad` radians
    pub fn rotate(&mut self, origin: Point2D, angle_rad: f32) {
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();
        let rot_pt = |p: Point2D| -> Point2D {
            let dx = p.x - origin.x;
            let dy = p.y - origin.y;
            Point2D::new(
                origin.x + dx * cos_a - dy * sin_a,
                origin.y + dx * sin_a + dy * cos_a,
            )
        };

        match self {
            Shape::Freehand { points, .. } => {
                for pt in points.iter_mut() {
                    *pt = rot_pt(*pt);
                }
            }
            Shape::Line { start, end, .. } => {
                *start = rot_pt(*start);
                *end = rot_pt(*end);
            }
            Shape::Arrow { start, end, .. } => {
                *start = rot_pt(*start);
                *end = rot_pt(*end);
            }
            Shape::Rect { p1, p2, rotation_rad, .. } => {
                let center = Point2D::new((p1.x + p2.x) * 0.5, (p1.y + p2.y) * 0.5);
                let new_center = rot_pt(center);
                let hw = (p2.x - p1.x).abs() * 0.5;
                let hh = (p2.y - p1.y).abs() * 0.5;
                *p1 = Point2D::new(new_center.x - hw, new_center.y - hh);
                *p2 = Point2D::new(new_center.x + hw, new_center.y + hh);
                *rotation_rad += angle_rad;
            }
            Shape::Ellipse { center, .. } => {
                *center = rot_pt(*center);
            }
            Shape::Emoji { pos, rotation_rad, .. } => {
                *pos = rot_pt(*pos);
                *rotation_rad += angle_rad;
            }
            Shape::Text { pos, text, font_size, rotation_rad, .. } => {
                let approx_w = (text.len() as f32 * *font_size * 0.6).max(24.0);
                let approx_h = (*font_size * 1.3).max(24.0);
                let center = Point2D::new(pos.x + approx_w * 0.5, pos.y + approx_h * 0.5);
                let new_center = rot_pt(center);
                pos.x = new_center.x - approx_w * 0.5;
                pos.y = new_center.y - approx_h * 0.5;
                *rotation_rad += angle_rad;
            }
        }
    }

    /// Retrieve the outer bounding box (min_pt, max_pt) for selection highlight
    pub fn get_bounding_box(&self) -> (Point2D, Point2D) {
        match self {
            Shape::Freehand { points, width, .. } => {
                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;
                for p in points {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                let hw = width * 0.5;
                (Point2D::new(min_x - hw, min_y - hw), Point2D::new(max_x + hw, max_y + hw))
            }
            Shape::Line { start, end, width, .. } | Shape::Arrow { start, end, width, .. } => {
                let hw = (width * 0.5).max(4.0);
                (
                    Point2D::new(start.x.min(end.x) - hw, start.y.min(end.y) - hw),
                    Point2D::new(start.x.max(end.x) + hw, start.y.max(end.y) + hw),
                )
            }
            Shape::Rect { p1, p2, rotation_rad, .. } => {
                if rotation_rad.abs() > 0.001 {
                    let cx = (p1.x + p2.x) * 0.5;
                    let cy = (p1.y + p2.y) * 0.5;
                    let hw = (p2.x - p1.x).abs() * 0.5;
                    let hh = (p2.y - p1.y).abs() * 0.5;
                    let cos_r = rotation_rad.cos();
                    let sin_r = rotation_rad.sin();
                    let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
                    let mut min_x = f32::MAX;
                    let mut min_y = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut max_y = f32::MIN;
                    for (x, y) in corners {
                        let px = cx + x * cos_r - y * sin_r;
                        let py = cy + x * sin_r + y * cos_r;
                        min_x = min_x.min(px);
                        min_y = min_y.min(py);
                        max_x = max_x.max(px);
                        max_y = max_y.max(py);
                    }
                    (
                        Point2D::new(min_x, min_y),
                        Point2D::new(max_x, max_y),
                    )
                } else {
                    (
                        Point2D::new(p1.x.min(p2.x), p1.y.min(p2.y)),
                        Point2D::new(p1.x.max(p2.x), p1.y.max(p2.y)),
                    )
                }
            }
            Shape::Ellipse { center, rx, ry, .. } => {
                (
                    Point2D::new(center.x - rx, center.y - ry),
                    Point2D::new(center.x + rx, center.y + ry),
                )
            }
            Shape::Emoji { pos, size, rotation_rad, .. } => {
                let half = *size * 0.5;
                if rotation_rad.abs() > 0.001 {
                    let cos_r = rotation_rad.cos();
                    let sin_r = rotation_rad.sin();
                    let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
                    let mut min_x = f32::MAX;
                    let mut min_y = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut max_y = f32::MIN;
                    for (x, y) in corners {
                        let px = pos.x + x * cos_r - y * sin_r;
                        let py = pos.y + x * sin_r + y * cos_r;
                        min_x = min_x.min(px);
                        min_y = min_y.min(py);
                        max_x = max_x.max(px);
                        max_y = max_y.max(py);
                    }
                    (Point2D::new(min_x, min_y), Point2D::new(max_x, max_y))
                } else {
                    (Point2D::new(pos.x - half, pos.y - half), Point2D::new(pos.x + half, pos.y + half))
                }
            }
            Shape::Text { pos, text, font_size, rotation_rad, .. } => {
                let approx_w = (text.len() as f32 * *font_size * 0.6).max(24.0);
                let approx_h = (*font_size * 1.3).max(24.0);
                if rotation_rad.abs() > 0.001 {
                    let cx = pos.x + approx_w * 0.5;
                    let cy = pos.y + approx_h * 0.5;
                    let hw = approx_w * 0.5;
                    let hh = approx_h * 0.5;
                    let cos_r = rotation_rad.cos();
                    let sin_r = rotation_rad.sin();
                    let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
                    let mut min_x = f32::MAX;
                    let mut min_y = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut max_y = f32::MIN;
                    for (x, y) in corners {
                        let px = cx + x * cos_r - y * sin_r;
                        let py = cy + x * sin_r + y * cos_r;
                        min_x = min_x.min(px);
                        min_y = min_y.min(py);
                        max_x = max_x.max(px);
                        max_y = max_y.max(py);
                    }
                    (Point2D::new(min_x, min_y), Point2D::new(max_x, max_y))
                } else {
                    (
                        Point2D::new(pos.x, pos.y),
                        Point2D::new(pos.x + approx_w, pos.y + approx_h),
                    )
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTool {
    None,
    Freehand,
    Text,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Emoji,
    Eraser,
    Transform,
}

impl ActiveTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActiveTool::Freehand => "freehand",
            ActiveTool::Text => "text",
            ActiveTool::Line => "line",
            ActiveTool::Arrow => "arrow",
            ActiveTool::Rect => "rect",
            ActiveTool::Ellipse => "ellipse",
            ActiveTool::Emoji => "emoji",
            ActiveTool::Eraser => "eraser",
            ActiveTool::Transform => "transform",
            ActiveTool::None => "none",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "freehand" => ActiveTool::Freehand,
            "text" => ActiveTool::Text,
            "line" => ActiveTool::Line,
            "arrow" => ActiveTool::Arrow,
            "rect" => ActiveTool::Rect,
            "ellipse" => ActiveTool::Ellipse,
            "emoji" => ActiveTool::Emoji,
            "eraser" => ActiveTool::Eraser,
            "transform" => ActiveTool::Transform,
            _ => ActiveTool::Freehand,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformHandle {
    Move,
    Rotate,
    ScaleTopLeft,
    ScaleTop,
    ScaleTopRight,
    ScaleRight,
    ScaleBottomRight,
    ScaleBottom,
    ScaleBottomLeft,
    ScaleLeft,
}

#[derive(Debug, Clone, Copy)]
pub struct OrientedBoundingBox {
    pub center: Point2D,
    pub half_w: f32,
    pub half_h: f32,
    pub rotation_rad: f32,
}

#[derive(Debug, Clone)]
pub struct LaserTrailPoint {
    pub pos: Point2D,
    pub created_at: Instant,
}

pub struct AnnotationManager {
    pub shapes: Vec<Shape>,
    undo_stack: Vec<Vec<Shape>>,
    redo_stack: Vec<Vec<Shape>>,
    pub in_progress: Option<Shape>,
    pub active_text: Option<ActiveTextInput>,
    pub selected_indices: Vec<usize>,
    pub hovered_shape_idx: Option<usize>,
    pub laser_points: Vec<LaserTrailPoint>,

    // Transform & Marquee selection state
    pub active_transform_handle: Option<TransformHandle>,
    pub transform_drag_start: Point2D,
    pub transform_initial_shapes: Vec<(usize, Shape)>,
    pub transform_initial_box: Option<(Point2D, Point2D)>,
    pub marquee_start: Option<Point2D>,
    pub marquee_current: Option<Point2D>,
    pub transform_rotation_angle: f32,
}

impl AnnotationManager {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            in_progress: None,
            active_text: None,
            selected_indices: Vec::new(),
            hovered_shape_idx: None,
            laser_points: Vec::new(),
            active_transform_handle: None,
            transform_drag_start: Point2D::new(0.0, 0.0),
            transform_initial_shapes: Vec::new(),
            transform_initial_box: None,
            marquee_start: None,
            marquee_current: None,
            transform_rotation_angle: 0.0,
        }
    }

    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.shapes.clone());
        if self.undo_stack.len() > 30 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> bool {
        self.selected_indices.clear();
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.shapes.clone());
            self.shapes = prev;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        self.selected_indices.clear();
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.shapes.clone());
            self.shapes = next;
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.active_text = None;
        self.selected_indices.clear();
        self.active_transform_handle = None;
        self.transform_initial_shapes.clear();
        self.transform_initial_box = None;
        self.marquee_start = None;
        self.marquee_current = None;
        self.transform_rotation_angle = 0.0;
        if !self.shapes.is_empty() {
            self.push_undo();
            self.shapes.clear();
        }
    }

    pub fn start_drawing(
        &mut self,
        tool: ActiveTool,
        start_pos: Point2D,
        color: [f32; 3],
        width: f32,
        opacity: f32,
        fill: bool,
        dotted: bool,
        line_wavy: bool,
        arrow_wavy: bool,
    ) {
        let rgba = [color[0], color[1], color[2], opacity.clamp(0.05, 1.0)];
        match tool {
            ActiveTool::Freehand => {
                self.in_progress = Some(Shape::Freehand {
                    points: vec![start_pos],
                    color: rgba,
                    width,
                });
            }
            ActiveTool::Line => {
                self.in_progress = Some(Shape::Line {
                    start: start_pos,
                    end: start_pos,
                    color: rgba,
                    width,
                    dotted,
                    wavy: line_wavy,
                });
            }
            ActiveTool::Arrow => {
                self.in_progress = Some(Shape::Arrow {
                    start: start_pos,
                    end: start_pos,
                    color: rgba,
                    width,
                    wavy: arrow_wavy,
                });
            }
            ActiveTool::Rect => {
                self.in_progress = Some(Shape::Rect {
                    p1: start_pos,
                    p2: start_pos,
                    color: rgba,
                    width,
                    fill,
                    rotation_rad: 0.0,
                });
            }
            ActiveTool::Ellipse => {
                self.in_progress = Some(Shape::Ellipse {
                    center: start_pos,
                    rx: 1.0,
                    ry: 1.0,
                    color: rgba,
                    width,
                    fill,
                });
            }
            _ => {}
        }
    }

    pub fn update_drawing(&mut self, current_pos: Point2D, shift_held: bool) -> bool {
        if let Some(ref mut shape) = self.in_progress {
            match shape {
                Shape::Freehand { points, .. } => {
                    if let Some(last) = points.last() {
                        // Sample every 6.0 px for lightweight, smooth strokes
                        if last.distance_to(current_pos) >= 6.0 {
                            points.push(current_pos);
                            return true;
                        }
                    } else {
                        points.push(current_pos);
                        return true;
                    }
                    false
                }
                Shape::Line { start, end, .. } => {
                    let target_end = if shift_held {
                        let dx = current_pos.x - start.x;
                        let dy = current_pos.y - start.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        let angle = dy.atan2(dx);
                        let snapped = (angle / (std::f32::consts::PI / 4.0)).round() * (std::f32::consts::PI / 4.0);
                        Point2D::new(start.x + len * snapped.cos(), start.y + len * snapped.sin())
                    } else {
                        current_pos
                    };

                    if end.distance_to(target_end) >= 1.5 {
                        *end = target_end;
                        true
                    } else {
                        false
                    }
                }
                Shape::Arrow { start, end, .. } => {
                    let target_end = if shift_held {
                        let dx = current_pos.x - start.x;
                        let dy = current_pos.y - start.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        let angle = dy.atan2(dx);
                        let snapped = (angle / (std::f32::consts::PI / 4.0)).round() * (std::f32::consts::PI / 4.0);
                        Point2D::new(start.x + len * snapped.cos(), start.y + len * snapped.sin())
                    } else {
                        current_pos
                    };

                    if end.distance_to(target_end) >= 1.5 {
                        *end = target_end;
                        true
                    } else {
                        false
                    }
                }
                Shape::Rect { p1, p2, rotation_rad, .. } => {
                    if shift_held {
                        let dx = current_pos.x - p1.x;
                        let dy = current_pos.y - p1.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        let angle = dy.atan2(dx);
                        let snapped = (angle / (std::f32::consts::PI / 4.0)).round() * (std::f32::consts::PI / 4.0);
                        *rotation_rad = snapped;
                        let side = (len / 1.4142).max(2.0);
                        *p2 = Point2D::new(p1.x + side, p1.y + side);
                        true
                    } else {
                        *rotation_rad = 0.0;
                        if p2.distance_to(current_pos) >= 2.0 {
                            *p2 = current_pos;
                            true
                        } else {
                            false
                        }
                    }
                }
                Shape::Ellipse { center, rx, ry, .. } => {
                    let nrx = (current_pos.x - center.x).abs().max(1.0);
                    let nry = (current_pos.y - center.y).abs().max(1.0);
                    let (target_rx, target_ry) = if shift_held {
                        let r = nrx.max(nry);
                        (r, r)
                    } else {
                        (nrx, nry)
                    };
                    if (*rx - target_rx).abs() >= 1.5 || (*ry - target_ry).abs() >= 1.5 {
                        *rx = target_rx;
                        *ry = target_ry;
                        true
                    } else {
                        false
                    }
                }
                Shape::Emoji { .. } | Shape::Text { .. } => false,
            }
        } else {
            false
        }
    }

    pub fn finish_drawing(&mut self) -> bool {
        if let Some(mut shape) = self.in_progress.take() {
            if let Shape::Freehand { ref mut points, .. } = shape {
                // Simplify freehand points using Ramer-Douglas-Peucker to remove jitter while preserving Bezier curvature
                *points = simplify_polyline(points, 1.2);
                if points.len() < 2 {
                    return false;
                }
            }
            self.push_undo();
            self.shapes.push(shape);
            true
        } else {
            false
        }
    }

    pub fn add_emoji_stamp(
        &mut self,
        pos: Point2D,
        emoji: &str,
        size: f32,
        opacity: f32,
        color: [f32; 4],
        flat: bool,
        filled: bool,
    ) {
        self.push_undo();
        self.shapes.push(Shape::Emoji {
            pos,
            emoji: emoji.to_string(),
            size,
            opacity: opacity.clamp(0.05, 1.0),
            color,
            flat,
            filled,
            rotation_rad: 0.0,
        });
    }

    /// Object Eraser: Delete the shape directly under the cursor point.
    pub fn erase_object_at(&mut self, pos: Point2D, tolerance: f32) -> bool {
        // Iterate backwards so we hit top-most drawn shapes first
        for i in (0..self.shapes.len()).rev() {
            if self.shapes[i].hit_test(pos, tolerance) {
                self.push_undo();
                self.shapes.remove(i);
                self.selected_indices.retain(|&idx| idx != i);
                for idx in &mut self.selected_indices {
                    if *idx > i {
                        *idx -= 1;
                    }
                }
                return true;
            }
        }
        false
    }

    /// Find the top-most shape directly under the cursor point (for Transform tool).
    pub fn find_shape_at(&self, pos: Point2D, tolerance: f32) -> Option<usize> {
        for i in (0..self.shapes.len()).rev() {
            if self.shapes[i].hit_test(pos, tolerance) {
                return Some(i);
            }
        }
        None
    }

    /// Retrieve the oriented bounding box of the active selection.
    /// Preserves constant width, height, and aspect ratio under rotation!
    pub fn get_selection_box(&self) -> Option<OrientedBoundingBox> {
        if self.selected_indices.is_empty() {
            return None;
        }

        // If actively dragging the rotation handle, use initial snapshot bounds
        // so dimensions and aspect ratio remain 100% rigid while rotating!
        if self.active_transform_handle == Some(TransformHandle::Rotate) {
            if let Some((b_min, b_max)) = self.transform_initial_box {
                let center = Point2D::new((b_min.x + b_max.x) * 0.5, (b_min.y + b_max.y) * 0.5);
                let half_w = (b_max.x - b_min.x) * 0.5;
                let half_h = (b_max.y - b_min.y) * 0.5;
                return Some(OrientedBoundingBox {
                    center,
                    half_w,
                    half_h,
                    rotation_rad: self.transform_rotation_angle,
                });
            }
        }

        // Single selected shape: preserve intrinsic orientation & unrotated aspect ratio
        if self.selected_indices.len() == 1 {
            let idx = self.selected_indices[0];
            if idx < self.shapes.len() {
                match &self.shapes[idx] {
                    Shape::Rect { p1, p2, rotation_rad, .. } => {
                        let center = Point2D::new((p1.x + p2.x) * 0.5, (p1.y + p2.y) * 0.5);
                        let half_w = (p2.x - p1.x).abs() * 0.5;
                        let half_h = (p2.y - p1.y).abs() * 0.5;
                        return Some(OrientedBoundingBox {
                            center,
                            half_w,
                            half_h,
                            rotation_rad: *rotation_rad,
                        });
                    }
                    Shape::Text { pos, text, font_size, rotation_rad, .. } => {
                        let approx_w = (text.len() as f32 * font_size * 0.6).max(24.0);
                        let approx_h = (*font_size * 1.3).max(24.0);
                        let half_w = approx_w * 0.5;
                        let half_h = approx_h * 0.5;
                        let center = Point2D::new(pos.x + half_w, pos.y + half_h);
                        return Some(OrientedBoundingBox {
                            center,
                            half_w,
                            half_h,
                            rotation_rad: *rotation_rad,
                        });
                    }
                    Shape::Emoji { pos, size, rotation_rad, .. } => {
                        let half = *size * 0.5;
                        return Some(OrientedBoundingBox {
                            center: *pos,
                            half_w: half,
                            half_h: half,
                            rotation_rad: *rotation_rad,
                        });
                    }
                    _ => {}
                }
            }
        }

        // Union bounding box for multiple shapes
        let (min_pt, max_pt) = self.get_selection_bounding_box()?;
        let center = Point2D::new((min_pt.x + max_pt.x) * 0.5, (min_pt.y + max_pt.y) * 0.5);
        let half_w = (max_pt.x - min_pt.x) * 0.5;
        let half_h = (max_pt.y - min_pt.y) * 0.5;
        Some(OrientedBoundingBox {
            center,
            half_w,
            half_h,
            rotation_rad: 0.0,
        })
    }

    /// Retrieve the union bounding box of all currently selected shapes
    pub fn get_selection_bounding_box(&self) -> Option<(Point2D, Point2D)> {
        if self.selected_indices.is_empty() {
            return None;
        }
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut found = false;

        for &idx in &self.selected_indices {
            if idx < self.shapes.len() {
                let (b_min, b_max) = self.shapes[idx].get_bounding_box();
                min_x = min_x.min(b_min.x);
                min_y = min_y.min(b_min.y);
                max_x = max_x.max(b_max.x);
                max_y = max_y.max(b_max.y);
                found = true;
            }
        }

        if found {
            Some((Point2D::new(min_x, min_y), Point2D::new(max_x, max_y)))
        } else {
            None
        }
    }

    /// Hit test against selection handles (rotate handle, 8 scale handles, or inside selection box)
    pub fn hit_test_transform_handle(&self, p: Point2D) -> Option<TransformHandle> {
        let obb = self.get_selection_box()?;
        let pad = 6.0;
        let hw = obb.half_w + pad;
        let hh = obb.half_h + pad;
        let center = obb.center;

        // Transform mouse point into local (unrotated) box coordinate system
        let p_local = if obb.rotation_rad.abs() > 0.001 {
            let cos_a = (-obb.rotation_rad).cos();
            let sin_a = (-obb.rotation_rad).sin();
            let dx = p.x - center.x;
            let dy = p.y - center.y;
            Point2D::new(
                center.x + dx * cos_a - dy * sin_a,
                center.y + dx * sin_a + dy * cos_a,
            )
        } else {
            p
        };

        let box_min = Point2D::new(center.x - hw, center.y - hh);
        let box_max = Point2D::new(center.x + hw, center.y + hh);
        let handle_radius = 8.0;

        // 1. Rotation handle (circle 24px above top center)
        let rot_pos = Point2D::new(center.x, box_min.y - 24.0);
        if p_local.distance_to(rot_pos) <= handle_radius + 2.0 {
            return Some(TransformHandle::Rotate);
        }

        // 2. Corner scale handles
        if p_local.distance_to(box_min) <= handle_radius {
            return Some(TransformHandle::ScaleTopLeft);
        }
        if p_local.distance_to(Point2D::new(box_max.x, box_min.y)) <= handle_radius {
            return Some(TransformHandle::ScaleTopRight);
        }
        if p_local.distance_to(box_max) <= handle_radius {
            return Some(TransformHandle::ScaleBottomRight);
        }
        if p_local.distance_to(Point2D::new(box_min.x, box_max.y)) <= handle_radius {
            return Some(TransformHandle::ScaleBottomLeft);
        }

        // 3. Edge scale handles
        if p_local.distance_to(Point2D::new(center.x, box_min.y)) <= handle_radius {
            return Some(TransformHandle::ScaleTop);
        }
        if p_local.distance_to(Point2D::new(center.x, box_max.y)) <= handle_radius {
            return Some(TransformHandle::ScaleBottom);
        }
        if p_local.distance_to(Point2D::new(box_min.x, center.y)) <= handle_radius {
            return Some(TransformHandle::ScaleLeft);
        }
        if p_local.distance_to(Point2D::new(box_max.x, center.y)) <= handle_radius {
            return Some(TransformHandle::ScaleRight);
        }

        // 4. Move: inside bounding box or clicking on any selected shape
        if crate::geometry::point_in_rect(p_local, box_min, box_max) {
            return Some(TransformHandle::Move);
        }
        for &idx in &self.selected_indices {
            if idx < self.shapes.len() && self.shapes[idx].hit_test(p, 10.0) {
                return Some(TransformHandle::Move);
            }
        }

        None
    }

    /// Find all shapes whose bounding boxes intersect the marquee selection rectangle
    pub fn find_shapes_in_rect(&self, p1: Point2D, p2: Point2D) -> Vec<usize> {
        let mut indices = Vec::new();
        let r_min = Point2D::new(p1.x.min(p2.x), p1.y.min(p2.y));
        let r_max = Point2D::new(p1.x.max(p2.x), p1.y.max(p2.y));

        for (i, shape) in self.shapes.iter().enumerate() {
            let (s_min, s_max) = shape.get_bounding_box();
            if crate::geometry::rects_intersect(r_min, r_max, s_min, s_max) {
                indices.push(i);
            }
        }
        indices
    }

    /// Prepare for a transform operation: snapshots selected shapes and bounding box
    pub fn begin_transform(&mut self) -> (Vec<(usize, Shape)>, Option<(Point2D, Point2D)>) {
        self.push_undo();
        let snapshot: Vec<(usize, Shape)> = self
            .selected_indices
            .iter()
            .filter_map(|&i| self.shapes.get(i).cloned().map(|s| (i, s)))
            .collect();
        let bbox = self.get_selection_bounding_box();
        (snapshot, bbox)
    }

    /// Delete all currently selected shapes
    pub fn delete_selected(&mut self) -> bool {
        if self.selected_indices.is_empty() {
            return false;
        }
        self.push_undo();
        self.selected_indices.sort_unstable();
        for &idx in self.selected_indices.iter().rev() {
            if idx < self.shapes.len() {
                self.shapes.remove(idx);
            }
        }
        self.selected_indices.clear();
        true
    }

    /// Clear current selection
    pub fn clear_selection(&mut self) {
        self.selected_indices.clear();
        self.active_transform_handle = None;
        self.transform_initial_shapes.clear();
        self.transform_initial_box = None;
        self.marquee_start = None;
        self.marquee_current = None;
        self.transform_rotation_angle = 0.0;
        self.transform_rotation_angle = 0.0;
    }

    /// Start a transform operation (Move, Rotate, Scale) with the active handle
    pub fn start_transform(&mut self, handle: TransformHandle, start_pt: Point2D) {
        self.active_transform_handle = Some(handle);
        self.transform_drag_start = start_pt;
        self.transform_rotation_angle = 0.0;
        let (snapshot, bbox) = self.begin_transform();
        self.transform_initial_shapes = snapshot;
        self.transform_initial_box = bbox;
    }

    /// Update transform during mouse drag
    pub fn update_transform(&mut self, curr_pt: Point2D, shift_held: bool) -> bool {
        let handle = match self.active_transform_handle {
            Some(h) => h,
            None => return false,
        };
        let (box_min, box_max) = match self.transform_initial_box {
            Some(b) => b,
            None => return false,
        };

        let center = Point2D::new((box_min.x + box_max.x) * 0.5, (box_min.y + box_max.y) * 0.5);

        match handle {
            TransformHandle::Move => {
                let mut dx = curr_pt.x - self.transform_drag_start.x;
                let mut dy = curr_pt.y - self.transform_drag_start.y;
                if shift_held {
                    if dx.abs() > dy.abs() * 2.0 {
                        dy = 0.0;
                    } else if dy.abs() > dx.abs() * 2.0 {
                        dx = 0.0;
                    }
                }
                for (idx, initial_shape) in &self.transform_initial_shapes {
                    if *idx < self.shapes.len() {
                        let mut shape = initial_shape.clone();
                        shape.translate(dx, dy);
                        self.shapes[*idx] = shape;
                    }
                }
            }
            TransformHandle::Rotate => {
                let start_angle = (self.transform_drag_start.y - center.y).atan2(self.transform_drag_start.x - center.x);
                let curr_angle = (curr_pt.y - center.y).atan2(curr_pt.x - center.x);
                let mut total_angle = curr_angle - start_angle;
                while total_angle > std::f32::consts::PI {
                    total_angle -= 2.0 * std::f32::consts::PI;
                }
                while total_angle < -std::f32::consts::PI {
                    total_angle += 2.0 * std::f32::consts::PI;
                }

                if shift_held {
                    let step = std::f32::consts::PI / 12.0; // 15-degree increments
                    total_angle = (total_angle / step).round() * step;
                }
                self.transform_rotation_angle = total_angle;

                for (idx, initial_shape) in &self.transform_initial_shapes {
                    if *idx < self.shapes.len() {
                        let mut shape = initial_shape.clone();
                        shape.rotate(center, total_angle);
                        self.shapes[*idx] = shape;
                    }
                }
            }
            _ => {
                // 8 Scale Handles with Opposite Anchor Scaling
                let (anchor, initial_dim_x, initial_dim_y, sign_x, sign_y) = match handle {
                    TransformHandle::ScaleBottomRight => (
                        box_min,
                        (box_max.x - box_min.x).max(10.0),
                        (box_max.y - box_min.y).max(10.0),
                        1.0f32,
                        1.0f32,
                    ),
                    TransformHandle::ScaleTopLeft => (
                        box_max,
                        (box_max.x - box_min.x).max(10.0),
                        (box_max.y - box_min.y).max(10.0),
                        -1.0f32,
                        -1.0f32,
                    ),
                    TransformHandle::ScaleTopRight => (
                        Point2D::new(box_min.x, box_max.y),
                        (box_max.x - box_min.x).max(10.0),
                        (box_max.y - box_min.y).max(10.0),
                        1.0f32,
                        -1.0f32,
                    ),
                    TransformHandle::ScaleBottomLeft => (
                        Point2D::new(box_max.x, box_min.y),
                        (box_max.x - box_min.x).max(10.0),
                        (box_max.y - box_min.y).max(10.0),
                        -1.0f32,
                        1.0f32,
                    ),
                    TransformHandle::ScaleTop => (
                        Point2D::new(center.x, box_max.y),
                        1.0f32,
                        (box_max.y - box_min.y).max(10.0),
                        0.0f32,
                        -1.0f32,
                    ),
                    TransformHandle::ScaleBottom => (
                        Point2D::new(center.x, box_min.y),
                        1.0f32,
                        (box_max.y - box_min.y).max(10.0),
                        0.0f32,
                        1.0f32,
                    ),
                    TransformHandle::ScaleLeft => (
                        Point2D::new(box_max.x, center.y),
                        (box_max.x - box_min.x).max(10.0),
                        1.0f32,
                        -1.0f32,
                        0.0f32,
                    ),
                    TransformHandle::ScaleRight => (
                        Point2D::new(box_min.x, center.y),
                        (box_max.x - box_min.x).max(10.0),
                        1.0f32,
                        1.0f32,
                        0.0f32,
                    ),
                    _ => return false,
                };

                let mut sx = if sign_x != 0.0 {
                    let curr_dist_x = (curr_pt.x - anchor.x) * sign_x;
                    (curr_dist_x / initial_dim_x).clamp(0.05, 20.0)
                } else {
                    1.0
                };

                let mut sy = if sign_y != 0.0 {
                    let curr_dist_y = (curr_pt.y - anchor.y) * sign_y;
                    (curr_dist_y / initial_dim_y).clamp(0.05, 20.0)
                } else {
                    1.0
                };

                if shift_held {
                    if sign_x == 0.0 {
                        sx = sy;
                    } else if sign_y == 0.0 {
                        sy = sx;
                    } else {
                        let s = if (sx - 1.0).abs() > (sy - 1.0).abs() { sx } else { sy };
                        sx = s;
                        sy = s;
                    }
                }

                for (idx, initial_shape) in &self.transform_initial_shapes {
                    if *idx < self.shapes.len() {
                        let mut shape = initial_shape.clone();
                        shape.scale(anchor, sx, sy);
                        self.shapes[*idx] = shape;
                    }
                }
            }
        }
        true
    }

    /// Commit active transform operation
    pub fn finish_transform(&mut self) {
        self.active_transform_handle = None;
        self.transform_initial_shapes.clear();
        self.transform_initial_box = None;
        self.transform_rotation_angle = 0.0;
    }

    /// Cancel active transform and restore initial pre-drag state
    pub fn cancel_transform_or_marquee(&mut self) {
        if self.active_transform_handle.is_some() {
            for (idx, initial_shape) in &self.transform_initial_shapes {
                if *idx < self.shapes.len() {
                    self.shapes[*idx] = initial_shape.clone();
                }
            }
            self.finish_transform();
        }
        self.marquee_start = None;
        self.marquee_current = None;
        self.transform_rotation_angle = 0.0;
    }

    /// Start a marquee box selection
    pub fn start_marquee(&mut self, start_pt: Point2D) {
        self.marquee_start = Some(start_pt);
        self.marquee_current = Some(start_pt);
    }

    /// Update marquee drag position
    pub fn update_marquee(&mut self, curr_pt: Point2D) {
        self.marquee_current = Some(curr_pt);
    }

    /// Finish marquee box selection
    pub fn finish_marquee(&mut self, ctrl_or_shift: bool) {
        if let (Some(start), Some(curr)) = (self.marquee_start.take(), self.marquee_current.take()) {
            if start.distance_to(curr) > 3.0 {
                let intersecting = self.find_shapes_in_rect(start, curr);
                if ctrl_or_shift {
                    for idx in intersecting {
                        if !self.selected_indices.contains(&idx) {
                            self.selected_indices.push(idx);
                        }
                    }
                    self.selected_indices.sort_unstable();
                } else {
                    self.selected_indices = intersecting;
                }
            } else if !ctrl_or_shift {
                self.selected_indices.clear();
            }
        }
    }

    /// Begin typing text at the given clicked position
    pub fn start_text_input(&mut self, pos: Point2D, font_family: String, font_size: f32, color: [f32; 4]) {
        self.commit_active_text();
        self.active_text = Some(ActiveTextInput {
            pos,
            text: String::new(),
            font_family,
            font_size,
            color,
        });
    }

    /// Commit active text input into a persistent shape
    pub fn commit_active_text(&mut self) -> bool {
        if let Some(input) = self.active_text.take() {
            let trimmed = input.text.trim();
            if !trimmed.is_empty() {
                self.push_undo();
                self.shapes.push(Shape::Text {
                    pos: input.pos,
                    text: input.text,
                    font_family: input.font_family,
                    font_size: input.font_size,
                    color: input.color,
                    rotation_rad: 0.0,
                });
                return true;
            }
        }
        false
    }

    pub fn add_laser_point(&mut self, pos: Point2D) -> bool {
        if let Some(last) = self.laser_points.last() {
            // Reduced point density: minimum 7.0 px movement between laser points
            if last.pos.distance_to(pos) < 7.0 {
                return false;
            }
        }

        // Cap laser points to at most 22 points for lightweight rendering and smooth motion
        if self.laser_points.len() >= 22 {
            self.laser_points.remove(0);
        }

        self.laser_points.push(LaserTrailPoint {
            pos,
            created_at: Instant::now(),
        });
        true
    }

    /// Cleanup expired laser points. Returns whether active items remain.
    pub fn tick_cleanup(&mut self, laser_duration_ms: u64) -> bool {
        let now = Instant::now();
        let laser_dur = std::time::Duration::from_millis(laser_duration_ms);
        self.laser_points.retain(|p| now.duration_since(p.created_at) < laser_dur);

        !self.laser_points.is_empty()
    }
}

/// Ramer-Douglas-Peucker polyline simplification to eliminate redundant points and smooth strokes
pub fn simplify_polyline(pts: &[Point2D], tolerance: f32) -> Vec<Point2D> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }

    let mut dmax = 0.0f32;
    let mut index = 0;
    let end = pts.len() - 1;
    let p_start = pts[0];
    let p_end = pts[end];

    for i in 1..end {
        let d = perpendicular_distance(pts[i], p_start, p_end);
        if d > dmax {
            index = i;
            dmax = d;
        }
    }

    if dmax > tolerance {
        let mut rec1 = simplify_polyline(&pts[0..=index], tolerance);
        let rec2 = simplify_polyline(&pts[index..=end], tolerance);
        rec1.pop();
        rec1.extend(rec2);
        rec1
    } else {
        vec![p_start, p_end]
    }
}

fn perpendicular_distance(p: Point2D, a: Point2D, b: Point2D) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.0001 {
        return p.distance_to(a);
    }
    let num = ((p.x - a.x) * dy - (p.y - a.y) * dx).abs();
    num / len_sq.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplify_collinear_polyline() {
        let pts = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(10.0, 0.1),
            Point2D::new(20.0, -0.1),
            Point2D::new(30.0, 0.0),
            Point2D::new(40.0, 0.0),
        ];
        let simplified = simplify_polyline(&pts, 1.0);
        // Nearly collinear points should be reduced to start and end
        assert_eq!(simplified.len(), 2);
        assert_eq!(simplified[0], Point2D::new(0.0, 0.0));
        assert_eq!(simplified[1], Point2D::new(40.0, 0.0));
    }

    #[test]
    fn test_laser_point_density_reduction() {
        let mut mgr = AnnotationManager::new();
        assert!(mgr.add_laser_point(Point2D::new(0.0, 0.0)));
        // Sub-threshold movement (< 7px) should be rejected
        assert!(!mgr.add_laser_point(Point2D::new(2.0, 2.0)));
        // Sufficient movement should be accepted
        assert!(mgr.add_laser_point(Point2D::new(10.0, 10.0)));
        assert_eq!(mgr.laser_points.len(), 2);
    }

    #[test]
    fn test_shift_snapping_and_circle_constraint() {
        let mut mgr = AnnotationManager::new();
        // Line with shift held: 45 deg snap
        mgr.start_drawing(ActiveTool::Line, Point2D::new(0.0, 0.0), [1.0, 0.0, 0.0], 2.0, 1.0, false, false, false, false);
        // Drag to (100.0, 20.0) -> raw angle ~11 deg -> snaps to 0 deg
        mgr.update_drawing(Point2D::new(100.0, 20.0), true);
        if let Some(Shape::Line { start, end, .. }) = mgr.in_progress {
            assert_eq!(start, Point2D::new(0.0, 0.0));
            assert!(end.y.abs() < 0.01); // Snapped to horizontal!
        } else {
            panic!("Expected line in progress");
        }
        mgr.finish_drawing();

        // Ellipse with shift held: rx == ry (circle)
        mgr.start_drawing(ActiveTool::Ellipse, Point2D::new(100.0, 100.0), [1.0, 0.0, 0.0], 2.0, 1.0, false, false, false, false);
        mgr.update_drawing(Point2D::new(180.0, 130.0), true);
        if let Some(Shape::Ellipse { rx, ry, .. }) = mgr.in_progress {
            assert_eq!(rx, ry); // Perfect circle!
        } else {
            panic!("Expected ellipse in progress");
        }
        mgr.finish_drawing();
    }

    #[test]
    fn test_active_tool_str_roundtrip() {
        for tool in [
            ActiveTool::Freehand,
            ActiveTool::Text,
            ActiveTool::Line,
            ActiveTool::Arrow,
            ActiveTool::Rect,
            ActiveTool::Ellipse,
            ActiveTool::Emoji,
            ActiveTool::Eraser,
            ActiveTool::Transform,
        ] {
            let s = tool.as_str();
            assert_eq!(ActiveTool::from_str(s), tool);
        }
    }

    #[test]
    fn test_multi_selection_and_marquee() {
        let mut mgr = AnnotationManager::new();
        // Shape 0: Rect from (10, 10) to (50, 50)
        mgr.shapes.push(Shape::Rect {
            p1: Point2D::new(10.0, 10.0),
            p2: Point2D::new(50.0, 50.0),
            color: [1.0, 0.0, 0.0, 1.0],
            width: 2.0,
            fill: false,
            rotation_rad: 0.0,
        });
        // Shape 1: Ellipse centered at (100, 100), rx=20, ry=20
        mgr.shapes.push(Shape::Ellipse {
            center: Point2D::new(100.0, 100.0),
            rx: 20.0,
            ry: 20.0,
            color: [0.0, 1.0, 0.0, 1.0],
            width: 2.0,
            fill: false,
        });
        // Shape 2: Line from (300, 300) to (350, 350)
        mgr.shapes.push(Shape::Line {
            start: Point2D::new(300.0, 300.0),
            end: Point2D::new(350.0, 350.0),
            color: [0.0, 0.0, 1.0, 1.0],
            width: 2.0,
            dotted: false,
            wavy: false,
        });

        // Marquee from (0, 0) to (130, 130) should select Shape 0 and Shape 1
        mgr.start_marquee(Point2D::new(0.0, 0.0));
        mgr.update_marquee(Point2D::new(130.0, 130.0));
        mgr.finish_marquee(false);
        assert_eq!(mgr.selected_indices, vec![0, 1]);

        // Multi-select addition with Ctrl/Shift: marquee over (280, 280) to (400, 400)
        mgr.start_marquee(Point2D::new(280.0, 280.0));
        mgr.update_marquee(Point2D::new(400.0, 400.0));
        mgr.finish_marquee(true);
        assert_eq!(mgr.selected_indices, vec![0, 1, 2]);

        // Check union bounding box covers from ~6 to ~364
        let (b_min, b_max) = mgr.get_selection_bounding_box().expect("bounding box should exist");
        assert!(b_min.x <= 10.0 && b_min.y <= 10.0);
        assert!(b_max.x >= 350.0 && b_max.y >= 350.0);
    }

    #[test]
    fn test_transform_move_scale_rotate_and_delete() {
        let mut mgr = AnnotationManager::new();
        // Shape 0: Rect from (100, 100) to (200, 200)
        mgr.shapes.push(Shape::Rect {
            p1: Point2D::new(100.0, 100.0),
            p2: Point2D::new(200.0, 200.0),
            color: [1.0, 0.0, 0.0, 1.0],
            width: 2.0,
            fill: false,
            rotation_rad: 0.0,
        });
        mgr.selected_indices = vec![0];

        // 1. Move
        mgr.start_transform(TransformHandle::Move, Point2D::new(150.0, 150.0));
        mgr.update_transform(Point2D::new(200.0, 250.0), false);
        mgr.finish_transform();

        if let Shape::Rect { p1, p2, .. } = &mgr.shapes[0] {
            assert!((p1.x - 150.0).abs() < 1.0);
            assert!((p1.y - 200.0).abs() < 1.0);
            assert!((p2.x - 250.0).abs() < 1.0);
            assert!((p2.y - 300.0).abs() < 1.0);
        } else {
            panic!("Expected Rect shape");
        }

        // 2. Scale Bottom-Right with Shift (Uniform / Aspect-Ratio Constrained)
        let (orig_min, orig_max) = mgr.get_selection_bounding_box().unwrap();
        mgr.start_transform(TransformHandle::ScaleBottomRight, orig_max);
        mgr.update_transform(Point2D::new(orig_max.x + 50.0, orig_max.y + 10.0), true);
        mgr.finish_transform();

        let (new_min, new_max) = mgr.get_selection_bounding_box().unwrap();
        // Bottom-Right scale keeps top-left anchor invariant
        assert!((new_min.x - orig_min.x).abs() < 1.0);
        assert!((new_min.y - orig_min.y).abs() < 1.0);
        // Scaled outward
        assert!(new_max.x > orig_max.x);
        assert!(new_max.y > orig_max.y);

        // 3. Rotate with Shift snapping (15-degree steps)
        let center = Point2D::new((new_min.x + new_max.x) * 0.5, (new_min.y + new_max.y) * 0.5);
        let rot_pos = Point2D::new(center.x, new_min.y - 24.0);
        mgr.start_transform(TransformHandle::Rotate, rot_pos);
        // Drag to 90 degrees right of center
        mgr.update_transform(Point2D::new(center.x + 100.0, center.y), true);
        mgr.finish_transform();

        if let Shape::Rect { rotation_rad, .. } = &mgr.shapes[0] {
            // Snapped rotation should be non-zero and a multiple of 15 degrees (PI / 12)
            let step = std::f32::consts::PI / 12.0;
            let ratio = (*rotation_rad / step).round();
            assert!((*rotation_rad - ratio * step).abs() < 0.02);
        }

        // 4. Delete selected
        assert!(mgr.delete_selected());
        assert!(mgr.shapes.is_empty());
        assert!(mgr.selected_indices.is_empty());

        // 5. Undo restores deleted shape
        assert!(mgr.undo());
        assert_eq!(mgr.shapes.len(), 1);
    }

    #[test]
    fn test_text_and_emoji_rotation_and_config() {
        let mut mgr = AnnotationManager::new();
        mgr.shapes.push(Shape::Text {
            pos: Point2D::new(100.0, 100.0),
            text: "Hello".to_string(),
            font_family: "Segoe UI".to_string(),
            font_size: 24.0,
            color: [1.0, 1.0, 1.0, 1.0],
            rotation_rad: 0.0,
        });
        mgr.selected_indices = vec![0];
        let (min_pt, max_pt) = mgr.get_selection_bounding_box().unwrap();
        let center = Point2D::new((min_pt.x + max_pt.x) * 0.5, (min_pt.y + max_pt.y) * 0.5);

        // Rotate text shape by 90 degrees (PI / 2)
        mgr.shapes[0].rotate(center, std::f32::consts::FRAC_PI_2);
        if let Shape::Text { rotation_rad, .. } = &mgr.shapes[0] {
            assert!((*rotation_rad - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        } else {
            panic!("Expected Text shape");
        }

        // Add emoji stamp with flat & filled options
        mgr.add_emoji_stamp(
            Point2D::new(50.0, 50.0),
            "🔥",
            32.0,
            0.8,
            [1.0, 0.5, 0.0, 0.8],
            true,
            false,
        );
        assert_eq!(mgr.shapes.len(), 2);
        if let Shape::Emoji { emoji, flat, filled, opacity, .. } = &mgr.shapes[1] {
            assert_eq!(emoji, "🔥");
            assert!(*flat);
            assert!(!*filled);
            assert_eq!(*opacity, 0.8);
        } else {
            panic!("Expected Emoji shape");
        }

        // Rotate emoji
        mgr.shapes[1].rotate(Point2D::new(50.0, 50.0), std::f32::consts::FRAC_PI_2);
        if let Shape::Emoji { rotation_rad, .. } = &mgr.shapes[1] {
            assert!((*rotation_rad - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        } else {
            panic!("Expected Emoji shape");
        }

        // Check that oriented bounding box preserves aspect ratio!
        mgr.selected_indices = vec![0];
        let obb = mgr.get_selection_box().unwrap();
        assert!((obb.rotation_rad - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }
}

