//! Style sheets: parsing (cssparser for tokens, Servo's `selectors` for
//! selectors), an index of rules by the id, class or element name their
//! rightmost compound selector needs, and a tokenizer for declaration
//! values.
//!
//! `@media` blocks apply when their query holds for a screen of the
//! viewport's width (`min-width`/`max-width` in px or em, `screen`, `all`,
//! not `print`); `@supports` blocks apply; `@import` is reported to the
//! caller; other at-rules are skipped.

use std::collections::HashMap;
use std::sync::Arc;

use cssparser::{Delimiter, ParseError, Parser, Token};
use selectors::parser::{Component, ParseRelative, Selector, SelectorList};

use crate::select::{Impl, Parser as SelParser};

#[derive(Clone, Debug)]
pub struct Decl {
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent,
    Author,
}

pub struct Rule {
    pub selector: Selector<Impl>,
    pub specificity: u32,
    pub decls: Arc<Vec<Decl>>,
    pub order: usize,
    pub origin: Origin,
}

/// Rules from any number of style sheets, indexed.
#[derive(Default)]
pub struct Rules {
    pub rules: Vec<Rule>,
    pub by_id: HashMap<String, Vec<usize>>,
    pub by_class: HashMap<String, Vec<usize>>,
    pub by_tag: HashMap<String, Vec<usize>>,
    pub universal: Vec<usize>,
}

/// The environment @media queries are evaluated in.
#[derive(Clone, Copy)]
pub struct Media {
    pub width: f32,
    pub height: f32,
}

impl Rules {
    /// Add a style sheet's rules; returns the URLs of its @imports.
    pub fn add_sheet(&mut self, css: &str, origin: Origin, media: Media) -> Vec<String> {
        let mut p = Parser::new(css);
        let mut imports = Vec::new();
        let mut out = Vec::new();
        parse_rule_list(&mut p, media, &mut out, &mut imports, 0);
        for (list, decls) in out {
            let decls = Arc::new(decls);
            for sel in list.slice().iter() {
                let order = self.rules.len();
                let rule = Rule { selector: sel.clone(), specificity: sel.specificity(), decls: decls.clone(), order, origin };
                self.index(order, &rule.selector);
                self.rules.push(rule);
            }
        }
        imports
    }

    fn index(&mut self, i: usize, sel: &Selector<Impl>) {
        let (mut id, mut class, mut tag) = (None, None, None);
        for c in sel.iter() {
            match c {
                Component::ID(s) => id = Some(s.0.clone()),
                Component::Class(s) => {
                    if class.is_none() {
                        class = Some(s.0.clone())
                    }
                }
                Component::LocalName(l) => tag = Some(l.lower_name.0.clone()),
                _ => {}
            }
        }
        if let Some(id) = id {
            self.by_id.entry(id).or_default().push(i);
        } else if let Some(c) = class {
            self.by_class.entry(c).or_default().push(i);
        } else if let Some(t) = tag {
            self.by_tag.entry(t).or_default().push(i);
        } else {
            self.universal.push(i);
        }
    }
}

type Parsed = Vec<(SelectorList<Impl>, Vec<Decl>)>;

fn parse_rule_list(p: &mut Parser, media: Media, out: &mut Parsed, imports: &mut Vec<String>, depth: u32) {
    loop {
        let state = p.state();
        let tok = match p.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match tok {
            Token::CDO | Token::CDC | Token::Semicolon => {}
            Token::AtKeyword(name) => {
                let name = name.to_ascii_lowercase();
                let prelude: Result<String, ParseError<()>> = p.parse_until_before(Delimiter::CurlyBracketBlock | Delimiter::Semicolon, |p| {
                    let start = p.position();
                    while p.next().is_ok() {}
                    Ok(p.slice_from(start).to_string())
                });
                let prelude = prelude.unwrap_or_default();
                match p.next() {
                    Ok(Token::CurlyBracketBlock) => {
                        let apply = match name.as_str() {
                            "media" => media_applies(&prelude, media),
                            "supports" | "layer" | "document" | "-moz-document" | "container" | "scope" | "starting-style" => true,
                            _ => false,
                        };
                        if apply && depth < 8 {
                            let _: Result<(), ParseError<()>> = p.parse_nested_block(|p| {
                                parse_rule_list(p, media, out, imports, depth + 1);
                                Ok(())
                            });
                        }
                    }
                    _ => {
                        if name == "import" {
                            if let Some(u) = import_url(&prelude) {
                                imports.push(u);
                            }
                        }
                    }
                }
            }
            _ => {
                p.reset(&state);
                let sel: Result<SelectorList<Impl>, _> =
                    p.parse_until_before(Delimiter::CurlyBracketBlock, |p| SelectorList::parse(&SelParser, p, ParseRelative::No));
                match p.next() {
                    Ok(Token::CurlyBracketBlock) => {
                        let decls: Result<Vec<Decl>, ParseError<()>> = p.parse_nested_block(|p| Ok(parse_decls(p)));
                        if let (Ok(sel), Ok(decls)) = (sel, decls) {
                            if !decls.is_empty() {
                                out.push((sel, decls));
                            }
                        }
                    }
                    _ => break,
                }
            }
        }
    }
}

