//! Windows BMP decoding to RGBA. Re-Volt uses pure black as the transparent
//! colour key, so black texels get alpha 0.

#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Top-down RGBA8.
    pub rgba: Vec<u8>,
}

pub fn decode(data: &[u8]) -> Result<Image, String> {
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if data.get(0..2) != Some(b"BM") {
        return Err("not a BMP".into());
    }
    let bad = || "truncated BMP header".to_string();
    let pixel_offset = u32_at(10).ok_or_else(bad)? as usize;
    let header_size = u32_at(14).ok_or_else(bad)? as usize;
    let width = u32_at(18).ok_or_else(bad)? as i32;
    let raw_height = u32_at(22).ok_or_else(bad)? as i32;
    let bpp = u16_at(28).ok_or_else(bad)?;
    let compression = u32_at(30).ok_or_else(bad)?;
    if compression != 0 {
        return Err(format!("compressed BMP ({compression}) not supported"));
    }
    if width <= 0 || raw_height == 0 || width > 4096 || raw_height.abs() > 4096 {
        return Err(format!("bad BMP size {width}x{raw_height}"));
    }
    let (w, h) = (width as usize, raw_height.unsigned_abs() as usize);
    let bottom_up = raw_height > 0;
    let row_bytes = (w * bpp as usize).div_ceil(32) * 4;

    let palette: Vec<[u8; 3]> = if bpp <= 8 {
        let colors = match u32_at(46).ok_or_else(bad)? {
            0 => 1usize << bpp,
            n => n as usize,
        };
        let start = 14 + header_size;
        (0..colors)
            .map(|i| {
                let p = data.get(start + i * 4..start + i * 4 + 3).unwrap_or(&[0, 0, 0]);
                [p[2], p[1], p[0]]
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_row = if bottom_up { h - 1 - y } else { y };
        let start = pixel_offset + src_row * row_bytes;
        let row = data.get(start..start + row_bytes).ok_or("truncated BMP pixel data")?;
        for x in 0..w {
            let rgb = match bpp {
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3]],
                32 => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4]],
                16 => {
                    let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]);
                    let c = |s: u16| (((v >> s) & 31) as u32 * 255 / 31) as u8;
                    [c(10), c(5), c(0)]
                }
                8 => palette.get(row[x] as usize).copied().unwrap_or([0; 3]),
                4 => palette.get(((row[x / 2] >> (4 - (x % 2) * 4)) & 15) as usize).copied().unwrap_or([0; 3]),
                _ => return Err(format!("unsupported BMP depth {bpp}")),
            };
            let a = if rgb == [0, 0, 0] { 0 } else { 255 };
            rgba[(y * w + x) * 4..][..4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
        }
    }
    Ok(Image { width: w as u32, height: h as u32, rgba })
}
