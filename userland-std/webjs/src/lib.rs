//! webjs: JavaScript for WebRS2 — Boa runs a page's scripts against a
//! minimal DOM over webcore's document before it is styled.
//!
//! The DOM the scripts see is written in JavaScript (`prelude.js`) over a
//! few native functions on node ids (`__dom_*` here). Scripts run once,
//! in document order (inline and `src`, not modules), then
//! DOMContentLoaded and load listeners and the timers they set. Later
//! events (clicks...) do not run scripts. Limits keep a script from
//! hanging the page: a loop iteration and a recursion limit, and 2 MB of
//! script per page.

use std::cell::RefCell;

use boa_engine::object::builtins::JsArray;
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsArgs, JsResult, JsValue, NativeFunction, Source};
use html5ever::{ns, LocalName, QualName};
use webcore::dom::{Dom, Element, NodeData, NodeId};
use webcore::Url;

pub struct Outcome {
    /// console.log output and errors.
    pub log: Vec<String>,
    pub scripts: usize,
    pub errors: usize,
}

struct State {
    dom: Dom,
    url: Url,
    log: Vec<String>,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    ST.with(|s| f(s.borrow_mut().as_mut().expect("webjs state")))
}

const PRELUDE: &str = include_str!("prelude.js");
const MAX_SOURCE: usize = 2 << 20;

/// Run the scripts of `dom` (the document at `url`); `fetch` gets external
/// scripts' text.
pub fn run(dom: Dom, url: &Url, fetch: &dyn Fn(&Url) -> Option<String>) -> (Dom, Outcome) {
    let mut scripts: Vec<Result<String, Url>> = Vec::new();
    for n in dom.descendants(Dom::DOCUMENT) {
        let Some(e) = dom.element(n) else { continue };
        if !e.is("script") {
            continue;
        }
        let ty = e.attr("type").unwrap_or("").trim().to_ascii_lowercase();
        if !(ty.is_empty() || ty.contains("javascript") || ty.contains("ecmascript")) {
            continue;
        }
        match e.attr("src") {
            Some(src) => {
                if let Ok(u) = url.join(src.trim()) {
                    scripts.push(Err(u));
                }
            }
            None => scripts.push(Ok(dom.text_content(n))),
        }
    }
    let mut out = Outcome { log: Vec::new(), scripts: 0, errors: 0 };
    if scripts.is_empty() {
        return (dom, out);
    }
    ST.with(|s| *s.borrow_mut() = Some(State { dom, url: url.clone(), log: Vec::new() }));
    {
        let mut ctx = Context::default();
        ctx.runtime_limits_mut().set_loop_iteration_limit(1_000_000);
        ctx.runtime_limits_mut().set_recursion_limit(400);
        ctx.runtime_limits_mut().set_stack_size_limit(10_000);
        register(&mut ctx);
        if let Err(e) = ctx.eval(Source::from_bytes(PRELUDE)) {
            out.log.push(format!("prelude: {e}"));
        }
        let mut total = 0;
        for s in scripts {
            let code = match s {
                Ok(c) => c,
                Err(u) => match fetch(&u) {
                    Some(c) => c,
                    None => {
                        out.log.push(format!("cannot fetch {u}"));
                        continue;
                    }
                },
            };
            total += code.len();
            if total > MAX_SOURCE {
                out.log.push("too much script: the rest is skipped".into());
                break;
            }
            out.scripts += 1;
            if let Err(e) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                out.errors += 1;
                out.log.push(format!("error: {e}"));
            }
            ctx.run_jobs().ok();
        }
        if let Err(e) = ctx.eval(Source::from_bytes("__finish()")) {
            out.log.push(format!("error: {e}"));
        }
        ctx.run_jobs().ok();
    }
    let st = ST.with(|s| s.borrow_mut().take()).unwrap();
    let mut dom = st.dom;
    out.log.extend(st.log);
    // Scripts ran: what is for browsers without them goes.
    let noscript: Vec<NodeId> = dom.descendants(Dom::DOCUMENT).filter(|&n| dom.element(n).is_some_and(|e| e.is("noscript"))).collect();
    for n in noscript {
        detach(&mut dom, n);
    }
    (dom, out)
}

fn num(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<i64> {
    Ok(args.get_or_undefined(i).to_number(ctx)? as i64)
}

fn string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(args.get_or_undefined(i).to_string(ctx)?.to_std_string_escaped())
}