fn import_url(prelude: &str) -> Option<String> {
    let mut p = Parser::new(prelude);
    match p.next().ok()?.clone() {
        Token::QuotedString(s) | Token::UnquotedUrl(s) => {
            // A media list after it: only screen ones.
            let rest = p.slice_from(p.position()).to_ascii_lowercase();
            if rest.contains("print") && !rest.contains("screen") {
                return None;
            }
            Some(s.to_string())
        }
        Token::Function(f) if f.eq_ignore_ascii_case("url") => {
            let r: Result<String, ParseError<()>> = p.parse_nested_block(|p| Ok(p.expect_string().map(|s| s.to_string()).unwrap_or_default()));
            r.ok()
        }
        _ => None,
    }
}

/// Does an @media query list hold for a screen of this size?
pub fn media_applies(q: &str, m: Media) -> bool {
    let q = q.to_ascii_lowercase();
    if q.trim().is_empty() {
        return true;
    }
    q.split(',').any(|part| {
        let part = part.trim();
        let (neg, part) = match part.strip_prefix("not ") {
            Some(r) => (true, r),
            None => (false, part),
        };
        let part = part.strip_prefix("only ").unwrap_or(part);
        let mut ok = true;
        for term in part.split(" and ") {
            let t = term.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(inner) = t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
                ok &= feature(inner, m);
            } else {
                ok &= matches!(t, "screen" | "all");
            }
        }
        ok != neg
    })
}

fn feature(f: &str, m: Media) -> bool {
    let Some((name, value)) = f.split_once(':') else {
        // (color), (hover)...
        return !matches!(f.trim(), "prefers-reduced-motion" | "forced-colors" | "inverted-colors");
    };
    let v = value.trim();
    let px = |v: &str| -> Option<f32> {
        if let Some(n) = v.strip_suffix("px") {
            n.trim().parse().ok()
        } else if let Some(n) = v.strip_suffix("rem").or(v.strip_suffix("em")) {
            n.trim().parse::<f32>().ok().map(|n| n * 16.0)
        } else {
            v.parse().ok()
        }
    };
    match name.trim() {
        "min-width" => px(v).is_some_and(|x| m.width >= x),
        "max-width" => px(v).is_some_and(|x| m.width <= x),
        "min-height" => px(v).is_some_and(|x| m.height >= x),
        "max-height" => px(v).is_some_and(|x| m.height <= x),
        "orientation" => (v == "landscape") == (m.width >= m.height),
        "prefers-color-scheme" => v == "light",
        "prefers-reduced-motion" => v == "no-preference",
        "hover" | "any-hover" => v == "hover",
        "pointer" | "any-pointer" => v == "fine",
        "min-resolution" | "-webkit-min-device-pixel-ratio" | "min--moz-device-pixel-ratio" => {
            v.trim_end_matches("dppx").trim_end_matches("dpi").parse::<f32>().is_ok_and(|x| x <= 1.0 || (v.ends_with("dpi") && x <= 96.0))
        }
        "max-resolution" | "-webkit-max-device-pixel-ratio" => true,
        _ => false,
    }
}

