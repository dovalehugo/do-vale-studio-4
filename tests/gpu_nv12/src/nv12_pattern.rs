//! Synthetic NV12 test pattern in YUV space (CPU writes Y/U/V only — no RGB conversion).

/// BT.709 limited-range YUV triple (8-bit studio swing).
#[derive(Clone, Copy, Debug)]
pub struct YuvPixel {
    pub y: u8,
    pub u: u8,
    pub v: u8,
}

impl YuvPixel {
    pub const BLACK: Self = Self { y: 16, u: 128, v: 128 };
    pub const WHITE: Self = Self { y: 235, u: 128, v: 128 };
    /// Reference limited-range primaries (not derived from RGB on CPU).
    pub const RED: Self = Self { y: 76, u: 84, v: 255 };
    pub const GREEN: Self = Self { y: 149, u: 44, v: 21 };
    pub const BLUE: Self = Self { y: 29, u: 255, v: 107 };
    pub const YELLOW: Self = Self { y: 210, u: 16, v: 146 };
    pub const CYAN: Self = Self { y: 170, u: 166, v: 16 };
    pub const MAGENTA: Self = Self { y: 105, u: 202, v: 222 };
    /// Approximate skin-tone patch (YUV constants).
    pub const SKIN: Self = Self { y: 180, u: 98, v: 118 };
}

pub struct Nv12Pattern {
    pub y_plane: Vec<u8>,
    pub uv_plane: Vec<u8>,
}

pub fn generate_test_pattern(width: u32, height: u32) -> Nv12Pattern {
    assert!(width % 2 == 0 && height % 2 == 0);

    let mut y_plane = vec![0u8; (width * height) as usize];
    let uv_width = width / 2;
    let uv_height = height / 2;
    let mut uv_plane = vec![0u8; (uv_width * uv_height * 2) as usize];

    // Layout (3 rows × 4 columns of macro-patches)
    let cols = 4u32;
    let rows = 3u32;
    let patch_w = width / cols;
    let patch_h = height / rows;

    let patches: [[YuvPixel; 4]; 3] = [
        [
            YuvPixel::WHITE,
            YuvPixel::YELLOW,
            YuvPixel::CYAN,
            YuvPixel::GREEN,
        ],
        [
            YuvPixel::MAGENTA,
            YuvPixel::RED,
            YuvPixel::BLUE,
            YuvPixel::SKIN,
        ],
        [
            YuvPixel::BLACK,
            YuvPixel {
                y: 64,
                u: 128,
                v: 128,
            },
            YuvPixel {
                y: 128,
                u: 128,
                v: 128,
            },
            YuvPixel {
                y: 192,
                u: 128,
                v: 128,
            },
        ],
    ];

    for row in 0..rows {
        for col in 0..cols {
            let y0 = row * patch_h;
            let x0 = col * patch_w;
            let pixel = patches[row as usize][col as usize];
            fill_patch_y(&mut y_plane, width, x0, y0, patch_w, patch_h, pixel);

            // Horizontal luma gradient overlay in bottom-right patch
            if row == 2 && col == 3 {
                for py in 0..patch_h {
                    for px in 0..patch_w {
                        let x = x0 + px;
                        let y = y0 + py;
                        let t = px as f32 / patch_w as f32;
                        let grad_y = (16.0 + t * (235.0 - 16.0)) as u8;
                        y_plane[(y * width + x) as usize] = grad_y;
                    }
                }
            }

            // Vertical chroma gradient in bottom-left (U sweep, V fixed)
            if row == 2 && col == 0 {
                for py in 0..patch_h {
                    for px in 0..patch_w {
                        let u = (16.0 + (px as f32 / patch_w as f32) * (240.0 - 16.0)) as u8;
                        write_uv(
                            &mut uv_plane,
                            uv_width,
                            x0 + px,
                            y0 + py,
                            u,
                            128,
                        );
                    }
                }
            } else {
                fill_patch_uv(
                    &mut uv_plane,
                    uv_width,
                    x0,
                    y0,
                    patch_w,
                    patch_h,
                    pixel,
                );
            }
        }
    }

    Nv12Pattern { y_plane, uv_plane }
}

fn fill_patch_y(
    y_plane: &mut [u8],
    width: u32,
    x0: u32,
    y0: u32,
    patch_w: u32,
    patch_h: u32,
    pixel: YuvPixel,
) {
    for py in 0..patch_h {
        for px in 0..patch_w {
            let x = x0 + px;
            let y = y0 + py;
            y_plane[(y * width + x) as usize] = pixel.y;
        }
    }
}

fn fill_patch_uv(
    uv_plane: &mut [u8],
    uv_width: u32,
    x0: u32,
    y0: u32,
    patch_w: u32,
    patch_h: u32,
    pixel: YuvPixel,
) {
    for py in 0..patch_h {
        for px in 0..patch_w {
            write_uv(uv_plane, uv_width, x0 + px, y0 + py, pixel.u, pixel.v);
        }
    }
}

fn write_uv(uv_plane: &mut [u8], uv_width: u32, x: u32, y: u32, u: u8, v: u8) {
    let ux = x / 2;
    let uy = y / 2;
    let idx = ((uy * uv_width + ux) * 2) as usize;
    if idx + 1 < uv_plane.len() {
        uv_plane[idx] = u;
        uv_plane[idx + 1] = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_has_expected_plane_sizes() {
        let p = generate_test_pattern(1920, 1080);
        assert_eq!(p.y_plane.len(), 1920 * 1080);
        assert_eq!(p.uv_plane.len(), 1920 * 1080 / 2);
    }
}
