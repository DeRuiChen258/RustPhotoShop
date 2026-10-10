//! Photoshop (PSD and PSB) import.
//!
//! This reads the composite image Photoshop stores alongside its layers — what the file shows
//! when it is flattened — which covers the common "open a PSD and work on it" case. Layer
//! records are not turned into layers yet; the macOS build does that, and the conversion report
//! it shows is honest about which parts stay editable. See `docs/KNOWN-LIMITATIONS.md` in this
//! crate.
//!
//! The header, the section layout and both compression modes (raw and PackBits RLE) follow the
//! published Photoshop file format.

use std::path::Path;

use crate::core::pixel::PremulImage;

/// What went wrong reading a Photoshop file.
#[derive(Debug, thiserror::Error)]
pub enum PsdError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("这不是 Photoshop 文件（文件签名不是 8BPS）")]
    NotPsd,
    #[error("不支持的 Photoshop 版本 {0}（仅支持 PSD 与 PSB）")]
    UnknownVersion(u16),
    #[error("仅支持 8 位 RGB 或灰度文件；该文件为 {depth} 位、颜色模式 {color_mode}")]
    UnsupportedFormat { depth: u16, color_mode: u16 },
    #[error("不支持压缩方式 {0}（仅支持 raw 与 PackBits）")]
    UnsupportedCompression(u16),
    #[error("文件在 {0} 处意外结束")]
    Truncated(&'static str),
    #[error("文件的通道数据损坏")]
    BadData,
    #[error("图像为 {width}×{height}，超出支持范围")]
    TooLarge { width: u32, height: u32 },
}

/// Reads a Photoshop file's composite image.
///
/// # Errors
///
/// Fails when the file is not a PSD/PSB, is not 8-bit RGB, or is damaged.
pub fn load(path: &Path) -> Result<PremulImage, PsdError> {
    let bytes = std::fs::read(path)?;
    decode(&bytes)
}

/// True when the bytes start with the Photoshop signature.
#[must_use]
pub fn looks_like_psd(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..4] == b"8BPS"
}

/// Decodes a Photoshop file's composite image from memory.
///
/// # Errors
///
/// As [`load`].
pub fn decode(bytes: &[u8]) -> Result<PremulImage, PsdError> {
    let mut reader = Reader::new(bytes);
    let signature = reader.bytes(4)?;
    if signature != b"8BPS" {
        return Err(PsdError::NotPsd);
    }
    let version = reader.u16()?;
    if version != 1 && version != 2 {
        return Err(PsdError::UnknownVersion(version));
    }
    let psb = version == 2;
    reader.skip(6)?; // reserved
    let channels = reader.u16()?;
    let height = reader.u32()?;
    let width = reader.u32()?;
    let depth = reader.u16()?;
    let color_mode = reader.u16()?;
    if depth != 8 || (color_mode != 3 && color_mode != 1) {
        return Err(PsdError::UnsupportedFormat { depth, color_mode });
    }
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 {
        return Err(PsdError::TooLarge { width, height });
    }
    // Color mode data, image resources, then the layer and mask section: all skipped, because
    // the composite image below them already holds what this build opens.
    let color_mode_length = reader.u32()? as usize;
    reader.skip(color_mode_length)?;
    let resources_length = reader.u32()? as usize;
    reader.skip(resources_length)?;
    let layer_length = if psb {
        reader.u64()? as usize
    } else {
        reader.u32()? as usize
    };
    reader.skip(layer_length)?;

    let compression = reader.u16()?;
    let pixels = (width as usize) * (height as usize);
    let planes = channels.clamp(1, 4) as usize;
    let mut channel_data: Vec<Vec<u8>> = Vec::with_capacity(planes);
    match compression {
        0 => {
            for _ in 0..planes {
                channel_data.push(reader.bytes(pixels)?.to_vec());
            }
        }
        1 => {
            // A row-length table per channel, then the PackBits rows themselves.
            let row_bytes = if psb { 4 } else { 2 };
            let mut lengths: Vec<Vec<usize>> = Vec::with_capacity(planes);
            for _ in 0..planes {
                let mut rows = Vec::with_capacity(height as usize);
                for _ in 0..height {
                    let length = if psb {
                        reader.u32()? as usize
                    } else {
                        reader.u16()? as usize
                    };
                    rows.push(length);
                }
                lengths.push(rows);
            }
            let _ = row_bytes;
            for rows in lengths {
                let mut plane = Vec::with_capacity(pixels);
                for length in rows {
                    let compressed = reader.bytes(length)?;
                    let mut row = unpack_bits(compressed, width as usize);
                    plane.append(&mut row);
                }
                channel_data.push(plane);
            }
        }
        other => return Err(PsdError::UnsupportedCompression(other)),
    }

    // Photoshop stores each channel as its own plane; an alpha plane, when there is one, is the
    // fourth. Grayscale files have a single plane that fills all three channels.
    let mut straight = vec![0_u8; pixels * 4];
    for index in 0..pixels {
        let (red, green, blue, alpha) = if color_mode == 1 || planes == 1 {
            let gray = channel_data[0][index];
            (gray, gray, gray, 255)
        } else {
            let red = channel_data[0][index];
            let green = channel_data.get(1).map_or(red, |plane| plane[index]);
            let blue = channel_data.get(2).map_or(red, |plane| plane[index]);
            // Photoshop's alpha is inverted in the flattened composite: 0 is opaque.
            let alpha = channel_data.get(3).map_or(255, |plane| 255 - plane[index]);
            (red, green, blue, alpha)
        };
        straight[index * 4] = red;
        straight[index * 4 + 1] = green;
        straight[index * 4 + 2] = blue;
        straight[index * 4 + 3] = alpha;
    }
    PremulImage::from_straight(width, height, &straight).map_err(|_| PsdError::BadData)
}

