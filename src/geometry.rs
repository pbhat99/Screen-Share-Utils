use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

impl Point2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance_to(self, other: Point2D) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn normalized(self) -> Self {
        let len = self.length();
        if len > 1e-6 {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        } else {
            Self { x: 0.0, y: 0.0 }
        }
    }
}

/// Calculate the shortest distance from a point `p` to a line segment `(a, b)`.
pub fn distance_to_segment(p: Point2D, a: Point2D, b: Point2D) -> f32 {
    let l2 = (b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y);
    if l2 < 1e-6 {
        return p.distance_to(a);
    }
    // Projection factor t
    let t = ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / l2;
    let t_clamped = t.clamp(0.0, 1.0);
    let proj = Point2D {
        x: a.x + t_clamped * (b.x - a.x),
        y: a.y + t_clamped * (b.y - a.y),
    };
    p.distance_to(proj)
}

/// Calculate distance to a rectangle boundary or interior.
pub fn distance_to_rect(p: Point2D, p1: Point2D, p2: Point2D) -> f32 {
    let min_x = p1.x.min(p2.x);
    let max_x = p1.x.max(p2.x);
    let min_y = p1.y.min(p2.y);
    let max_y = p1.y.max(p2.y);

    let top_dist = distance_to_segment(p, Point2D::new(min_x, min_y), Point2D::new(max_x, min_y));
    let bottom_dist = distance_to_segment(p, Point2D::new(min_x, max_y), Point2D::new(max_x, max_y));
    let left_dist = distance_to_segment(p, Point2D::new(min_x, min_y), Point2D::new(min_x, max_y));
    let right_dist = distance_to_segment(p, Point2D::new(max_x, min_y), Point2D::new(max_x, max_y));

    top_dist.min(bottom_dist).min(left_dist).min(right_dist)
}

/// Check if point is inside a rectangle.
pub fn point_in_rect(p: Point2D, p1: Point2D, p2: Point2D) -> bool {
    let min_x = p1.x.min(p2.x);
    let max_x = p1.x.max(p2.x);
    let min_y = p1.y.min(p2.y);
    let max_y = p1.y.max(p2.y);

    p.x >= min_x && p.x <= max_x && p.y >= min_y && p.y <= max_y
}

/// Calculate distance to an ellipse perimeter.
pub fn distance_to_ellipse(p: Point2D, center: Point2D, rx: f32, ry: f32) -> f32 {
    if rx < 1.0 || ry < 1.0 {
        return p.distance_to(center);
    }
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    let angle = dy.atan2(dx);
    let perimeter_point = Point2D::new(
        center.x + rx * angle.cos(),
        center.y + ry * angle.sin(),
    );
    p.distance_to(perimeter_point)
}

/// Calculate arrowhead wing points for an arrow from `start` to `end`.
pub fn calculate_arrow_head(start: Point2D, end: Point2D, head_length: f32, head_angle_rad: f32) -> (Point2D, Point2D) {
    let dir = Point2D::new(end.x - start.x, end.y - start.y).normalized();
    let angle = dir.y.atan2(dir.x);

    let wing1 = Point2D::new(
        end.x - head_length * (angle + head_angle_rad).cos(),
        end.y - head_length * (angle + head_angle_rad).sin(),
    );
    let wing2 = Point2D::new(
        end.x - head_length * (angle - head_angle_rad).cos(),
        end.y - head_length * (angle - head_angle_rad).sin(),
    );

    (wing1, wing2)
}

/// Check if two axis-aligned bounding boxes (defined by min and max points) intersect.
pub fn rects_intersect(a_min: Point2D, a_max: Point2D, b_min: Point2D, b_max: Point2D) -> bool {
    let a_l = a_min.x.min(a_max.x);
    let a_r = a_min.x.max(a_max.x);
    let a_t = a_min.y.min(a_max.y);
    let a_b = a_min.y.max(a_max.y);

    let b_l = b_min.x.min(b_max.x);
    let b_r = b_min.x.max(b_max.x);
    let b_t = b_min.y.min(b_max.y);
    let b_b = b_min.y.max(b_max.y);

    !(a_r < b_l || a_l > b_r || a_b < b_t || a_t > b_b)
}


