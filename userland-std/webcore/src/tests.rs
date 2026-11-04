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

fn media(q: &str, width: f32) -> bool {
    css::media_applies(q, Media { width, height: 600.0 })
}

#[test]
fn range_media_queries() {
    assert!(media("(width>=48rem)", 800.0));
    assert!(!media("(width>=48rem)", 700.0));
    assert!(media("screen and (width<=calc(48rem - .02px))", 700.0));
    assert!(!media("screen and (width<=calc(48rem - .02px))", 768.0));
    assert!(media("(400px <= width < 800px)", 400.0));
    assert!(!media("(400px <= width < 800px)", 800.0));
    assert!(!media("not all and (min-width: 100px)", 800.0));
    assert!(media("(max-width: 100px), (min-width: 500px)", 800.0));
    assert!(!media("print", 800.0));
    assert!(!media("(forced-colors:active)", 800.0));
    assert!(!media("(prefers-color-scheme:dark)", 800.0));
}

#[test]
fn supports_conditions() {
    assert!(css::supports_applies("(display:grid)"));
    assert!(css::supports_applies("(height:100dvh)"));
    assert!(!css::supports_applies("(anchor-name:--value)"));
    assert!(css::supports_applies("not (anchor-name:--value)"));
    assert!(!css::supports_applies("(grid-template-columns:subgrid)"));
    assert!(!css::supports_applies("(-webkit-touch-callout:none)"));
    assert!(css::supports_applies("selector(:popover-open)"));
    assert!(!css::supports_applies("selector(:nonsense)"));
}

#[test]
fn custom_properties() {
    // Names keep their case, and may refer to ones declared after them.
    let (d, s) = styled(
        "<style>:root{--Border-Color:var(--Base);--Base:#123456;--x:;}
         #a{color:var(--Border-Color)} #b{color:var(--border-color,#00ff00)} #c{--Base:#ff0000;color:var(--Base)}
         #d{color:var(--Border-Color)}</style><div id=a></div><div id=b></div><div id=c><p id=d></p></div>",
        400.0,
    );
    assert_eq!(style_of(&d, &s, "a").color, 0xff12_3456);
    assert_eq!(style_of(&d, &s, "b").color, 0xff00_ff00);
    assert_eq!(style_of(&d, &s, "c").color, 0xffff_0000);
    // Resolved where it was declared.
    assert_eq!(style_of(&d, &s, "d").color, 0xff12_3456);
}

