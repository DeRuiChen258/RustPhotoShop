//! Integer rectangles and the layer transform.

/// A half-open rectangle in document pixels: `x0..x1`, `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RectI {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl RectI {
    #[must_use]
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    #[must_use]
    pub fn from_size(x: i64, y: i64, width: u32, height: u32) -> Self {
        Self {
            x0: x,
            y0: y,
            x1: x + i64::from(width),
            y1: y + i64::from(height),
        }
    }

    #[must_use]
    pub fn width(&self) -> i64 {
        (self.x1 - self.x0).max(0)
    }

    #[must_use]
    pub fn height(&self) -> i64 {
        (self.y1 - self.y0).max(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    #[must_use]
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    #[must_use]
    pub fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    #[must_use]
    pub fn translated(&self, dx: i64, dy: i64) -> Self {
        Self {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }
}

/// How a resample reads pixels: the same three choices the project format stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Sampling {
    #[default]
    #[serde(rename = "High quality")]
    HighQuality,
    #[serde(rename = "Smooth")]
    Smooth,
    #[serde(rename = "Nearest")]
    Nearest,
}

impl Sampling {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::HighQuality => "High quality",
            Self::Smooth => "Smooth",
            Self::Nearest => "Nearest",
        }
    }
}

/// Where a layer's pixels sit on the canvas: a box, turned by `rotation` degrees clockwise
/// around the box's center, then flipped.
///
/// The project format stores `origin` as the top-left corner of the untransformed box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub origin: [f64; 2],
    pub size: [f64; 2],
    /// Degrees, clockwise.
    pub rotation: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub sampling: Sampling,
}

impl Transform {
    /// A box covering the whole canvas.
    #[must_use]
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            origin: [0.0, 0.0],
            size: [f64::from(width), f64::from(height)],
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            sampling: Sampling::HighQuality,
        }
    }

    /// A box covering the whole canvas, in the canvas's own coordinates.
    #[must_use]
    pub fn identity(width: u32, height: u32) -> Self {
        Self::full(width, height)
    }

    #[must_use]
    pub fn center(&self) -> [f64; 2] {
        [
            self.origin[0] + self.size[0] / 2.0,
            self.origin[1] + self.size[1] / 2.0,
        ]
    }

    /// True when the transform is a plain 1:1 placement of `image_size` at the origin.
    #[must_use]
    pub fn is_identity_for(&self, image_size: [f64; 2]) -> bool {
        self.rotation == 0.0
            && !self.flip_x
            && !self.flip_y
            && (self.size[0] - image_size[0]).abs() < 1e-6
            && (self.size[1] - image_size[1]).abs() < 1e-6
    }

    /// The four corners of the transformed box, clockwise from the top-left.
    #[must_use]
    pub fn quad(&self) -> [[f64; 2]; 4] {
        let corners = [
            [0.0, 0.0],
            [self.size[0], 0.0],
            [self.size[0], self.size[1]],
            [0.0, self.size[1]],
        ];
        let mut out = [[0.0_f64; 2]; 4];
        for (index, corner) in corners.iter().enumerate() {
            out[index] = self.box_to_document(*corner);
        }
        out
    }

    /// Maps a point in the layer's box (0..size) to document coordinates.
    #[must_use]
    pub fn box_to_document(&self, point: [f64; 2]) -> [f64; 2] {
        // The box is turned about its own center, which sits at `origin + size / 2`.
        let half = [self.size[0] / 2.0, self.size[1] / 2.0];
        let radians = self.rotation.to_radians();
        let (sin, cos) = radians.sin_cos();
        let x = point[0] - half[0];
        let y = point[1] - half[1];
        // Clockwise on a y-down canvas.
        [
            self.origin[0] + half[0] + x * cos - y * sin,
            self.origin[1] + half[1] + x * sin + y * cos,
        ]
    }

    /// Maps a document point into the layer's box; may fall outside `0..size`.
    #[must_use]
    pub fn document_to_box(&self, point: [f64; 2]) -> [f64; 2] {
        let half = [self.size[0] / 2.0, self.size[1] / 2.0];
        let radians = (-self.rotation).to_radians();
        let (sin, cos) = radians.sin_cos();
        let x = point[0] - self.origin[0] - half[0];
        let y = point[1] - self.origin[1] - half[1];
        [half[0] + x * cos - y * sin, half[1] + x * sin + y * cos]
    }

    /// Maps a document point to a pixel position inside an `image_size` image, or `None` when it
    /// falls outside the image.
    #[must_use]
    pub fn document_to_image(&self, point: [f64; 2], image_size: [f64; 2]) -> Option<[f64; 2]> {
        if self.size[0] <= 0.0 || self.size[1] <= 0.0 {
            return None;
        }
        let local = self.document_to_box(point);
        // `document_to_box` already answers in box coordinates, so the box origin is not
        // subtracted again here.
        let mut u = local[0] / self.size[0];
        let mut v = local[1] / self.size[1];
        if self.flip_x {
            u = 1.0 - u;
        }
        if self.flip_y {
            v = 1.0 - v;
        }
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            return None;
        }
        Some([u * image_size[0], v * image_size[1]])
    }

    /// Document pixels per image pixel, horizontally and vertically.
    #[must_use]
    pub fn scale_factors(&self, image_size: [f64; 2]) -> [f64; 2] {
        [
            if image_size[0] > 0.0 {
                self.size[0] / image_size[0]
            } else {
                1.0
            },
            if image_size[1] > 0.0 {
                self.size[1] / image_size[1]
            } else {
                1.0
            },
        ]
    }

    /// The axis-aligned document rectangle the transformed box covers.
    #[must_use]
    pub fn document_bounds(&self) -> RectI {
        let quad = self.quad();
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for corner in quad {
            min_x = min_x.min(corner[0]);
            min_y = min_y.min(corner[1]);
            max_x = max_x.max(corner[0]);
            max_y = max_y.max(corner[1]);
        }
        RectI {
            x0: snap_out(min_x, f64::floor),
            y0: snap_out(min_y, f64::floor),
            x1: snap_out(max_x, f64::ceil),
            y1: snap_out(max_y, f64::ceil),
        }
    }
}

