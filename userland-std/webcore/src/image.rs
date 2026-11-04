//! Image decoding (PNG, JPEG, GIF's first frame, BMP) to RGBA, with
//! large images scaled down.

pub struct Image {
    pub w: u32,
    pub h: u32,
    /// RGBA, rows from the top.
    pub rgba: Vec<u8>,
}

const MAX: u32 = 2048;

pub fn decode(data: &[u8]) -> Option<Image> {
    let img = if data.starts_with(b"\x89PNG") {
        png(data)
    } else if data.starts_with(&[0xff, 0xd8]) {
        jpeg(data)
    } else if data.starts_with(b"GIF8") {
        gif(data)
    } else if data.starts_with(b"BM") {
        bmp(data)
    } else {
        None
    }?;
    Some(shrink(img))
}

fn shrink(img: Image) -> Image {
    if img.w <= MAX && img.h <= MAX {
        return img;
    }
    let f = (img.w.max(img.h) as f32 / MAX as f32).ceil() as u32;
    let (w, h) = ((img.w / f).max(1), (img.h / f).max(1));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let s = (((y * f) * img.w + x * f) * 4) as usize;
            let d = ((y * w + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
        }
    }
    Image { w, h, rgba: out }
}

fn png(data: &[u8]) -> Option<Image> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let px = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(Image { w, h, rgba })
}

fn jpeg(data: &[u8]) -> Option<Image> {
    let mut d = jpeg_decoder::Decoder::new(data);
    let pixels = d.decode().ok()?;
    let info = d.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    let rgba = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels.chunks(2).flat_map(|c| [c[0], c[0], c[0], 255]).collect(),
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks(4)
            .flat_map(|c| {
                let k = 255 - c[3] as u32;
                let f = |v: u8| ((255 - v as u32) * k / 255) as u8;
                [f(c[0]), f(c[1]), f(c[2]), 255]
            })
            .collect(),
    };
    Some(Image { w, h, rgba })
}

fn gif(data: &[u8]) -> Option<Image> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut d = opts.read_info(std::io::Cursor::new(data)).ok()?;
    let (w, h) = (d.width() as u32, d.height() as u32);
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    let f = d.read_next_frame().ok()??;
    for y in 0..f.height as u32 {
        for x in 0..f.width as u32 {
            let (cx, cy) = (x + f.left as u32, y + f.top as u32);
            if cx >= w || cy >= h {
                continue;
            }
            let s = ((y * f.width as u32 + x) * 4) as usize;
            let d = ((cy * w + cx) * 4) as usize;
            if f.buffer[s + 3] != 0 {
                canvas[d..d + 4].copy_from_slice(&f.buffer[s..s + 4]);
            }
        }
    }
    Some(Image { w, h, rgba: canvas })
}

fn bmp(data: &[u8]) -> Option<Image> {
    let u32at = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(data.get(o..o + 4)?.try_into().ok()?)) };
    let off = u32at(10)?;
    let w = u32at(18)? as i32;
    let h = u32at(22)? as i32;
    let bpp = u16::from_le_bytes(data.get(28..30)?.try_into().ok()?);
    if bpp != 24 && bpp != 32 {
        return None;
    }
    let (w, flip) = (w.unsigned_abs(), h > 0);
    let h = h.unsigned_abs();
    let bytes = (bpp / 8) as u32;
    let stride = (w * bytes).div_ceil(4) * 4;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = if flip { h - 1 - y } else { y };
        for x in 0..w {
            let s = (off + sy * stride + x * bytes) as usize;
            let p = data.get(s..s + 3)?;
            let d = ((y * w + x) * 4) as usize;
            rgba[d..d + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    Some(Image { w, h, rgba })
}