fn node(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<Option<NodeId>> {
    let n = num(args, i, ctx)?;
    Ok(with(|s| (n >= 0 && (n as usize) < s.dom.nodes.len()).then_some(n as usize)))
}

fn ids(v: Vec<NodeId>, ctx: &mut Context) -> JsValue {
    JsArray::from_iter(v.into_iter().map(|i| JsValue::from(i as i32)), ctx).into()
}

fn id_value(n: Option<NodeId>) -> JsValue {
    JsValue::from(n.map_or(-1, |n| n as i32))
}

fn detach(dom: &mut Dom, n: NodeId) {
    if let Some(p) = dom.nodes[n].parent.take() {
        dom.nodes[p].children.retain(|&c| c != n);
    }
}

fn is_ancestor(dom: &Dom, a: NodeId, mut n: NodeId) -> bool {
    loop {
        if n == a {
            return true;
        }
        match dom.nodes[n].parent {
            Some(p) => n = p,
            None => return false,
        }
    }
}

/// Copy `src`'s subtree from another document; the copy is unattached.
fn import(dst: &mut Dom, from: &Dom, src: NodeId) -> NodeId {
    let data = match &from.nodes[src].data {
        NodeData::Element(e) => NodeData::Element(Element { name: e.name.clone(), attrs: e.attrs.clone(), template: None }),
        NodeData::Text(t) => NodeData::Text(t.clone()),
        NodeData::Comment => NodeData::Comment,
        NodeData::Doctype => NodeData::Comment,
        NodeData::Document => NodeData::Comment,
    };
    let n = dst.new_node(data);
    for &c in &from.nodes[src].children {
        let k = import(dst, from, c);
        dst.append_child(n, k);
    }
    n
}

/// Parse HTML as the contents of a body; the new nodes, unattached.
fn fragment(dom: &mut Dom, html: &str) -> Vec<NodeId> {
    let f = webcore::dom::parse(&format!("<!DOCTYPE html><html><body>{html}</body></html>"));
    let Some(body) = f.find("body") else { return Vec::new() };
    f.nodes[body].children.clone().into_iter().map(|c| import(dom, &f, c)).collect()
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

fn escape(s: &str, attr: bool) -> String {
    let mut o = s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    if attr {
        o = o.replace('"', "&quot;");
    }
    o
}

fn serialize(dom: &Dom, n: NodeId, out: &mut String) {
    match &dom.nodes[n].data {
        NodeData::Text(t) => out.push_str(&escape(t, false)),
        NodeData::Element(e) => {
            out.push('<');
            out.push_str(&e.name.local);
            for (k, v) in &e.attrs {
                out.push_str(&format!(" {}=\"{}\"", &**k, escape(v, true)));
            }
            out.push('>');
            if VOID.contains(&&*e.name.local) {
                return;
            }
            for &c in &dom.nodes[n].children {
                serialize(dom, c, out);
            }
            out.push_str(&format!("</{}>", &*e.name.local));
        }
        _ => {
            for &c in &dom.nodes[n].children {
                serialize(dom, c, out);
            }
        }
    }
}

fn query(dom: &Dom, root: NodeId, sel: &str, all: bool) -> Vec<NodeId> {
    let mut p = cssparser::Parser::new(sel);
    let Ok(list) = selectors::parser::SelectorList::parse(&webcore::select::Parser, &mut p, selectors::parser::ParseRelative::No) else {
        return Vec::new();
    };
    let mut caches = selectors::matching::SelectorCaches::default();
    let mut ctx = selectors::matching::MatchingContext::new(
        selectors::matching::MatchingMode::Normal,
        None,
        &mut caches,
        selectors::context::QuirksMode::NoQuirks,
        selectors::matching::NeedsSelectorFlags::No,
        selectors::matching::MatchingForInvalidation::No,
    );
    let mut out = Vec::new();
    for n in dom.descendants(root) {
        if dom.element(n).is_some() && selectors::matching::matches_selector_list(&list, webcore::select::El { dom, id: n }, &mut ctx) {
            out.push(n);
            if !all {
                break;
            }
        }
    }
    out
}

macro_rules! natives {
    ($ctx:ident, $($name:literal => $f:expr, $n:literal;)*) => {
        $( $ctx.register_global_callable(js_string!($name), $n, NativeFunction::from_fn_ptr($f)).expect($name); )*
    };
}

fn register(ctx: &mut Context) {
    natives! { ctx,
        "__dom_type" => |_, a, c| {
            let n = node(a, 0, c)?;
            Ok(JsValue::from(with(|s| match n.map(|n| &s.dom.nodes[n].data) {
                Some(NodeData::Element(_)) => 1,
                Some(NodeData::Text(_)) => 3,
                Some(NodeData::Document) => 9,
                _ => 8,
            })))
        }, 1;
        "__dom_parent" => |_, a, c| {
            let n = node(a, 0, c)?;
            Ok(id_value(n.and_then(|n| with(|s| s.dom.nodes[n].parent))))
        }, 1;
        "__dom_children" => |_, a, c| {
            let n = node(a, 0, c)?;
            let elements = a.get_or_undefined(1).to_boolean();
            let v = n.map(|n| with(|s| s.dom.nodes[n].children.iter().copied().filter(|&k| !elements || s.dom.element(k).is_some()).collect())).unwrap_or_default();
            Ok(ids(v, c))
        }, 2;
        "__dom_sibling" => |_, a, c| {
            let n = node(a, 0, c)?;
            let dir = num(a, 1, c)?;
            let elements = a.get_or_undefined(2).to_boolean();
            Ok(id_value(n.and_then(|n| with(|s| {
                let p = s.dom.nodes[n].parent?;
                let kids = &s.dom.nodes[p].children;
                let mut i = kids.iter().position(|&k| k == n)? as i64;
                loop {
                    i += dir;
                    if i < 0 || i as usize >= kids.len() {
                        return None;
                    }
                    let k = kids[i as usize];
                    if !elements || s.dom.element(k).is_some() {
                        return Some(k);
                    }
                }
            }))))
        }, 3;
        "__dom_text" => |_, a, c| {
            let n = node(a, 0, c)?;
            Ok(js_string!(n.map(|n| with(|s| s.dom.text_content(n))).unwrap_or_default()).into())
        }, 1;
        "__dom_set_text" => |_, a, c| {
            let n = node(a, 0, c)?;
            let t = string(a, 1, c)?;
            if let Some(n) = n {
                with(|s| {
                    if let NodeData::Text(old) = &mut s.dom.nodes[n].data {
                        *old = t;
                        return;
                    }
                    for k in std::mem::take(&mut s.dom.nodes[n].children) {
                        s.dom.nodes[k].parent = None;
                    }
                    if !t.is_empty() {
                        let k = s.dom.new_node(NodeData::Text(t));
                        s.dom.append_child(n, k);
                    }
                });
            }
            Ok(JsValue::undefined())
        }, 2;
        "__dom_insert" => |_, a, c| {
            let (p, k) = (node(a, 0, c)?, node(a, 1, c)?);
            let r = node(a, 2, c)?;
            if let (Some(p), Some(k)) = (p, k) {
                with(|s| {
                    if is_ancestor(&s.dom, k, p) {
                        return;
                    }
                    detach(&mut s.dom, k);
                    let pos = r.and_then(|r| s.dom.nodes[p].children.iter().position(|&x| x == r)).unwrap_or(s.dom.nodes[p].children.len());
                    s.dom.nodes[k].parent = Some(p);
                    s.dom.nodes[p].children.insert(pos, k);
                });
            }
            Ok(JsValue::undefined())
        }, 3;
        "__dom_remove" => |_, a, c| {
            if let Some(n) = node(a, 0, c)? {
                with(|s| detach(&mut s.dom, n));
            }
            Ok(JsValue::undefined())
        }, 1;
        "__dom_clone" => |_, a, c| {
            let n = node(a, 0, c)?;
            Ok(id_value(n.map(|n| with(|s| {
                // A copy through the subtree's HTML.
                let mut html = String::new();
                serialize(&s.dom, n, &mut html);
                fragment(&mut s.dom, &html).first().copied().unwrap_or(n)
            }))))
        }, 1;
        "__dom_attr" => |_, a, c| {
            let n = node(a, 0, c)?;
            let name = string(a, 1, c)?.to_ascii_lowercase();
            let v = n.and_then(|n| with(|s| s.dom.element(n).and_then(|e| e.attr(&name)).map(String::from)));
            Ok(v.map_or(JsValue::null(), |v| js_string!(v).into()))
        }, 2;
        "__dom_set_attr" => |_, a, c| {
            let n = node(a, 0, c)?;
            let name = string(a, 1, c)?.to_ascii_lowercase();
            let v = string(a, 2, c)?;
            if let Some(n) = n {
                with(|s| {
                    if let NodeData::Element(e) = &mut s.dom.nodes[n].data {
                        match e.attrs.iter_mut().find(|(k, _)| **k == *name) {
                            Some(x) => x.1 = v,
                            None => e.attrs.push((LocalName::from(name.as_str()), v)),
                        }
                    }
                });
            }
            Ok(JsValue::undefined())
        }, 3;
        "__dom_remove_attr" => |_, a, c| {
            let n = node(a, 0, c)?;
            let name = string(a, 1, c)?.to_ascii_lowercase();
            if let Some(n) = n {
                with(|s| {
                    if let NodeData::Element(e) = &mut s.dom.nodes[n].data {
                        e.attrs.retain(|(k, _)| **k != *name);
                    }
                });
            }
            Ok(JsValue::undefined())
        }, 2;
        "__dom_query" => |_, a, c| {
            let n = node(a, 0, c)?;
            let sel = string(a, 1, c)?;
            let all = a.get_or_undefined(2).to_boolean();
            let v = n.map(|n| with(|s| query(&s.dom, n, &sel, all))).unwrap_or_default();
            Ok(ids(v, c))
        }, 3;
        "__dom_matches" => |_, a, c| {
            let n = node(a, 0, c)?;
            let sel = string(a, 1, c)?;
            Ok(JsValue::from(n.is_some_and(|n| with(|s| {
                let root = s.dom.nodes[n].parent.unwrap_or(Dom::DOCUMENT);
                query(&s.dom, root, &sel, true).contains(&n)
            }))))
        }, 2;
        "__dom_by_id" => |_, a, c| {
            let id = string(a, 0, c)?;
            Ok(id_value(with(|s| s.dom.descendants(Dom::DOCUMENT).find(|&n| s.dom.element(n).and_then(|e| e.attr("id")) == Some(id.as_str())))))
        }, 1;
        "__dom_tag" => |_, a, c| {
            let n = node(a, 0, c)?;
            Ok(js_string!(n.and_then(|n| with(|s| s.dom.element(n).map(|e| e.name.local.to_string()))).unwrap_or_default()).into())
        }, 1;
        "__dom_create" => |_, a, c| {
            let tag = string(a, 0, c)?;
            Ok(id_value(Some(with(|s| {
                s.dom.new_node(NodeData::Element(Element { name: Box::new(QualName::new(None, ns!(html), LocalName::from(tag.as_str()))), attrs: Vec::new(), template: None }))
            }))))
        }, 1;
        "__dom_create_text" => |_, a, c| {
            let t = string(a, 0, c)?;
            Ok(id_value(Some(with(|s| s.dom.new_node(NodeData::Text(t))))))
        }, 1;
        "__dom_html" => |_, a, c| {
            let n = node(a, 0, c)?;
            let outer = a.get_or_undefined(1).to_boolean();
            let mut out = String::new();
            if let Some(n) = n {
                with(|s| {
                    if outer {
                        serialize(&s.dom, n, &mut out);
                    } else {
                        for &k in &s.dom.nodes[n].children {
                            serialize(&s.dom, k, &mut out);
                        }
                    }
                });
            }
            Ok(js_string!(out).into())
        }, 2;
        "__dom_set_html" => |_, a, c| {
            let n = node(a, 0, c)?;
            let html = string(a, 1, c)?;
            if let Some(n) = n {
                with(|s| {
                    for k in std::mem::take(&mut s.dom.nodes[n].children) {
                        s.dom.nodes[k].parent = None;
                    }
                    for k in fragment(&mut s.dom, &html) {
                        s.dom.append_child(n, k);
                    }
                });
            }
            Ok(JsValue::undefined())
        }, 2;
        "__dom_insert_html" => |_, a, c| {
            let n = node(a, 0, c)?;
            let pos = string(a, 1, c)?;
            let html = string(a, 2, c)?;
            if let Some(n) = n {
                with(|s| {
                    let new = fragment(&mut s.dom, &html);
                    let (parent, mut at) = match pos.as_str() {
                        "beforebegin" | "afterend" => {
                            let Some(p) = s.dom.nodes[n].parent else { return };
                            let i = s.dom.nodes[p].children.iter().position(|&x| x == n).unwrap_or(0);
                            (p, if pos == "afterend" { i + 1 } else { i })
                        }
                        "afterbegin" => (n, 0),
                        _ => (n, s.dom.nodes[n].children.len()),
                    };
                    for k in new {
                        s.dom.nodes[k].parent = Some(parent);
                        s.dom.nodes[parent].children.insert(at, k);
                        at += 1;
                    }
                });
            }
            Ok(JsValue::undefined())
        }, 3;
        "__dom_write" => |_, a, c| {
            let html = string(a, 0, c)?;
            with(|s| {
                let body = s.dom.find("body").unwrap_or(Dom::DOCUMENT);
                for k in fragment(&mut s.dom, &html) {
                    s.dom.append_child(body, k);
                }
            });
            Ok(JsValue::undefined())
        }, 1;
        "__location" => |_, _, _| Ok(js_string!(with(|s| s.url.to_string())).into()), 0;
        "__user_agent" => |_, _, _| Ok(js_string!(webcore::http::USER_AGENT).into()), 0;
        "__resolve" => |_, a, c| {
            let h = string(a, 0, c)?;
            Ok(js_string!(with(|s| s.url.join(&h).map(|u| u.to_string()).unwrap_or(h))).into())
        }, 1;
        "__log" => |_, a, c| {
            let t = string(a, 0, c)?;
            with(|s| {
                if s.log.len() < 200 {
                    s.log.push(t);
                }
            });
            Ok(JsValue::undefined())
        }, 1;
    }
    let _ = ctx.register_global_property(js_string!("__webjs"), JsValue::from(1), Attribute::all());
}

/// Run a snippet against a document and return the document (tests).
pub fn run_inline(html: &str, url: &str) -> (Dom, Outcome) {
    let dom = webcore::dom::parse(html);
    run(dom, &Url::parse(url).unwrap(), &|_| None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(d: &Dom, id: &str) -> String {
        let n = d.descendants(Dom::DOCUMENT).find(|&n| d.element(n).and_then(|e| e.attr("id")) == Some(id)).unwrap();
        d.text_content(n)
    }

    #[test]
    fn dom_basics() {
        let (d, o) = run_inline(
            "<p id=a>old</p><ul id=l></ul><div id=h></div><noscript>no</noscript>\
             <script>\
               document.getElementById('a').textContent = 'new ' + (1 + 2);\
               var l = document.getElementById('l');\
               ['x','y'].forEach(function (t) { var li = document.createElement('li'); li.textContent = t; l.appendChild(li); });\
               document.querySelector('#h').innerHTML = '<b>bold</b> and ' + document.querySelectorAll('li').length;\
               document.title = 'set by script';\
               console.log('hello', 42);\
               document.addEventListener('DOMContentLoaded', function () { document.getElementById('h').classList.add('ready'); });\
               setTimeout(function () { document.getElementById('a').style.display = 'none'; }, 10);\
             </script>",
            "http://test/",
        );
        assert_eq!(o.errors, 0, "{:?}", o.log);
        assert_eq!(text_of(&d, "a"), "new 3");
        assert_eq!(text_of(&d, "l"), "xy");
        assert_eq!(text_of(&d, "h"), "bold and 2");
        assert_eq!(d.title().as_deref(), Some("set by script"));
        assert!(o.log.iter().any(|l| l == "hello 42"), "{:?}", o.log);
        let h = d.descendants(Dom::DOCUMENT).find(|&n| d.element(n).and_then(|e| e.attr("id")) == Some("h")).unwrap();
        assert_eq!(d.element(h).unwrap().attr("class"), Some("ready"));
        let a = d.descendants(Dom::DOCUMENT).find(|&n| d.element(n).and_then(|e| e.attr("id")) == Some("a")).unwrap();
        assert_eq!(d.element(a).unwrap().attr("style"), Some("display: none"));
        assert!(d.find("noscript").is_none());
    }

    #[test]
    fn errors_and_limits() {
        let (_, o) = run_inline("<script>throw new Error('boom')</script><script>while (true) {}</script><script>var ok = 1;</script>", "http://test/");
        assert_eq!(o.scripts, 3);
        assert_eq!(o.errors, 2, "{:?}", o.log);
    }
}