#[test]
fn cascade_layers() {
    let (d, s) = styled(
        "<style>@layer base, theme;
         @layer theme { #a { color: #00ff00 } .i { color: #00ff00 !important } }
         @layer base { div#a.a { color: #ff0000 } .i { color: #ff0000 !important } }
         #u { color: #00ff00 } @layer base { #u.u { color: #ff0000 } }
         </style><div id=a class=a></div><div id=u class=u></div><div id=i class=i></div>",
        400.0,
    );
    // A later layer beats specificity, and no layer beats any layer;
    // !important turns the order round.
    assert_eq!(style_of(&d, &s, "a").color, 0xff00_ff00);
    assert_eq!(style_of(&d, &s, "u").color, 0xff00_ff00);
    assert_eq!(style_of(&d, &s, "i").color, 0xffff_0000);
}

#[test]
fn colour_mixing() {
    assert_eq!(css::parse_color(&css::tokens("color-mix(in srgb, #ff0000, #0000ff)"), 0), Some(0xff80_0080));
    assert_eq!(css::parse_color(&css::tokens("color-mix(in srgb, #ffffff 25%, #000000)"), 0), Some(0xff40_4040));
    assert_eq!(css::parse_color(&css::tokens("color-mix(in srgb, #000000 50%, transparent)"), 0), Some(0x8000_0000));
}

#[test]
fn background_shorthand_resets() {
    let (d, s) = styled("<style>#a{background-color:red;background:0 0}</style><button id=a></button>", 400.0);
    assert_eq!(style_of(&d, &s, "a").background, 0);
}

#[test]
fn flex_children_are_items() {
    let l = lay("<body style='margin:0;font-size:20px'><div style='display:flex;justify-content:space-between'><span>a</span><span>b</span></div>", 400.0);
    let t = texts(&l);
    assert_eq!(t[0].0, 0.0);
    assert_eq!(t[1].0, 390.0);
}

#[test]
fn grid_spans() {
    let (d, s) = styled("<div style='display:grid;grid-template-columns:repeat(12,minmax(0,1fr))'><p id=a style='grid-column:auto/span 12'></p><p id=b style='grid-column:2 / -1'></p></div>", 400.0);
    let (a, b) = (style_of(&d, &s, "a"), style_of(&d, &s, "b"));
    assert_eq!(a.grid_column_span, 12);
    assert_eq!(b.grid_col_lines, (2, -1));
    let l = lay("<body style='margin:0;font-size:20px'><div style='display:grid;grid-template-columns:repeat(4,minmax(0,1fr))'><p style='margin:0;grid-column:auto/span 4'>aaaa bbbb</p></div>", 400.0);
    assert_eq!(texts(&l).len(), 1, "one line across all columns");
}

#[test]
fn buttons_hold_content() {
    let l = lay("<body style='margin:0;font-size:20px'><button style='padding:0;border:0;font-size:20px'><b>go</b> on</button>", 400.0);
    assert!(texts(&l).iter().any(|t| t.2.contains("go")));
    assert!(l.items.iter().any(|i| matches!(i, Item::Control { kind: layout::ControlKind::ButtonBox, .. })));
}

#[test]
fn generated_content() {
    let l = lay("<style>p{margin:0} p::before{content:'x' attr(data-y)} p:after{content:none} i::after{content:'!'}</style><body><p data-y=z>a<i>b</i></p>", 400.0);
    let all: String = texts(&l).into_iter().map(|t| t.2).collect();
    assert_eq!(all.replace(' ', ""), "xzab!");
    // An empty one is a box, as tab underlines are.
    let l = lay("<style>body{margin:0}div{position:relative;height:20px}div::after{content:'';position:absolute;bottom:0;left:0;width:30px;height:2px;background:#ff0000}</style><body><div></div>", 400.0);
    assert!(l.items.iter().any(|i| matches!(i, Item::Fill { r, color: 0xffff_0000 } if r.w == 30.0 && r.y == 18.0)));
}

#[test]
fn hidden_by_default() {
    let l = lay("<body><dialog>a</dialog><div popover>b</div><details><summary>c</summary><p>d</p></details><details open><p>e</p></details>", 400.0);
    let t: Vec<String> = texts(&l).into_iter().map(|x| x.2).collect();
    assert_eq!(t, vec!["c".to_string(), "e".to_string()]);
}

#[test]
fn revealed_by_what_does_not_run() {
    // Animations end visible; a script clearing the opacity may not run.
    let l = lay("<style>.a{opacity:0;animation:in 1s forwards}</style><body><p class=a>a</p>", 400.0);
    assert_eq!(texts(&l).len(), 1);
    let l = lay("<body><p style='opacity:0'>a</p><script type=module src=x.js></script>", 400.0);
    assert_eq!(texts(&l).len(), 1);
}

#[test]
fn inline_block_baseline() {
    // The padded box's text sits on the line's baseline.
    let l = lay("<body style='margin:0;font-size:20px'>a<span style='display:inline-block;padding:10px 0'>b</span>", 400.0);
    let t = texts(&l);
    assert_eq!(t[0].1, t[1].1);
}

#[test]
fn table_by_display_and_spacing() {
    let l = lay("<body style='margin:0;font-size:20px'><div style='display:table'><div style='display:table-cell'>a</div><div style='display:table-cell'>b</div></div>", 400.0);
    assert_eq!(texts(&l).len(), 2);
    let l = lay("<body style='margin:0;font-size:20px'><table style='width:100%;border-collapse:collapse'><tr><td style='padding:0'>a</td><td style='padding:0'>b</td></tr></table>", 400.0);
    let t = texts(&l);
    assert!(t[1].0 > 100.0, "the table fills its width: {t:?}");
}

#[test]
fn percent_images_follow_their_line() {
    let (d, s) = styled("<body style='margin:0'><div style='width:100px'><img id=i src=x style='width:100%'></div>", 400.0);
    let n = d.descendants(dom::Dom::DOCUMENT).find(|&n| d.element(n).is_some_and(|e| e.is("img"))).unwrap();
    let mut sizes = layout::ImageSizes::new();
    sizes.insert(n, (400, 200));
    let l = layout::layout(&d, &s, &Url::parse("http://test/").unwrap(), 400.0, &mut Fixed, &sizes);
    let r = l.items.iter().find_map(|i| if let Item::Image { r, .. } = i { Some(*r) } else { None }).unwrap();
    assert_eq!((r.w, r.h), (100.0, 50.0));
}

#[test]
fn svg_shapes() {
    let (d, s) = styled(
        "<svg id=s width=16 height=16 viewBox='0 0 8 8' fill=currentColor style='color:#ff0000'>
         <path d='M0 0h4v4H0Z'/><circle cx=6 cy=6 r=2 fill='#0000ff'/>
         <path fill-rule=evenodd d='M4 0h4v4H4ZM5 1h2v2H5Z' fill='#00ff00'/></svg>",
        400.0,
    );
    let n = d.descendants(dom::Dom::DOCUMENT).find(|&n| d.element(n).is_some_and(|e| e.is("svg"))).unwrap();
    assert_eq!(crate::svg::natural_size(&d, n), (16.0, 16.0));
    let img = crate::svg::render(&d, &s, n, 16, 16);
    let px = |x: usize, y: usize| { let i = (y * 16 + x) * 4; [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]] };
    assert_eq!(px(3, 3), [255, 0, 0, 255]);
    assert_eq!(px(12, 12), [0, 0, 255, 255]);
    assert_eq!(px(9, 1), [0, 255, 0, 255]);
    // The hole of the even-odd ring, and outside everything.
    assert_eq!(px(12, 4)[3], 0);
    assert_eq!(px(1, 14)[3], 0);
}

#[test]
fn rounded_boxes() {
    let r = layout::Rect { x: 0.0, y: 0.0, w: 20.0, h: 20.0 };
    let mut area = 0.0;
    layout::round_spans(r, [10.0; 4], 0.0, |_, _, w, h| area += w * h);
    // Close to a circle's 314.
    assert!((300.0..330.0).contains(&area), "{area}");
    let mut ring = 0.0;
    layout::round_spans(r, [0.0; 4], 1.0, |_, _, w, h| ring += w * h);
    assert_eq!(ring, 76.0);
}