/// Rounds a coordinate that is a hair away from a whole number before floor/ceil, so a rotated
/// box does not grow by a pixel from floating-point noise.
fn snap_out(value: f64, round: fn(f64) -> f64) -> i64 {
    let nearest = value.round();
    if (value - nearest).abs() < 1e-6 {
        nearest as i64
    } else {
        round(value) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_intersection_and_containment() {
        let rect = RectI::new(0, 0, 10, 10);
        assert!(rect.contains(9, 9));
        assert!(!rect.contains(10, 10));
        let other = RectI::new(5, 5, 20, 20);
        assert_eq!(rect.intersect(&other), RectI::new(5, 5, 10, 10));
        assert!(RectI::new(20, 20, 21, 21).intersect(&rect).is_empty());
    }

    #[test]
    fn identity_transform_maps_document_to_image() {
        let transform = Transform::full(100, 50);
        let mapped = transform
            .document_to_image([10.0, 20.0], [100.0, 50.0])
            .expect("inside");
        assert!((mapped[0] - 10.0).abs() < 1e-9);
        assert!((mapped[1] - 20.0).abs() < 1e-9);
        assert!(
            transform
                .document_to_image([100.0, 20.0], [100.0, 50.0])
                .is_none()
        );
    }

    #[test]
    fn scaled_and_flipped_transform_round_trips() {
        let mut transform = Transform::full(100, 100);
        transform.origin = [10.0, 20.0];
        transform.size = [50.0, 25.0];
        transform.flip_x = true;
        let document_point = transform.box_to_document([10.0, 5.0]);
        let back = transform.document_to_image(document_point, [100.0, 50.0]);
        let back = back.expect("inside image");
        // box (10, 5) of 50×25 is normalized (0.2, 0.2), flipped horizontally to (0.8, 0.2).
        assert!((back[0] - 80.0).abs() < 1e-6, "got {back:?}");
        assert!((back[1] - 10.0).abs() < 1e-6, "got {back:?}");
    }

    #[test]
    fn rotation_moves_the_bounds() {
        let mut transform = Transform::full(100, 100);
        transform.rotation = 90.0;
        let bounds = transform.document_bounds();
        assert_eq!(bounds.width(), 100);
        assert_eq!(bounds.height(), 100);
        transform.size = [200.0, 50.0];
        let bounds = transform.document_bounds();
        assert_eq!(bounds.width(), 50);
        assert_eq!(bounds.height(), 200);
    }

    #[test]
    fn scale_factors_report_document_pixels_per_image_pixel() {
        let mut transform = Transform::full(100, 100);
        transform.size = [200.0, 50.0];
        let factors = transform.scale_factors([100.0, 100.0]);
        assert!((factors[0] - 2.0).abs() < 1e-9);
        assert!((factors[1] - 0.5).abs() < 1e-9);
    }
}
