//! Selections: a document-sized coverage mask, created by the marquee, the magic wand or the
//! Select menu.

use crate::core::geometry::RectI;
use crate::core::pixel::GrayImage;

/// How a new shape joins the selection that is already there — the same four choices the
/// modifier keys give the marquee tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}

/// The pixels an edit is limited to, stored at document resolution: 0 is outside, 255 inside,
/// with soft edges in between.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    mask: GrayImage,
}

impl Selection {
    /// A selection holding everything.
    #[must_use]
    pub fn all(width: u32, height: u32) -> Self {
        Self {
            mask: GrayImage::new(width, height, 255),
        }
    }

    /// A rectangular selection, clipped to the canvas.
    #[must_use]
    pub fn rectangle(width: u32, height: u32, rect: RectI) -> Self {
        let mut mask = GrayImage::new(width, height, 0);
        let rect = rect.intersect(&RectI::new(0, 0, i64::from(width), i64::from(height)));
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                mask.set_pixel(x as u32, y as u32, 255);
            }
        }
        Self { mask }
    }

    /// An elliptical selection inscribed in `rect`, with a one-pixel soft edge.
    #[must_use]
    pub fn ellipse(width: u32, height: u32, rect: RectI) -> Self {
        let mut mask = GrayImage::new(width, height, 0);
        let rect = rect.intersect(&RectI::new(0, 0, i64::from(width), i64::from(height)));
        let radius_x = rect.width() as f64 / 2.0;
        let radius_y = rect.height() as f64 / 2.0;
        if radius_x <= 0.0 || radius_y <= 0.0 {
            return Self { mask };
        }
        let center_x = rect.x0 as f64 + radius_x;
        let center_y = rect.y0 as f64 + radius_y;
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let dx = (x as f64 + 0.5 - center_x) / radius_x;
                let dy = (y as f64 + 0.5 - center_y) / radius_y;
                let distance = (dx * dx + dy * dy).sqrt();
                let coverage = if distance <= 1.0 {
                    255.0
                } else if distance < 1.0 + 1.0 / radius_x.min(radius_y) {
                    255.0 * (1.0 - (distance - 1.0) * radius_x.min(radius_y))
                } else {
                    0.0
                };
                mask.set_pixel(x as u32, y as u32, coverage.clamp(0.0, 255.0) as u8);
            }
        }
        Self { mask }
    }

    /// Wraps a mask that was already computed, for example by the magic wand kernel.
    #[must_use]
    pub fn from_mask(mask: GrayImage) -> Self {
        Self { mask }
    }

    #[must_use]
    pub fn mask(&self) -> &GrayImage {
        &self.mask
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.mask.width()
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.mask.height()
    }

    /// The half-open bounds of the selected area.
    #[must_use]
    pub fn bounds(&self) -> Option<RectI> {
        self.mask.bounds().map(|(x0, y0, x1, y1)| {
            RectI::new(i64::from(x0), i64::from(y0), i64::from(x1), i64::from(y1))
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mask.bounds().is_none()
    }

    #[must_use]
    pub fn coverage(&self) -> &[u8] {
        self.mask.as_slice()
    }

    #[must_use]
    pub fn coverage_at(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 {
            return 0;
        }
        self.mask.pixel(x as u32, y as u32)
    }

    pub fn invert(&mut self) {
        for value in self.mask.as_mut_slice() {
            *value = 255 - *value;
        }
    }

    /// Grows the selection by a box blur, which is what Feather does.
    pub fn feather(&mut self, radius: u32) {
        self.mask = self.mask.blurred(radius);
    }

    /// Joins another selection into this one.
    pub fn combine(&mut self, other: &Self, mode: SelectionMode) {
        if other.width() != self.width() || other.height() != self.height() {
            return;
        }
        for (target, source) in self
            .mask
            .as_mut_slice()
            .iter_mut()
            .zip(other.coverage().iter())
        {
            *target = match mode {
                SelectionMode::Replace => *source,
                SelectionMode::Add => (*target).max(*source),
                SelectionMode::Subtract => target.saturating_sub(*source),
                SelectionMode::Intersect => ((u32::from(*target) * u32::from(*source)) / 255) as u8,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_selection_covers_its_pixels() {
        let selection = Selection::rectangle(8, 8, RectI::new(2, 3, 5, 6));
        assert_eq!(selection.bounds(), Some(RectI::new(2, 3, 5, 6)));
        assert_eq!(selection.coverage_at(2, 3), 255);
        assert_eq!(selection.coverage_at(1, 3), 0);
    }

    #[test]
    fn rectangle_is_clipped_to_the_canvas() {
        let selection = Selection::rectangle(4, 4, RectI::new(-5, -5, 2, 2));
        assert_eq!(selection.bounds(), Some(RectI::new(0, 0, 2, 2)));
    }

    #[test]
    fn ellipse_has_a_round_shape() {
        let selection = Selection::ellipse(16, 16, RectI::new(0, 0, 16, 16));
        assert_eq!(selection.coverage_at(8, 8), 255);
        assert_eq!(selection.coverage_at(0, 0), 0);
    }

    #[test]
    fn combining_selections_respects_the_mode() {
        let mut selection = Selection::rectangle(8, 8, RectI::new(0, 0, 4, 4));
        let other = Selection::rectangle(8, 8, RectI::new(2, 2, 6, 6));
        selection.combine(&other, SelectionMode::Add);
        assert_eq!(selection.coverage_at(5, 5), 255);
        assert_eq!(selection.bounds(), Some(RectI::new(0, 0, 6, 6)));
        let mut intersection = Selection::rectangle(8, 8, RectI::new(0, 0, 4, 4));
        intersection.combine(&other, SelectionMode::Intersect);
        assert_eq!(intersection.bounds(), Some(RectI::new(2, 2, 4, 4)));
        let mut difference = Selection::rectangle(8, 8, RectI::new(0, 0, 4, 4));
        difference.combine(&other, SelectionMode::Subtract);
        assert_eq!(difference.coverage_at(3, 3), 0);
        assert_eq!(difference.coverage_at(1, 1), 255);
    }

    #[test]
    fn invert_turns_the_selection_inside_out() {
        let mut selection = Selection::rectangle(4, 4, RectI::new(0, 0, 2, 4));
        selection.invert();
        assert_eq!(selection.coverage_at(0, 0), 0);
        assert_eq!(selection.coverage_at(3, 3), 255);
    }

    #[test]
    fn feather_softens_the_edge() {
        let mut selection = Selection::rectangle(16, 16, RectI::new(0, 0, 8, 16));
        selection.feather(2);
        let edge = selection.coverage_at(8, 8);
        assert!(edge > 0 && edge < 255, "edge should be partial: {edge}");
    }
}
