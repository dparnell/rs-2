//! Host tests of the engine (`cargo test -p webcore --target
//! aarch64-apple-darwin` from userland-std): styles, layout with a fake
//! font of fixed-width characters.

use crate::css::{self, Media, Origin, Rules};
use crate::dom;
use crate::layout::{self, FontKey, Fonts, Item};
use crate::style;
use url::Url;

/// Every character is half the font size wide; ascender 0.8, descender 0.2.
struct Fixed;

impl Fonts for Fixed {
    fn width(&mut self, f: FontKey, text: &str) -> f32 {
        text.chars().count() as f32 * f.size as f32 / 2.0
    }
    fn metrics(&mut self, f: FontKey) -> (f32, f32) {
        (f.size as f32 * 0.8, f.size as f32 * 0.2)
    }
}

fn styled(html: &str, width: f32) -> (dom::Dom, style::Styles) {
    let d = dom::parse(html);
    let media = Media { width, height: 600.0 };
    let mut rules = Rules::default();
    rules.add_sheet(style::UA_CSS, Origin::UserAgent, media);
    for n in d.descendants(dom::Dom::DOCUMENT) {
        if d.element(n).is_some_and(|e| e.is("style")) {
            rules.add_sheet(&d.text_content(n), Origin::Author, media);
        }
    }
    let s = style::compute(&d, &rules, (width, 600.0));
    (d, s)
}

fn style_of<'a>(d: &dom::Dom, s: &'a style::Styles, id: &str) -> &'a style::Style {
    let n = d.descendants(dom::Dom::DOCUMENT).find(|&n| d.element(n).and_then(|e| e.attr("id")) == Some(id)).expect("element");
    s.of[n].as_ref().unwrap()
}

fn lay(html: &str, width: f32) -> layout::Laid {
    let (d, s) = styled(html, width);
    let base = Url::parse("http://test/").unwrap();
    layout::layout(&d, &s, &base, width, &mut Fixed, &layout::ImageSizes::new())
}

fn texts(l: &layout::Laid) -> Vec<(f32, f32, String)> {
    l.items
        .iter()
        .filter_map(|i| if let Item::Text { x, y, text, .. } = i { Some((*x, *y, text.clone())) } else { None })
        .collect()
}

#[test]
fn colours() {
    assert_eq!(css::parse_color(&css::tokens("#fff"), 0), Some(0xffff_ffff));
    assert_eq!(css::parse_color(&css::tokens("#123456"), 0), Some(0xff12_3456));
    assert_eq!(css::parse_color(&css::tokens("rgb(255, 0, 0)"), 0), Some(0xffff_0000));
    assert_eq!(css::parse_color(&css::tokens("rgba(0 0 255 / 50%)"), 0), Some(0x8000_00ff));
    assert_eq!(css::parse_color(&css::tokens("hsl(120, 100%, 50%)"), 0), Some(0xff00_ff00));
    assert_eq!(css::parse_color(&css::tokens("rebeccapurple"), 0), Some(0xff66_3399));
    assert_eq!(css::parse_color(&css::tokens("light-dark(#eee, #222)"), 0), Some(0xffee_eeee));
}

#[test]
fn cascade() {
    let (d, s) = styled(
        "<style>p { color: red } .x { color: blue } #y { color: green } p.z { color: navy !important } :root { --c: #0f0 } .v { color: var(--c, red) }</style>\
         <p id=a>a</p><p id=b class=x>b</p><p id=y class=x>c</p><p id=c class='x z' style='color: black'>d</p><p id=d class=v>e</p>\
         <p id=e style='color: purple'>f</p>",
        800.0,
    );
    assert_eq!(style_of(&d, &s, "a").color, 0xffff_0000);
    assert_eq!(style_of(&d, &s, "b").color, 0xff00_00ff);
    assert_eq!(style_of(&d, &s, "y").color, 0xff00_8000);
    assert_eq!(style_of(&d, &s, "c").color, 0xff00_0080);
    assert_eq!(style_of(&d, &s, "d").color, 0xff00_ff00);
    assert_eq!(style_of(&d, &s, "e").color, 0xff80_0080);
}

