//! webshot: render a page with webcore on the host, to a PNG.
//!
//! ```text
//! cargo run -p webshot --target aarch64-apple-darwin -- URL OUT.png [WIDTH] [HEIGHT]
//! ```
//!
//! Text is drawn with fontdue from dist/fonts (the Liberation faces rs2's
//! GPI uses for Helvetica, Times New Roman and Courier), so line breaks
//! come out close to what WEB.EXE shows.

use std::collections::HashMap;

use webcore::layout::{self, FontKey, Fonts, Item};
use webcore::style::{Family, ListStyle};

struct HostFonts {
    faces: HashMap<(Family, bool, bool), fontdue::Font>,
}

impl HostFonts {
    fn new(dir: &str) -> HostFonts {
        let mut faces = HashMap::new();
        for (fam, b, i, file) in [
            (Family::Sans, false, false, "LIBSANS.TTF"),
            (Family::Sans, true, false, "LIBSANSB.TTF"),
            (Family::Sans, false, true, "LIBSANSI.TTF"),
            (Family::Sans, true, true, "LIBSANBI.TTF"),
            (Family::Serif, false, false, "LIBSERIF.TTF"),
            (Family::Serif, true, false, "LIBSERFB.TTF"),
            (Family::Serif, false, true, "LIBSERFI.TTF"),
            (Family::Serif, true, true, "LIBSRFBI.TTF"),
            (Family::Mono, false, false, "LIBMONO.TTF"),
            (Family::Mono, true, false, "LIBMONOB.TTF"),
            (Family::Mono, false, true, "LIBMONOI.TTF"),
            (Family::Mono, true, true, "LIBMONBI.TTF"),
        ] {
            if let Ok(data) = std::fs::read(format!("{dir}/{file}")) {
                if let Ok(f) = fontdue::Font::from_bytes(data, fontdue::FontSettings::default()) {
                    faces.insert((fam, b, i), f);
                }
            }
        }
        HostFonts { faces }
    }
    fn face(&self, f: FontKey) -> &fontdue::Font {
        self.faces.get(&(f.family, f.bold, f.italic)).or(self.faces.get(&(f.family, false, false))).or(self.faces.values().next()).expect("fonts")
    }
}

impl Fonts for HostFonts {
    fn width(&mut self, f: FontKey, text: &str) -> f32 {
        let face = self.face(f);
        text.chars().map(|c| face.metrics(c, f.size as f32).advance_width).sum()
    }
    fn metrics(&mut self, f: FontKey) -> (f32, f32) {
        let m = self.face(f).horizontal_line_metrics(f.size as f32).unwrap();
        (m.ascent.round(), (-m.descent).round())
    }
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u32>,
    clips: Vec<(f32, f32, f32, f32)>,
}

impl Canvas {
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32) {
        let a = (c >> 24) as u32;
        let (x0, y0) = (x.round().max(0.0) as usize, y.round().max(0.0) as usize);
        let (x1, y1) = (((x + w).round().max(0.0) as usize).min(self.w), ((y + h).round().max(0.0) as usize).min(self.h));
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.blend(xx, yy, c & 0xffffff, a);
            }
        }
    }
    fn blend(&mut self, x: usize, y: usize, c: u32, a: u32) {
        if x >= self.w || y >= self.h {
            return;
        }
        let (fx, fy) = (x as f32, y as f32);
        if self.clips.iter().any(|&(l, t, r, b)| fx < l || fx >= r || fy < t || fy >= b) {
            return;
        }
        let d = self.px[y * self.w + x];
        let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
        let r = mix(c >> 16 & 255, d >> 16 & 255);
        let g = mix(c >> 8 & 255, d >> 8 & 255);
        let b = mix(c & 255, d & 255);
        self.px[y * self.w + x] = r << 16 | g << 8 | b;
    }
}

