//! Pixel buffers.
//!
//! Layer images and the composited canvas are stored **premultiplied** 8-bit sRGB RGBA — the
//! same representation the upstream C kernels work in, so a buffer can be handed to a kernel
//! without a conversion pass. Files on disk are straight (non-premultiplied), so loading and
//! saving convert in one place each (see [`PremulImage::from_straight`] and
//! [`PremulImage::to_straight`]).

use std::fmt;

/// One straight or premultiplied RGBA pixel.
pub type Rgba = [u8; 4];

/// An 8-bit RGBA image with premultiplied alpha, rows top-down, 4 bytes per pixel.
#[derive(Clone, PartialEq, Eq)]
pub struct PremulImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl fmt::Debug for PremulImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PremulImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.data.len())
            .finish()
    }
}

/// Colors outside `0..=255` channels or a byte count that does not match the size.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PixelError {
    #[error("图像为 {width}×{height}，但收到 {actual} 字节（应为 {expected} 字节）")]
    BadLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
    #[error("图像尺寸 {width}×{height} 超出范围")]
    BadSize { width: u32, height: u32 },
}

impl PremulImage {
    /// A fully transparent image. Panics only on sizes whose byte count overflows `usize`.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; byte_len(width, height)],
        }
    }

    /// A fully opaque image in one straight sRGB color.
    #[must_use]
    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let mut image = Self::new(width, height);
        image.fill(color);
        image
    }

    /// Wraps premultiplied bytes that are already in the right layout.
    ///
    /// # Errors
    ///
    /// Returns [`PixelError::BadLength`] when `data` is not exactly `width * height * 4` bytes.
    pub fn from_premultiplied(width: u32, height: u32, data: Vec<u8>) -> Result<Self, PixelError> {
        let expected = byte_len(width, height);
        if data.len() != expected {
            return Err(PixelError::BadLength {
                width,
                height,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Converts straight (non-premultiplied) RGBA bytes, as they appear in a PNG.
    ///
    /// # Errors
    ///
    /// Returns [`PixelError::BadLength`] when `rgba` is not exactly `width * height * 4` bytes.
    pub fn from_straight(width: u32, height: u32, rgba: &[u8]) -> Result<Self, PixelError> {
        let expected = byte_len(width, height);
        if rgba.len() != expected {
            return Err(PixelError::BadLength {
                width,
                height,
                expected,
                actual: rgba.len(),
            });
        }
        let mut data = rgba.to_vec();
        premultiply_in_place(&mut data);
        Ok(Self {
            width,
            height,
            data,
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Bytes per row.
    #[must_use]
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }

    /// The straight (non-premultiplied) bytes, ready to be written to a PNG.
    #[must_use]
    pub fn to_straight(&self) -> Vec<u8> {
        let mut data = self.data.clone();
        unpremultiply_in_place(&mut data);
        data
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        let index = self.index(x, y)?;
        Some([
            self.data[index],
            self.data[index + 1],
            self.data[index + 2],
            self.data[index + 3],
        ])
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: Rgba) {
        if let Some(index) = self.index(x, y) {
            self.data[index..index + 4].copy_from_slice(&pixel);
        }
    }

    /// Paints one pixel over whatever is there, with an extra opacity from 0 to 1.
    pub fn blend_pixel(&mut self, x: u32, y: u32, pixel: Rgba, opacity: f32) {
        let Some(index) = self.index(x, y) else {
            return;
        };
        let base = [
            self.data[index],
            self.data[index + 1],
            self.data[index + 2],
            self.data[index + 3],
        ];
        let out = over(base, pixel, opacity);
        self.data[index..index + 4].copy_from_slice(&out);
    }

    /// Fills the whole image with one premultiplied color (replacing, not blending).
    pub fn fill(&mut self, color: Rgba) {
        for chunk in self.data.chunks_exact_mut(4) {
            chunk.copy_from_slice(&color);
        }
    }

    /// Draws `source` at `(dx, dy)` with source-over compositing.
    pub fn blit(&mut self, source: &Self, dx: i64, dy: i64) {
        self.blit_scaled(source, dx, dy, 1.0);
    }

    /// Draws `source` at `(dx, dy)` with source-over compositing and an extra opacity.
    #[allow(clippy::needless_range_loop)] // four channels per pixel, addressed by number
    pub fn blit_scaled(&mut self, source: &Self, dx: i64, dy: i64, opacity: f32) {
        if opacity <= 0.0 {
            return;
        }
        // Clip the copy to both images, so only the overlapping rectangle is walked.
        let left = dx.max(0);
        let top = dy.max(0);
        let right = (dx + i64::from(source.width)).min(i64::from(self.width));
        let bottom = (dy + i64::from(source.height)).min(i64::from(self.height));
        if left >= right || top >= bottom {
            return;
        }
        for y in top..bottom {
            let source_y = (y - dy) as u32;
            let row = (y as usize) * self.stride();
            for x in left..right {
                let source_x = (x - dx) as u32;
                if let Some(source_pixel) = source.pixel(source_x, source_y) {
                    let index = row + (x as usize) * 4;
                    let base = [
                        self.data[index],
                        self.data[index + 1],
                        self.data[index + 2],
                        self.data[index + 3],
                    ];
                    let out = over(base, source_pixel, opacity);
                    self.data[index..index + 4].copy_from_slice(&out);
                }
            }
        }
    }

    /// A resized copy using bilinear sampling of the premultiplied pixels, which is the correct
    /// space for resampling anything with transparency.
    #[must_use]
    #[allow(clippy::needless_range_loop)] // four channels per pixel, addressed by number
    pub fn scale_to(&self, width: u32, height: u32) -> Self {
        if width == 0 || height == 0 {
            return Self::new(width.max(1), height.max(1));
        }
        if width == self.width && height == self.height {
            return self.clone();
        }
        if self.is_empty() {
            return Self::new(width, height);
        }
        let mut out = Self::new(width, height);
        let scale_x = self.width as f64 / width as f64;
        let scale_y = self.height as f64 / height as f64;
        for y in 0..height {
            let sy = ((y as f64 + 0.5) * scale_y - 0.5).clamp(0.0, self.height as f64 - 1.0);
            let y0 = sy.floor() as u32;
            let y1 = (y0 + 1).min(self.height - 1);
            let fy = (sy - f64::from(y0)) as f32;
            for x in 0..width {
                let sx = ((x as f64 + 0.5) * scale_x - 0.5).clamp(0.0, self.width as f64 - 1.0);
                let x0 = sx.floor() as u32;
                let x1 = (x0 + 1).min(self.width - 1);
                let fx = (sx - f64::from(x0)) as f32;
                let p00 = self.pixel(x0, y0).unwrap_or([0; 4]);
                let p10 = self.pixel(x1, y0).unwrap_or([0; 4]);
                let p01 = self.pixel(x0, y1).unwrap_or([0; 4]);
                let p11 = self.pixel(x1, y1).unwrap_or([0; 4]);
                let mut out_pixel = [0_u8; 4];
                for channel in 0..4 {
                    let top = lerp(f32::from(p00[channel]), f32::from(p10[channel]), fx);
                    let bottom = lerp(f32::from(p01[channel]), f32::from(p11[channel]), fx);
                    out_pixel[channel] = lerp(top, bottom, fy).round().clamp(0.0, 255.0) as u8;
                }
                out.set_pixel(x, y, out_pixel);
            }
        }
        out
    }

    /// The half-open pixel rectangle `(x, y, width, height)`, or `None` when it does not overlap.
    #[must_use]
    pub fn crop(&self, x: i64, y: i64, width: u32, height: u32) -> Option<Self> {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + i64::from(width)).min(i64::from(self.width));
        let y1 = (y + i64::from(height)).min(i64::from(self.height));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let out_width = (x1 - x0) as u32;
        let out_height = (y1 - y0) as u32;
        let mut out = Self::new(out_width, out_height);
        for row in 0..out_height {
            let source_start = ((y0 as u32 + row) as usize) * self.stride() + x0 as usize * 4;
            let target_start = row as usize * out.stride();
            let len = out_width as usize * 4;
            out.data[target_start..target_start + len]
                .copy_from_slice(&self.data[source_start..source_start + len]);
        }
        Some(out)
    }

    pub fn flip_horizontal(&mut self) {
        let stride = self.stride();
        for row in self.data.chunks_exact_mut(stride) {
            for x in 0..self.width as usize / 2 {
                let left = x * 4;
                let right = (self.width as usize - 1 - x) * 4;
                for channel in 0..4 {
                    row.swap(left + channel, right + channel);
                }
            }
        }
    }

    pub fn flip_vertical(&mut self) {
        let stride = self.stride();
        let rows = self.height as usize;
        for y in 0..rows / 2 {
            let (top, bottom) = self.data.split_at_mut((y + 1) * stride);
            let top_row = &mut top[y * stride..(y + 1) * stride];
            let bottom_row = &mut bottom[(rows - 2 - y) * stride..(rows - 1 - y) * stride];
            top_row.swap_with_slice(bottom_row);
        }
    }

    /// A copy turned a quarter turn clockwise, so width and height swap.
    #[must_use]
    pub fn rotate_90_cw(&self) -> Self {
        let mut out = Self::new(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(pixel) = self.pixel(x, y) {
                    out.set_pixel(self.height - 1 - y, x, pixel);
                }
            }
        }
        out
    }

    /// The half-open bounds of the pixels that are not fully transparent.
    #[must_use]
    pub fn alpha_bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let mut left = self.width;
        let mut right = 0;
        let mut top = self.height;
        let mut bottom = 0;
        let mut found = false;
        for y in 0..self.height {
            for x in 0..self.width {
                let index = self.index(x, y).expect("index inside image");
                if self.data[index + 3] != 0 {
                    found = true;
                    left = left.min(x);
                    right = right.max(x + 1);
                    top = top.min(y);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        found.then_some((left, top, right, bottom))
    }

    /// Replaces the image with `other` blended over it, weighted per pixel by `coverage`
    /// (`width * height` bytes, 255 = fully `other`). Used to limit an adjustment to a
    /// selection.
    #[allow(clippy::needless_range_loop)] // channels are addressed by number
    pub fn lerp_with_coverage(&mut self, other: &Self, coverage: &[u8]) {
        debug_assert_eq!(self.width, other.width);
        debug_assert_eq!(self.height, other.height);
        debug_assert_eq!(coverage.len(), (self.width * self.height) as usize);
        for (index, chunk) in self.data.chunks_exact_mut(4).enumerate() {
            let weight = coverage[index];
            if weight == 0 {
                continue;
            }
            let amount = f32::from(weight) / 255.0;
            for channel in 0..4 {
                let base = f32::from(chunk[channel]);
                let target = f32::from(other.data[index * 4 + channel]);
                chunk[channel] = lerp(base, target, amount).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Multiplies the alpha of every pixel by `coverage` (255 = unchanged).
    #[allow(clippy::needless_range_loop)] // channels are addressed by number
    pub fn multiply_coverage(&mut self, coverage: &[u8]) {
        debug_assert_eq!(coverage.len(), (self.width * self.height) as usize);
        for (index, chunk) in self.data.chunks_exact_mut(4).enumerate() {
            let weight = u32::from(coverage[index]);
            if weight == 255 {
                continue;
            }
            if weight == 0 {
                chunk.fill(0);
                continue;
            }
            for channel in 0..4 {
                chunk[channel] = ((u32::from(chunk[channel]) * weight + 127) / 255) as u8;
            }
        }
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height)
            .then_some((y as usize * self.width as usize + x as usize) * 4)
    }
}

/// An 8-bit grayscale image: layer masks and selections.
#[derive(Clone, PartialEq, Eq)]
pub struct GrayImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl fmt::Debug for GrayImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrayImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl GrayImage {
    #[must_use]
    pub fn new(width: u32, height: u32, value: u8) -> Self {
        Self {
            width,
            height,
            data: vec![value; (width as usize) * (height as usize)],
        }
    }

    /// # Errors
    ///
    /// Returns [`PixelError::BadLength`] when `data` is not `width * height` bytes.
    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<Self, PixelError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(PixelError::BadLength {
                width,
                height,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.data[y as usize * self.width as usize + x as usize]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, value: u8) {
        if x < self.width && y < self.height {
            self.data[y as usize * self.width as usize + x as usize] = value;
        }
    }

    /// The half-open bounds of non-zero pixels.
    #[must_use]
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let mut left = self.width;
        let mut right = 0;
        let mut top = self.height;
        let mut bottom = 0;
        let mut found = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.data[y as usize * self.width as usize + x as usize] != 0 {
                    found = true;
                    left = left.min(x);
                    right = right.max(x + 1);
                    top = top.min(y);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        found.then_some((left, top, right, bottom))
    }

    /// A box-blurred copy, the cheap feathering used for mask edges.
    #[must_use]
    pub fn blurred(&self, radius: u32) -> Self {
        if radius == 0 {
            return self.clone();
        }
        let radius = radius.min(256);
        let mut horizontal = Self::new(self.width, self.height, 0);
        let window = radius * 2 + 1;
        for y in 0..self.height {
            let mut sum: u32 = 0;
            for offset in 0..window {
                let x = offset as i64 - radius as i64;
                sum += u32::from(self.pixel(x.clamp(0, i64::from(self.width) - 1) as u32, y));
            }
            for x in 0..self.width {
                horizontal.set_pixel(x, y, (sum / window) as u8);
                let leaving = x as i64 - radius as i64;
                let entering = x as i64 + radius as i64 + 1;
                sum -= u32::from(self.pixel(leaving.clamp(0, i64::from(self.width) - 1) as u32, y));
                sum +=
                    u32::from(self.pixel(entering.clamp(0, i64::from(self.width) - 1) as u32, y));
            }
        }
        let mut out = Self::new(self.width, self.height, 0);
        for x in 0..self.width {
            let mut sum: u32 = 0;
            for offset in 0..window {
                let y = offset as i64 - radius as i64;
                sum +=
                    u32::from(horizontal.pixel(x, y.clamp(0, i64::from(self.height) - 1) as u32));
            }
            for y in 0..self.height {
                out.set_pixel(x, y, (sum / window) as u8);
                let leaving = y as i64 - radius as i64;
                let entering = y as i64 + radius as i64 + 1;
                sum -= u32::from(
                    horizontal.pixel(x, leaving.clamp(0, i64::from(self.height) - 1) as u32),
                );
                sum += u32::from(
                    horizontal.pixel(x, entering.clamp(0, i64::from(self.height) - 1) as u32),
                );
            }
        }
        out
    }
}

/// Source-over compositing in premultiplied space, with an extra source opacity.
#[must_use]
#[allow(clippy::needless_range_loop)] // four channels per pixel, addressed by number
pub fn over(base: Rgba, source: Rgba, opacity: f32) -> Rgba {
    if opacity >= 1.0 {
        let inverse = 255 - u32::from(source[3]);
        let mut out = [0_u8; 4];
        for channel in 0..4 {
            out[channel] = (u32::from(source[channel])
                + (u32::from(base[channel]) * inverse + 127) / 255)
                .min(255) as u8;
        }
        return out;
    }
    if opacity <= 0.0 {
        return base;
    }
    // Premultiplied source-over: `source * opacity + base * (1 - source_alpha * opacity)`.
    let effective_alpha = opacity * f32::from(source[3]) / 255.0;
    let keep = 1.0 - effective_alpha;
    let mut out = [0_u8; 4];
    for channel in 0..4 {
        out[channel] = (f32::from(source[channel]) * opacity + f32::from(base[channel]) * keep)
            .clamp(0.0, 255.0) as u8;
    }
    out
}

fn byte_len(width: u32, height: u32) -> usize {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .expect("image size overflows usize")
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

#[allow(clippy::needless_range_loop)] // channels are addressed by number
fn premultiply_in_place(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        let alpha = u32::from(chunk[3]);
        if alpha == 255 {
            continue;
        }
        for channel in 0..3 {
            chunk[channel] = ((u32::from(chunk[channel]) * alpha + 127) / 255) as u8;
        }
    }
}

#[allow(clippy::needless_range_loop)] // channels are addressed by number
fn unpremultiply_in_place(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        let alpha = u32::from(chunk[3]);
        if alpha == 255 {
            continue;
        }
        if alpha == 0 {
            chunk[..3].fill(0);
            continue;
        }
        for channel in 0..3 {
            let value = (u32::from(chunk[channel]) * 255 + alpha / 2) / alpha;
            chunk[channel] = value.min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_round_trip_keeps_colors() {
        let straight = vec![
            255, 128, 0, 128, //
            10, 20, 30, 255, //
            0, 0, 0, 0, //
            7, 8, 9, 255,
        ];
        let image = PremulImage::from_straight(2, 2, &straight).expect("size matches");
        assert_eq!(image.pixel(0, 0), Some([128, 64, 0, 128]));
        assert_eq!(image.to_straight(), straight);
    }

    #[test]
    fn blit_is_source_over() {
        let mut base = PremulImage::filled(2, 1, [255, 0, 0, 255]);
        let source = PremulImage::filled(1, 1, [0, 255, 0, 255]);
        base.blit(&source, 1, 0);
        assert_eq!(base.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(base.pixel(1, 0), Some([0, 255, 0, 255]));
    }

    #[test]
    fn blit_clips_at_the_edges() {
        let mut base = PremulImage::new(2, 2);
        let source = PremulImage::filled(2, 2, [10, 20, 30, 255]);
        base.blit(&source, -1, -1);
        assert_eq!(base.pixel(0, 0), Some([10, 20, 30, 255]));
        assert_eq!(base.pixel(1, 1), Some([0, 0, 0, 0]));
    }

    #[test]
    fn scale_keeps_transparency_premultiplied() {
        let mut image = PremulImage::new(2, 2);
        image.set_pixel(0, 0, [255, 255, 255, 255]);
        image.set_pixel(1, 0, [0, 0, 0, 0]);
        image.set_pixel(0, 1, [0, 0, 0, 0]);
        image.set_pixel(1, 1, [0, 0, 0, 0]);
        let scaled = image.scale_to(4, 4);
        assert_eq!(scaled.pixel(0, 0), Some([255, 255, 255, 255]));
        assert_eq!(scaled.pixel(3, 3), Some([0, 0, 0, 0]));
    }

    #[test]
    fn rotate_and_flip_are_inverse_operations() {
        let mut image = PremulImage::new(3, 2);
        image.set_pixel(0, 0, [1, 2, 3, 255]);
        let rotated = image.rotate_90_cw();
        assert_eq!(rotated.width(), 2);
        assert_eq!(rotated.height(), 3);
        assert_eq!(rotated.pixel(1, 0), Some([1, 2, 3, 255]));
        let mut twice = image.clone();
        twice.flip_horizontal();
        twice.flip_horizontal();
        assert_eq!(twice, image);
    }

    #[test]
    fn alpha_bounds_ignore_transparent_edges() {
        let mut image = PremulImage::new(4, 4);
        image.set_pixel(1, 2, [0, 0, 0, 10]);
        image.set_pixel(2, 2, [0, 0, 0, 255]);
        assert_eq!(image.alpha_bounds(), Some((1, 2, 3, 3)));
        assert_eq!(PremulImage::new(4, 4).alpha_bounds(), None);
    }

    #[test]
    fn coverage_limits_an_edit() {
        let mut base = PremulImage::filled(2, 1, [0, 0, 0, 255]);
        let other = PremulImage::filled(2, 1, [255, 255, 255, 255]);
        base.lerp_with_coverage(&other, &[255, 0]);
        assert_eq!(base.pixel(0, 0), Some([255, 255, 255, 255]));
        assert_eq!(base.pixel(1, 0), Some([0, 0, 0, 255]));
    }

    #[test]
    fn gray_bounds_and_blur() {
        let mut mask = GrayImage::new(4, 4, 0);
        mask.set_pixel(2, 2, 255);
        assert_eq!(mask.bounds(), Some((2, 2, 3, 3)));
        let blurred = mask.blurred(1);
        assert!(blurred.pixel(2, 2) > 0);
        assert_eq!(blurred.pixel(0, 0), 0);
    }
}