#[test]
fn inheritance_and_units() {
    let (d, s) = styled(
        "<style>body { font-size: 20px } div { font-size: 1.5em; padding: 1em 2rem; width: calc(100% - 10px) }</style><div id=a><span id=b>x</span></div>",
        800.0,
    );
    let a = style_of(&d, &s, "a");
    assert_eq!(a.font_size, 30.0);
    assert_eq!(a.padding[style::TOP], style::Len::Px(30.0));
    assert_eq!(a.padding[style::LEFT], style::Len::Px(32.0));
    assert_eq!(style_of(&d, &s, "b").font_size, 30.0);
}

#[test]
fn media_queries() {
    let html = "<style>@media (max-width: 600px) { #a { color: red } } @media print { #a { color: blue } }</style><p id=a>x</p>";
    let (d, s) = styled(html, 500.0);
    assert_eq!(style_of(&d, &s, "a").color, 0xffff_0000);
    let (d, s) = styled(html, 900.0);
    assert_eq!(style_of(&d, &s, "a").color, 0xff00_0000);
}

#[test]
fn line_breaking() {
    // 10 pels per character at 20px: "aaa bbb ccc" is 110 pels.
    let l = lay("<body style='margin:0; font-size:20px'><p style='margin:0'>aaa bbb ccc</p>", 75.0);
    let t = texts(&l);
    assert_eq!(t.len(), 2, "{t:?}");
    assert_eq!(t[0].2, "aaa bbb");
    assert_eq!(t[1].2, "ccc");
    assert!(t[1].1 > t[0].1);
    // Centred.
    let l = lay("<body style='margin:0; font-size:20px'><p style='margin:0; text-align:center'>ab</p>", 100.0);
    assert_eq!(texts(&l)[0].0, 40.0);
}

#[test]
fn flex_row() {
    let l = lay(
        "<body style='margin:0; font-size:20px'><div style='display:flex'><div style='flex:1'>a</div><div style='flex:1'>b</div></div>",
        200.0,
    );
    let t = texts(&l);
    assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0.0, 100.0]);
}

#[test]
fn table_columns_line_up() {
    let l = lay(
        "<body style='margin:0; font-size:20px'><table style='border-spacing:0'><tr><td>a</td><td>b</td></tr><tr><td>cccc</td><td>d</td></tr></table>",
        400.0,
    );
    let t = texts(&l);
    let x_of = |s: &str| t.iter().find(|x| x.2 == s).unwrap().0;
    assert_eq!(x_of("b"), x_of("d"));
    assert!(x_of("b") >= 40.0);
}

#[test]
fn float_shortens_lines() {
    let l = lay(
        "<body style='margin:0; font-size:20px'><div style='float:right; width:60px; height:100px'></div><p style='margin:0'>aaa bbb ccc</p>",
        120.0,
    );
    let t = texts(&l);
    // 60 pels beside the float: a word per line.
    assert_eq!(t.len(), 3, "{t:?}");
}

#[test]
fn links_and_lists() {
    let l = lay("<body style='font-size:20px'><ul><li><a href='/x'>one</a></li><li>two</li></ul><ol start=3><li>c</li></ol>", 400.0);
    assert_eq!(l.links.len(), 1);
    assert_eq!(l.links[0].as_str(), "http://test/x");
    assert_eq!(l.items.iter().filter(|i| matches!(i, Item::Marker { .. })).count(), 2);
    assert!(texts(&l).iter().any(|t| t.2 == "3."));
}

#[test]
fn hidden_things() {
    let l = lay("<body><p style='display:none'>a</p><p style='opacity:0'>b</p><p>c</p><script>d</script>", 400.0);
    let t: Vec<String> = texts(&l).into_iter().map(|x| x.2).collect();
    assert_eq!(t, vec!["c".to_string()]);
}