fn text(c: &mut Canvas, fonts: &HostFonts, x: f32, y: f32, s: &str, font: FontKey, color: u32) {
    let face = fonts.face(font);
    let mut pen = x;
    for ch in s.chars() {
        let (m, bmp) = face.rasterize(ch, font.size as f32);
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = (y - m.height as f32 - m.ymin as f32).round() as i32;
        for yy in 0..m.height {
            for xx in 0..m.width {
                let a = bmp[yy * m.width + xx] as u32 * (color >> 24) / 255;
                if a > 0 && gx + (xx as i32) >= 0 && gy + (yy as i32) >= 0 {
                    c.blend((gx + xx as i32) as usize, (gy + yy as i32) as usize, color & 0xffffff, a);
                }
            }
        }
        pen += m.advance_width;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let url = webcore::parse_user_url(&args[1]).expect("url");
    let out = &args[2];
    let width: f32 = args.get(3).and_then(|w| w.parse().ok()).unwrap_or(800.0);
    let height: usize = args.get(4).and_then(|w| w.parse().ok()).unwrap_or(2000);
    let fontdir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dist/fonts");
    let mut fonts = HostFonts::new(fontdir);
    let t0 = std::time::Instant::now();
    let page = webcore::page::load(&url, (width, height as f32), &|s| eprintln!("{s}")).expect("load");
    eprintln!("loaded and styled in {:?}: {} nodes, {} rules ({} universal, {} ids, {} classes, {} tags)", t0.elapsed(), page.dom.nodes.len(), page.rules.rules.len(), page.rules.universal.len(), page.rules.by_id.len(), page.rules.by_class.len(), page.rules.by_tag.len());
    let mut page = page;
    let t = std::time::Instant::now();
    page.restyle((width, height as f32));
    eprintln!("restyle {:?}", t.elapsed());
    let mut sizes = layout::ImageSizes::new();
    let mut decoded = HashMap::new();
    for (n, u) in &page.images {
        if let Ok(r) = webcore::http::get(u) {
            if let Some(img) = webcore::image::decode(&r.body) {
                sizes.insert(*n, (img.w, img.h));
                decoded.insert(*n, img);
            }
        }
    }
    let t1 = std::time::Instant::now();
    let laid = layout::layout(&page.dom, &page.styles, &page.url, width, &mut fonts, &sizes);
    eprintln!("laid out in {:?}: {} items, height {}", t1.elapsed(), laid.items.len(), laid.height);
    let h = height.min(laid.height.ceil() as usize + 1).max(1);
    let mut c = Canvas { w: width as usize, h, px: vec![laid.background & 0xffffff; width as usize * h], clips: Vec::new() };
    for it in &laid.items {
        match it {
            Item::Fill { r, color } => c.fill(r.x, r.y, r.w, r.h, *color),
            Item::PushClip { r } => c.clips.push((r.x.round(), r.y.round(), (r.x + r.w).round(), (r.y + r.h).round())),
            Item::PopClip => {
                c.clips.pop();
            }
            Item::RoundRect { r, color, radii, ring } => layout::round_spans(*r, *radii, *ring, |x, y, w, h| c.fill(x, y, w, h, *color)),
            Item::Corners { r, color, radii } => layout::corner_spans(*r, *radii, |x, y, w, h| c.fill(x, y, w, h, *color)),
            Item::Svg { r, node, .. } => {
                let (w, h) = (r.w.round().max(1.0) as u32, r.h.round().max(1.0) as u32);
                let img = webcore::svg::render(&page.dom, &page.styles, *node, w, h);
                let (x0, y0) = (r.x.round() as i64, r.y.round() as i64);
                for yy in 0..h as i64 {
                    for xx in 0..w as i64 {
                        let s = ((yy * w as i64 + xx) * 4) as usize;
                        let p = &img.rgba[s..s + 4];
                        if p[3] != 0 && x0 + xx >= 0 && y0 + yy >= 0 {
                            c.blend((x0 + xx) as usize, (y0 + yy) as usize, (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32, p[3] as u32);
                        }
                    }
                }
            }
            Item::Marker { r, kind, color } => {
                if *kind == ListStyle::Circle {
                    c.fill(r.x, r.y, r.w, 1.0, *color);
                    c.fill(r.x, r.y + r.h - 1.0, r.w, 1.0, *color);
                    c.fill(r.x, r.y, 1.0, r.h, *color);
                    c.fill(r.x + r.w - 1.0, r.y, 1.0, r.h, *color);
                } else {
                    c.fill(r.x, r.y, r.w, r.h, *color);
                }
            }
            Item::Control { r, node, kind, font, color, pad } => {
                use webcore::layout::ControlKind as K;
                if matches!(kind, K::Checkbox | K::Radio) {
                    c.fill(r.x, r.y, r.w, r.h, 0xff808080);
                    c.fill(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 0xfff0f0f0);
                    continue;
                }
                if *kind == K::ButtonBox {
                    continue;
                }
                let e = page.dom.element(*node).unwrap();
                let (value, col) = match e.attr("value").filter(|v| !v.is_empty()) {
                    Some(v) => (v, *color),
                    None => (e.attr("placeholder").unwrap_or(""), 0xff808080),
                };
                let (a, d) = fonts.metrics(*font);
                let base = r.y + (r.h + a - d) / 2.0;
                let tw = fonts.width(*font, value);
                let x = if matches!(kind, K::Submit | K::Button) { r.x + (r.w - tw) / 2.0 } else { r.x + pad };
                text(&mut c, &fonts, x, base, value, *font, col);
            }
            Item::Image { r, node } => {
                if let Some(img) = decoded.get(node) {
                    for yy in 0..r.h as usize {
                        for xx in 0..r.w as usize {
                            let sx = (xx as f32 * img.w as f32 / r.w) as usize;
                            let sy = (yy as f32 * img.h as f32 / r.h) as usize;
                            let s = (sy * img.w as usize + sx) * 4;
                            if s + 3 < img.rgba.len() {
                                let p = &img.rgba[s..s + 4];
                                c.blend(r.x as usize + xx, r.y as usize + yy, (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32, p[3] as u32);
                            }
                        }
                    }
                }
            }
            Item::Text { x, y, text: text_s, font, color, decoration, w } => {
                text(&mut c, &fonts, *x, *y, text_s, *font, *color);
                if decoration & 1 != 0 {
                    c.fill(*x, y + 1.0, *w, 1.0, *color);
                }
                if decoration & 2 != 0 {
                    c.fill(*x, y - font.size as f32 * 0.3, *w, 1.0, *color);
                }
            }
        }
    }
    let file = std::fs::File::create(out).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), c.w as u32, c.h as u32);
    enc.set_color(png::ColorType::Rgb);
    let mut wr = enc.write_header().unwrap();
    let data: Vec<u8> = c.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    wr.write_image_data(&data).unwrap();
    eprintln!("{} links", laid.links.len());
}
