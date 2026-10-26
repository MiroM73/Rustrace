//! The program icon, drawn in code: a blue rounded square with a white log page whose lines are
//! colored like the severities (normal, warning, error). Used for the window icon, the start-up
//! splash and (through `build.rs`, which includes this file) the icon resource of the executable.

/// Straight-alpha RGBA color.
type Rgba = [f32; 4];

const LINE: Rgba = [125.0, 135.0, 150.0, 255.0];
const WARN: Rgba = [230.0, 140.0, 0.0, 255.0];
const ERROR: Rgba = [220.0, 60.0, 60.0, 255.0];
/// Lines of the page: (y center, right end, color); all start at x = 0.31.
const LINES: [(f32, f32, Rgba); 5] = [
    (0.29, 0.69, LINE),
    (0.40, 0.60, LINE),
    (0.51, 0.69, WARN),
    (0.62, 0.55, LINE),
    (0.73, 0.65, ERROR),
];

/// Inside a rounded rectangle (x0, y0, x1, y1, corner radius); coordinates 0..1.
fn in_rounded(x: f32, y: f32, (x0, y0, x1, y1, r): (f32, f32, f32, f32, f32)) -> bool {
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// Color of one point of the icon (coordinates 0..1).
fn sample(x: f32, y: f32) -> Rgba {
    if !in_rounded(x, y, (0.03, 0.03, 0.97, 0.97, 0.2)) {
        return [0.0; 4];
    }
    for (ly, end, color) in LINES {
        if in_rounded(x, y, (0.31, ly - 0.035, end, ly + 0.035, 0.035)) {
            return color;
        }
    }
    if in_rounded(x, y, (0.21, 0.13, 0.79, 0.89, 0.06)) {
        return [255.0, 255.0, 255.0, 255.0];
    }
    // Background: vertical gradient from a lighter to a darker blue.
    let top = [45.0, 125.0, 215.0];
    let bottom = [22.0, 70.0, 150.0];
    let mix = |i: usize| top[i] + (bottom[i] - top[i]) * y;
    [mix(0), mix(1), mix(2), 255.0]
}

/// The icon as `size` × `size` RGBA pixels (straight alpha, rows top to bottom), anti-aliased
/// with 4 × 4 samples per pixel.
pub fn rgba(size: u32) -> Vec<u8> {
    const N: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            // Premultiplied sum of the samples.
            let mut acc = [0.0f32; 4];
            for sy in 0..N {
                for sx in 0..N {
                    let x = (px as f32 + (sx as f32 + 0.5) / N as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / N as f32) / size as f32;
                    let c = sample(x, y);
                    let a = c[3] / 255.0;
                    for i in 0..3 {
                        acc[i] += c[i] * a;
                    }
                    acc[3] += c[3];
                }
            }
            let n = (N * N) as f32;
            let a = acc[3] / n;
            for v in &acc[..3] {
                let c = if a > 0.0 { v / n / (a / 255.0) } else { 0.0 };
                out.push(c.round().clamp(0.0, 255.0) as u8);
            }
            out.push(a.round() as u8);
        }
    }
    out
}

/// A Windows `.ico` file with the icon in the given sizes (32-bit BMP images).
#[allow(dead_code)] // used by build.rs
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| ico_image(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&s, img) in sizes.iter().zip(&images) {
        // 256 is written as 0.
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

/// One `.ico` image: BITMAPINFOHEADER (double height for the mask), BGRA rows bottom to top and
/// an empty AND mask (the alpha channel is used).
#[allow(dead_code)]
fn ico_image(size: u32) -> Vec<u8> {
    let px = rgba(size);
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, size, size * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(size * size * 4 + mask_row * size).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[px[i + 2], px[i + 1], px[i], px[i + 3]]);
        }
    }
    out.resize(out.len() + (mask_row * size) as usize, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_pixels_and_ico_layout() {
        let px = rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        // Transparent corner, opaque middle (white page).
        assert_eq!(px[3], 0);
        let mid = ((16 * 32 + 8) * 4) as usize;
        assert_eq!(&px[mid..mid + 4], &[255, 255, 255, 255]);

        let ico = ico(&[16, 256]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        // Second entry: 256 written as 0, offset after the first image.
        assert_eq!(ico[6 + 16], 0);
        let first_len = u32::from_le_bytes(ico[14..18].try_into().unwrap());
        assert_eq!(first_len, 40 + 16 * 16 * 4 + 4 * 16);
        let second_off = u32::from_le_bytes(ico[34..38].try_into().unwrap());
        assert_eq!(second_off, 6 + 32 + first_len);
        assert_eq!(ico.len() as u32, second_off + 40 + 256 * 256 * 4 + 32 * 256);
    }
}