/// PackBits, the run-length coding Photoshop uses for PSD rows.
fn unpack_bits(source: &[u8], expected: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected);
    let mut index = 0;
    while index < source.len() && out.len() < expected {
        let control = source[index] as i8;
        index += 1;
        if control >= 0 {
            let count = control as usize + 1;
            for _ in 0..count {
                if index >= source.len() || out.len() >= expected {
                    break;
                }
                out.push(source[index]);
                index += 1;
            }
        } else if control != -128 {
            let count = 1 - control as isize;
            if index >= source.len() {
                break;
            }
            let value = source[index];
            index += 1;
            for _ in 0..count {
                if out.len() >= expected {
                    break;
                }
                out.push(value);
            }
        }
    }
    out.resize(expected, 0);
    out
}

/// A bounds-checked big-endian reader.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8], PsdError> {
        if self.position + count > self.bytes.len() {
            return Err(PsdError::Truncated("a data block"));
        }
        let slice = &self.bytes[self.position..self.position + count];
        self.position += count;
        Ok(slice)
    }

    fn skip(&mut self, count: usize) -> Result<(), PsdError> {
        self.bytes(count).map(|_| ())
    }

    fn u16(&mut self) -> Result<u16, PsdError> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, PsdError> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, PsdError> {
        let high = u64::from(self.u32()?);
        let low = u64::from(self.u32()?);
        Ok((high << 32) | low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the smallest possible PSD: header, empty sections, one image data block.
    fn build_psd(
        width: u32,
        height: u32,
        channels: u16,
        compression: u16,
        planes: &[Vec<u8>],
        psb: bool,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"8BPS");
        bytes.extend_from_slice(&(if psb { 2_u16 } else { 1_u16 }).to_be_bytes());
        bytes.extend_from_slice(&[0_u8; 6]);
        bytes.extend_from_slice(&channels.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&8_u16.to_be_bytes());
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes()); // color mode data
        bytes.extend_from_slice(&0_u32.to_be_bytes()); // image resources
        if psb {
            bytes.extend_from_slice(&0_u64.to_be_bytes()); // layer and mask info
        } else {
            bytes.extend_from_slice(&0_u32.to_be_bytes());
        }
        bytes.extend_from_slice(&compression.to_be_bytes());
        match compression {
            0 => {
                for plane in planes {
                    bytes.extend_from_slice(plane);
                }
            }
            1 => {
                // One PackBits row per channel row.
                let mut rows: Vec<Vec<u8>> = Vec::new();
                for plane in planes {
                    for row in plane.chunks(width as usize) {
                        rows.push(pack_bits_row(row));
                    }
                }
                // Length tables first: all rows of channel 0, then channel 1, and so on.
                let mut lengths: Vec<Vec<u16>> = Vec::new();
                for plane in planes {
                    let mut channel_lengths = Vec::new();
                    for row in plane.chunks(width as usize) {
                        channel_lengths.push(pack_bits_row(row).len() as u16);
                    }
                    lengths.push(channel_lengths);
                }
                for channel_lengths in &lengths {
                    for length in channel_lengths {
                        if psb {
                            bytes.extend_from_slice(&u32::from(*length).to_be_bytes());
                        } else {
                            bytes.extend_from_slice(&length.to_be_bytes());
                        }
                    }
                }
                for row in rows {
                    bytes.extend_from_slice(&row);
                }
            }
            _ => {}
        }
        bytes
    }

    fn pack_bits_row(row: &[u8]) -> Vec<u8> {
        // Literal runs only, which is valid PackBits and enough for the tests.
        let mut out = vec![(row.len() as i8 - 1) as u8];
        out.extend_from_slice(row);
        out
    }

    #[test]
    fn reads_a_raw_rgb_file() {
        let red = vec![255, 0, 0, 0];
        let green = vec![0, 255, 0, 0];
        let blue = vec![0, 0, 255, 255];
        let bytes = build_psd(2, 2, 3, 0, &[red, green, blue], false);
        let image = decode(&bytes).expect("decode");
        assert_eq!((image.width(), image.height()), (2, 2));
        assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(image.pixel(1, 0), Some([0, 255, 0, 255]));
        assert_eq!(image.pixel(1, 1), Some([0, 0, 255, 255]));
    }

    #[test]
    fn reads_a_packbits_rgb_file() {
        let red = vec![10, 20, 30, 40];
        let green = vec![1, 2, 3, 4];
        let blue = vec![5, 6, 7, 8];
        let bytes = build_psd(2, 2, 3, 1, &[red, green, blue], false);
        let image = decode(&bytes).expect("decode");
        assert_eq!(image.pixel(0, 0), Some([10, 1, 5, 255]));
        assert_eq!(image.pixel(1, 1), Some([40, 4, 8, 255]));
    }

    #[test]
    fn reads_an_alpha_channel_in_the_photoshop_direction() {
        let red = vec![255, 255];
        let green = vec![0, 0];
        let blue = vec![0, 0];
        // Photoshop's flattened alpha is inverted: 0 means opaque, 255 means clear.
        let alpha = vec![0, 255];
        let bytes = build_psd(2, 1, 4, 0, &[red, green, blue, alpha], false);
        let image = decode(&bytes).expect("decode");
        assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(image.pixel(1, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn reads_a_psb_layer_section_length() {
        let red = vec![1, 2, 3, 4];
        let green = vec![5, 6, 7, 8];
        let blue = vec![9, 10, 11, 12];
        let bytes = build_psd(2, 2, 3, 1, &[red, green, blue], true);
        let image = decode(&bytes).expect("decode");
        assert_eq!(image.pixel(1, 1), Some([4, 8, 12, 255]));
    }

    #[test]
    fn other_files_are_refused_with_a_clear_message() {
        let error = decode(b"not a photoshop file at all").unwrap_err();
        assert!(matches!(error, PsdError::NotPsd));
        assert!(error.to_string().contains("8BPS"));
        assert!(!looks_like_psd(b"\x89PNG"));
        assert!(looks_like_psd(b"8BPS"));
    }

    #[test]
    fn a_truncated_file_is_refused_rather_than_guessed() {
        let mut bytes = build_psd(2, 2, 3, 0, &[vec![0; 4], vec![0; 4], vec![0; 4]], false);
        bytes.truncate(bytes.len() - 2);
        assert!(decode(&bytes).is_err());
    }
}