/// The declarations of a block (also a `style` attribute's text).
pub fn parse_decls(p: &mut Parser) -> Vec<Decl> {
    let mut out = Vec::new();
    loop {
        let name = match p.next() {
            Ok(Token::Ident(n)) => n.to_ascii_lowercase(),
            Ok(Token::Semicolon) => continue,
            Ok(_) => {
                let _: Result<(), ParseError<()>> = p.parse_until_after(Delimiter::Semicolon, |p| {
                    while p.next().is_ok() {}
                    Ok(())
                });
                continue;
            }
            Err(_) => break,
        };
        let value: Result<String, ParseError<()>> = p.parse_until_after(Delimiter::Semicolon, |p| {
            if p.expect_colon().is_err() {
                while p.next().is_ok() {}
                return Ok(String::new());
            }
            let start = p.position();
            while p.next_including_whitespace_and_comments().is_ok() {}
            Ok(p.slice_from(start).to_string())
        });
        let Ok(mut value) = value else { continue };
        let mut important = false;
        let t = value.trim_end();
        let lower = t.to_ascii_lowercase();
        if let Some(i) = lower.rfind('!') {
            if lower[i + 1..].trim() == "important" {
                important = true;
                value = t[..i].to_string();
            }
        }
        let value = value.trim().to_string();
        if !value.is_empty() {
            out.push(Decl { name, value, important });
        }
    }
    out
}

/// A `style` attribute.
pub fn parse_style_attr(s: &str) -> Vec<Decl> {
    let mut p = Parser::new(s);
    parse_decls(&mut p)
}

// ---- Declaration values ----

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Num(f32),
    Dim(f32, String),
    Pct(f32),
    Hash(String),
    Str(String),
    Url(String),
    Func(String, Vec<Tok>),
    Comma,
    Slash,
    Delim(char),
}

pub fn tokens(s: &str) -> Vec<Tok> {
    let mut p = Parser::new(s);
    toks(&mut p, 0)
}

fn toks(p: &mut Parser, depth: u32) -> Vec<Tok> {
    let mut out = Vec::new();
    loop {
        let t = match p.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        out.push(match t {
            Token::Ident(s) => Tok::Ident(s.to_ascii_lowercase()),
            Token::Number { value, .. } => Tok::Num(value),
            Token::Dimension { value, unit, .. } => Tok::Dim(value, unit.to_ascii_lowercase()),
            Token::Percentage { unit_value, .. } => Tok::Pct(unit_value * 100.0),
            Token::Hash(s) | Token::IDHash(s) => Tok::Hash(s.to_string()),
            Token::QuotedString(s) => Tok::Str(s.to_string()),
            Token::UnquotedUrl(s) => Tok::Url(s.to_string()),
            Token::Comma => Tok::Comma,
            Token::Delim('/') => Tok::Slash,
            Token::Delim(c) => Tok::Delim(c),
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                let inner: Result<Vec<Tok>, ParseError<()>> = p.parse_nested_block(|p| Ok(if depth < 16 { toks(p, depth + 1) } else { Vec::new() }));
                let inner = inner.unwrap_or_default();
                if name == "url" {
                    match inner.first() {
                        Some(Tok::Str(s)) => Tok::Url(s.clone()),
                        _ => Tok::Func(name, inner),
                    }
                } else {
                    Tok::Func(name, inner)
                }
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let _: Result<(), ParseError<()>> = p.parse_nested_block(|p| {
                    while p.next().is_ok() {}
                    Ok(())
                });
                continue;
            }
            _ => continue,
        });
    }
    out
}

// ---- Colours ----

/// 0xAARRGGBB.
pub type Color = u32;
pub const TRANSPARENT: Color = 0;
pub const BLACK: Color = 0xff00_0000;

pub fn parse_color(t: &[Tok], current: Color) -> Option<Color> {
    match t.first()? {
        Tok::Hash(h) => hex_color(h),
        Tok::Ident(n) => match n.as_str() {
            "transparent" => Some(TRANSPARENT),
            "currentcolor" => Some(current),
            _ => named_color(n).map(|c| 0xff00_0000 | c),
        },
        Tok::Func(f, args) => {
            let nums: Vec<f32> = args
                .iter()
                .filter_map(|a| match a {
                    Tok::Num(n) => Some(*n),
                    Tok::Pct(p) => Some(-*p - 1000.0),
                    Tok::Dim(n, u) if u == "deg" => Some(*n),
                    Tok::Ident(i) if i == "none" => Some(0.0),
                    _ => None,
                })
                .collect();
            let chan = |v: f32| -> u32 {
                if v <= -1000.0 { ((-v - 1000.0) * 2.55).round().clamp(0.0, 255.0) as u32 } else { v.round().clamp(0.0, 255.0) as u32 }
            };
            let alpha = |v: Option<&f32>| -> u32 {
                match v {
                    None => 255,
                    Some(&v) if v <= -1000.0 => ((-v - 1000.0) * 2.55).round().clamp(0.0, 255.0) as u32,
                    Some(&v) => (v * 255.0).round().clamp(0.0, 255.0) as u32,
                }
            };
            match f.as_str() {
                "rgb" | "rgba" if nums.len() >= 3 => Some(alpha(nums.get(3)) << 24 | chan(nums[0]) << 16 | chan(nums[1]) << 8 | chan(nums[2])),
                "hsl" | "hsla" if nums.len() >= 3 => {
                    let pct = |v: f32| if v <= -1000.0 { (-v - 1000.0) / 100.0 } else { v / 100.0 };
                    let (h, s, l) = (nums[0].rem_euclid(360.0) / 360.0, pct(nums[1]), pct(nums[2]));
                    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
                    let p = 2.0 * l - q;
                    let hue = |mut t: f32| {
                        t = t.rem_euclid(1.0);
                        if t < 1.0 / 6.0 {
                            p + (q - p) * 6.0 * t
                        } else if t < 0.5 {
                            q
                        } else if t < 2.0 / 3.0 {
                            p + (q - p) * (2.0 / 3.0 - t) * 6.0
                        } else {
                            p
                        }
                    };
                    let c = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
                    Some(alpha(nums.get(3)) << 24 | c(hue(h + 1.0 / 3.0)) << 16 | c(hue(h)) << 8 | c(hue(h - 1.0 / 3.0)))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn hex_color(h: &str) -> Option<Color> {
    let v = |s: &str| u32::from_str_radix(s, 16).ok();
    let d = |i: usize| v(&h[i..i + 1]).map(|x| x * 17);
    match h.len() {
        3 => Some(0xff00_0000 | d(0)? << 16 | d(1)? << 8 | d(2)?),
        4 => Some(d(3)? << 24 | d(0)? << 16 | d(1)? << 8 | d(2)?),
        6 => Some(0xff00_0000 | v(h)?),
        8 => {
            let x = v(h)?;
            Some((x & 0xff) << 24 | x >> 8)
        }
        _ => None,
    }
}

fn named_color(n: &str) -> Option<u32> {
    Some(match n {
        "black" => 0x000000,
        "silver" => 0xc0c0c0,
        "gray" | "grey" => 0x808080,
        "white" => 0xffffff,
        "maroon" => 0x800000,
        "red" => 0xff0000,
        "purple" => 0x800080,
        "fuchsia" | "magenta" => 0xff00ff,
        "green" => 0x008000,
        "lime" => 0x00ff00,
        "olive" => 0x808000,
        "yellow" => 0xffff00,
        "navy" => 0x000080,
        "blue" => 0x0000ff,
        "teal" => 0x008080,
        "aqua" | "cyan" => 0x00ffff,
        "orange" => 0xffa500,
        "aliceblue" => 0xf0f8ff,
        "antiquewhite" => 0xfaebd7,
        "aquamarine" => 0x7fffd4,
        "azure" => 0xf0ffff,
        "beige" => 0xf5f5dc,
        "bisque" => 0xffe4c4,
        "blanchedalmond" => 0xffebcd,
        "blueviolet" => 0x8a2be2,
        "brown" => 0xa52a2a,
        "burlywood" => 0xdeb887,
        "cadetblue" => 0x5f9ea0,
        "chartreuse" => 0x7fff00,
        "chocolate" => 0xd2691e,
        "coral" => 0xff7f50,
        "cornflowerblue" => 0x6495ed,
        "cornsilk" => 0xfff8dc,
        "crimson" => 0xdc143c,
        "darkblue" => 0x00008b,
        "darkcyan" => 0x008b8b,
        "darkgoldenrod" => 0xb8860b,
        "darkgray" | "darkgrey" => 0xa9a9a9,
        "darkgreen" => 0x006400,
        "darkkhaki" => 0xbdb76b,
        "darkmagenta" => 0x8b008b,
        "darkolivegreen" => 0x556b2f,
        "darkorange" => 0xff8c00,
        "darkorchid" => 0x9932cc,
        "darkred" => 0x8b0000,
        "darksalmon" => 0xe9967a,
        "darkseagreen" => 0x8fbc8f,
        "darkslateblue" => 0x483d8b,
        "darkslategray" | "darkslategrey" => 0x2f4f4f,
        "darkturquoise" => 0x00ced1,
        "darkviolet" => 0x9400d3,
        "deeppink" => 0xff1493,
        "deepskyblue" => 0x00bfff,
        "dimgray" | "dimgrey" => 0x696969,
        "dodgerblue" => 0x1e90ff,
        "firebrick" => 0xb22222,
        "floralwhite" => 0xfffaf0,
        "forestgreen" => 0x228b22,
        "gainsboro" => 0xdcdcdc,
        "ghostwhite" => 0xf8f8ff,
        "gold" => 0xffd700,
        "goldenrod" => 0xdaa520,
        "greenyellow" => 0xadff2f,
        "honeydew" => 0xf0fff0,
        "hotpink" => 0xff69b4,
        "indianred" => 0xcd5c5c,
        "indigo" => 0x4b0082,
        "ivory" => 0xfffff0,
        "khaki" => 0xf0e68c,
        "lavender" => 0xe6e6fa,
        "lavenderblush" => 0xfff0f5,
        "lawngreen" => 0x7cfc00,
        "lemonchiffon" => 0xfffacd,
        "lightblue" => 0xadd8e6,
        "lightcoral" => 0xf08080,
        "lightcyan" => 0xe0ffff,
        "lightgoldenrodyellow" => 0xfafad2,
        "lightgray" | "lightgrey" => 0xd3d3d3,
        "lightgreen" => 0x90ee90,
        "lightpink" => 0xffb6c1,
        "lightsalmon" => 0xffa07a,
        "lightseagreen" => 0x20b2aa,
        "lightskyblue" => 0x87cefa,
        "lightslategray" | "lightslategrey" => 0x778899,
        "lightsteelblue" => 0xb0c4de,
        "lightyellow" => 0xffffe0,
        "limegreen" => 0x32cd32,
        "linen" => 0xfaf0e6,
        "mediumaquamarine" => 0x66cdaa,
        "mediumblue" => 0x0000cd,
        "mediumorchid" => 0xba55d3,
        "mediumpurple" => 0x9370db,
        "mediumseagreen" => 0x3cb371,
        "mediumslateblue" => 0x7b68ee,
        "mediumspringgreen" => 0x00fa9a,
        "mediumturquoise" => 0x48d1cc,
        "mediumvioletred" => 0xc71585,
        "midnightblue" => 0x191970,
        "mintcream" => 0xf5fffa,
        "mistyrose" => 0xffe4e1,
        "moccasin" => 0xffe4b5,
        "navajowhite" => 0xffdead,
        "oldlace" => 0xfdf5e6,
        "olivedrab" => 0x6b8e23,
        "orangered" => 0xff4500,
        "orchid" => 0xda70d6,
        "palegoldenrod" => 0xeee8aa,
        "palegreen" => 0x98fb98,
        "paleturquoise" => 0xafeeee,
        "palevioletred" => 0xdb7093,
        "papayawhip" => 0xffefd5,
        "peachpuff" => 0xffdab9,
        "peru" => 0xcd853f,
        "pink" => 0xffc0cb,
        "plum" => 0xdda0dd,
        "powderblue" => 0xb0e0e6,
        "rebeccapurple" => 0x663399,
        "rosybrown" => 0xbc8f8f,
        "royalblue" => 0x4169e1,
        "saddlebrown" => 0x8b4513,
        "salmon" => 0xfa8072,
        "sandybrown" => 0xf4a460,
        "seagreen" => 0x2e8b57,
        "seashell" => 0xfff5ee,
        "sienna" => 0xa0522d,
        "skyblue" => 0x87ceeb,
        "slateblue" => 0x6a5acd,
        "slategray" | "slategrey" => 0x708090,
        "snow" => 0xfffafa,
        "springgreen" => 0x00ff7f,
        "steelblue" => 0x4682b4,
        "tan" => 0xd2b48c,
        "thistle" => 0xd8bfd8,
        "tomato" => 0xff6347,
        "turquoise" => 0x40e0d0,
        "violet" => 0xee82ee,
        "wheat" => 0xf5deb3,
        "whitesmoke" => 0xf5f5f5,
        "yellowgreen" => 0x9acd32,
        // CSS system colours, as a light theme.
        "canvas" | "window" | "field" => 0xffffff,
        "canvastext" | "windowtext" | "fieldtext" | "buttontext" => 0x000000,
        "buttonface" | "buttonborder" => 0xdddddd,
        "linktext" => 0x0000ee,
        "graytext" => 0x808080,
        "highlight" => 0x3366cc,
        "highlighttext" => 0xffffff,
        _ => return None,
    })
}
