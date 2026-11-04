// WebRS2's DOM for scripts, written over a few native primitives
// (__dom_*: node ids of the page's document). What a page's scripts can
// use: document.getElementById/querySelector(All)/getElementsBy*,
// createElement/createTextNode, element.textContent/innerHTML/innerText,
// attributes, id/className/classList, style.*, appendChild/removeChild/
// insertBefore/remove, parentNode/children/childNodes/first/last/
// next/previous, document.title/write, console, setTimeout (run once the
// scripts are done), addEventListener (DOMContentLoaded and load run
// then; other events never fire), location, navigator.
(function (g) {
  var cache = new Map();
  function wrap(id) {
    if (id === null || id === undefined || id < 0) return null;
    var w = cache.get(id);
    if (w) return w;
    var t = __dom_type(id);
    w = Object.create(t === 1 ? ElementProto : t === 9 ? DocumentProto : NodeProto);
    Object.defineProperty(w, "__id", { value: id });
    cache.set(id, w);
    return w;
  }
  function ids(list) { return list.map(wrap); }
  function camelToKebab(k) { return k.replace(/[A-Z]/g, function (c) { return "-" + c.toLowerCase(); }); }
  function parseStyle(s) {
    var m = {};
    (s || "").split(";").forEach(function (d) {
      var i = d.indexOf(":");
      if (i > 0) m[d.slice(0, i).trim().toLowerCase()] = d.slice(i + 1).trim();
    });
    return m;
  }
  function writeStyle(el, m) {
    var s = Object.keys(m).filter(function (k) { return m[k] !== ""; }).map(function (k) { return k + ": " + m[k]; }).join("; ");
    __dom_set_attr(el.__id, "style", s);
  }
  var listeners = [];

  var NodeProto = {
    get nodeType() { return __dom_type(this.__id); },
    get parentNode() { return wrap(__dom_parent(this.__id)); },
    get parentElement() { var p = wrap(__dom_parent(this.__id)); return p && p.nodeType === 1 ? p : null; },
    get childNodes() { return ids(__dom_children(this.__id, false)); },
    get firstChild() { var c = __dom_children(this.__id, false); return c.length ? wrap(c[0]) : null; },
    get lastChild() { var c = __dom_children(this.__id, false); return c.length ? wrap(c[c.length - 1]) : null; },
    get nextSibling() { return wrap(__dom_sibling(this.__id, 1, false)); },
    get previousSibling() { return wrap(__dom_sibling(this.__id, -1, false)); },
    get textContent() { return __dom_text(this.__id); },
    set textContent(v) { __dom_set_text(this.__id, String(v)); },
    get nodeValue() { return this.nodeType === 3 ? __dom_text(this.__id) : null; },
    set nodeValue(v) { if (this.nodeType === 3) __dom_set_text(this.__id, String(v)); },
    get data() { return this.nodeValue; },
    set data(v) { this.nodeValue = v; },
    appendChild: function (c) { __dom_insert(this.__id, c.__id, -1); return c; },
    append: function () { for (var i = 0; i < arguments.length; i++) { var a = arguments[i]; this.appendChild(typeof a === "string" ? document.createTextNode(a) : a); } },
    prepend: function () { for (var i = arguments.length - 1; i >= 0; i--) { var a = arguments[i]; var n = typeof a === "string" ? document.createTextNode(a) : a; this.insertBefore(n, this.firstChild); } },
    insertBefore: function (c, ref) { __dom_insert(this.__id, c.__id, ref ? ref.__id : -1); return c; },
    removeChild: function (c) { __dom_remove(c.__id); return c; },
    replaceChild: function (n, old) { this.insertBefore(n, old); __dom_remove(old.__id); return old; },
    remove: function () { __dom_remove(this.__id); },
    contains: function (n) { while (n) { if (n === this) return true; n = n.parentNode; } return false; },
    hasChildNodes: function () { return __dom_children(this.__id, false).length > 0; },
    addEventListener: function (type, fn) { listeners.push([this, String(type), fn]); },
    removeEventListener: function () {},
    dispatchEvent: function () { return true; },
    cloneNode: function () { return wrap(__dom_clone(this.__id)); },
  };

  var ElementProto = Object.create(NodeProto);
  Object.assign(ElementProto, {
    getAttribute: function (n) { return __dom_attr(this.__id, String(n)); },
    setAttribute: function (n, v) { __dom_set_attr(this.__id, String(n), String(v)); },
    removeAttribute: function (n) { __dom_remove_attr(this.__id, String(n)); },
    hasAttribute: function (n) { return __dom_attr(this.__id, String(n)) !== null; },
    querySelector: function (s) { var r = __dom_query(this.__id, String(s), false); return r.length ? wrap(r[0]) : null; },
    querySelectorAll: function (s) { return ids(__dom_query(this.__id, String(s), true)); },
    getElementsByTagName: function (t) { return ids(__dom_query(this.__id, String(t), true)); },
    getElementsByClassName: function (c) { return ids(__dom_query(this.__id, "." + String(c).trim().split(/\s+/).join("."), true)); },
    matches: function (s) { return __dom_matches(this.__id, String(s)); },
    closest: function (s) { var n = this; while (n && n.nodeType === 1) { if (n.matches(s)) return n; n = n.parentNode; } return null; },
    focus: function () {}, blur: function () {}, click: function () {},
    getBoundingClientRect: function () { return { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }; },
    insertAdjacentHTML: function (pos, html) { __dom_insert_html(this.__id, String(pos).toLowerCase(), String(html)); },
  });
  Object.defineProperties(ElementProto, {
    tagName: { get: function () { return __dom_tag(this.__id).toUpperCase(); } },
    nodeName: { get: function () { return __dom_tag(this.__id).toUpperCase(); } },
    localName: { get: function () { return __dom_tag(this.__id); } },
    id: { get: function () { return this.getAttribute("id") || ""; }, set: function (v) { this.setAttribute("id", v); } },
    className: { get: function () { return this.getAttribute("class") || ""; }, set: function (v) { this.setAttribute("class", v); } },
    href: { get: function () { var h = this.getAttribute("href"); return h === null ? "" : __resolve(h); }, set: function (v) { this.setAttribute("href", v); } },
    src: { get: function () { var h = this.getAttribute("src"); return h === null ? "" : __resolve(h); }, set: function (v) { this.setAttribute("src", v); } },
    value: { get: function () { return this.getAttribute("value") || ""; }, set: function (v) { this.setAttribute("value", v); } },
    hidden: { get: function () { return this.hasAttribute("hidden"); }, set: function (v) { if (v) this.setAttribute("hidden", ""); else this.removeAttribute("hidden"); } },
    children: { get: function () { return ids(__dom_children(this.__id, true)); } },
    childElementCount: { get: function () { return __dom_children(this.__id, true).length; } },
    firstElementChild: { get: function () { var c = __dom_children(this.__id, true); return c.length ? wrap(c[0]) : null; } },
    lastElementChild: { get: function () { var c = __dom_children(this.__id, true); return c.length ? wrap(c[c.length - 1]) : null; } },
    nextElementSibling: { get: function () { return wrap(__dom_sibling(this.__id, 1, true)); } },
    previousElementSibling: { get: function () { return wrap(__dom_sibling(this.__id, -1, true)); } },
    innerHTML: { get: function () { return __dom_html(this.__id, false); }, set: function (v) { __dom_set_html(this.__id, String(v)); } },
    outerHTML: { get: function () { return __dom_html(this.__id, true); } },
    innerText: { get: function () { return this.textContent; }, set: function (v) { this.textContent = v; } },
    dataset: { get: function () { var el = this; return new Proxy({}, {
      get: function (t, k) { return el.getAttribute("data-" + camelToKebab(String(k))) || undefined; },
      set: function (t, k, v) { el.setAttribute("data-" + camelToKebab(String(k)), v); return true; } }); } },
    classList: { get: function () {
      var el = this;
      function list() { return (el.getAttribute("class") || "").split(/\s+/).filter(function (x) { return x; }); }
      return {
        contains: function (c) { return list().indexOf(c) >= 0; },
        add: function () { var l = list(); for (var i = 0; i < arguments.length; i++) if (l.indexOf(arguments[i]) < 0) l.push(arguments[i]); el.setAttribute("class", l.join(" ")); },
        remove: function () { var a = Array.prototype.slice.call(arguments); el.setAttribute("class", list().filter(function (x) { return a.indexOf(x) < 0; }).join(" ")); },
        toggle: function (c, force) { var has = this.contains(c); var want = force === undefined ? !has : !!force; if (want && !has) this.add(c); if (!want && has) this.remove(c); return want; },
        get length() { return list().length; },
        item: function (i) { return list()[i] || null; },
      };
    } },
    style: { get: function () {
      var el = this;
      return new Proxy({}, {
        get: function (t, k) {
          if (k === "setProperty") return function (n, v) { var m = parseStyle(el.getAttribute("style")); m[String(n)] = String(v); writeStyle(el, m); };
          if (k === "removeProperty") return function (n) { var m = parseStyle(el.getAttribute("style")); delete m[String(n)]; writeStyle(el, m); };
          if (k === "cssText") return el.getAttribute("style") || "";
          return parseStyle(el.getAttribute("style"))[camelToKebab(String(k))] || "";
        },
        set: function (t, k, v) {
          if (k === "cssText") { el.setAttribute("style", String(v)); return true; }
          var m = parseStyle(el.getAttribute("style")); m[camelToKebab(String(k))] = String(v); writeStyle(el, m); return true;
        },
      });
    } },
  });

  var DocumentProto = Object.create(ElementProto);
  Object.defineProperties(DocumentProto, {
    documentElement: { get: function () { var c = __dom_children(0, true); return c.length ? wrap(c[0]) : null; } },
    body: { get: function () { var r = __dom_query(0, "body", false); return r.length ? wrap(r[0]) : null; } },
    head: { get: function () { var r = __dom_query(0, "head", false); return r.length ? wrap(r[0]) : null; } },
    title: { get: function () { var r = __dom_query(0, "title", false); return r.length ? __dom_text(r[0]).trim() : ""; },
             set: function (v) {
               var r = __dom_query(0, "title", false), t;
               if (r.length) t = r[0];
               else { var h = __dom_query(0, "head", false); if (!h.length) return; t = __dom_create("title"); __dom_insert(h[0], t, -1); }
               __dom_set_text(t, String(v));
             } },
    readyState: { get: function () { return "loading"; } },
    cookie: { get: function () { return ""; }, set: function () {} },
    location: { get: function () { return g.location; } },
    defaultView: { get: function () { return g; } },
  });
  Object.assign(DocumentProto, {
    getElementById: function (id) { var r = __dom_by_id(String(id)); return r < 0 ? null : wrap(r); },
    createElement: function (t) { return wrap(__dom_create(String(t).toLowerCase())); },
    createElementNS: function (ns, t) { return wrap(__dom_create(String(t).toLowerCase())); },
    createTextNode: function (s) { return wrap(__dom_create_text(String(s))); },
    createDocumentFragment: function () { return wrap(__dom_create("div")); },
    createComment: function () { return wrap(__dom_create_text("")); },
    write: function () { __dom_write(Array.prototype.join.call(arguments, "")); },
    writeln: function () { __dom_write(Array.prototype.join.call(arguments, "") + "\n"); },
  });

  g.document = wrap(0);
  g.window = g;
  g.self = g;
  g.globalThis = g;
  var href = __location();
  var u = href.match(/^([a-z]+:)\/\/([^\/:?#]*)(:\d+)?([^?#]*)(\?[^#]*)?(#.*)?$/i) || [];
  g.location = { href: href, protocol: u[1] || "", hostname: u[2] || "", host: (u[2] || "") + (u[3] || ""),
                 port: (u[3] || "").slice(1), pathname: u[4] || "/", search: u[5] || "", hash: u[6] || "",
                 origin: (u[1] || "") + "//" + (u[2] || "") + (u[3] || ""),
                 toString: function () { return href; }, assign: function () {}, replace: function () {}, reload: function () {} };
  g.navigator = { userAgent: __user_agent(), language: "en", languages: ["en"], platform: "OS/2", cookieEnabled: false, onLine: true };
  g.screen = { width: 1024, height: 768, availWidth: 1024, availHeight: 768, colorDepth: 24 };
  g.innerWidth = 1024; g.innerHeight = 768; g.devicePixelRatio = 1;
  function fmt(args) { return Array.prototype.map.call(args, function (a) { try { return typeof a === "string" ? a : JSON.stringify(a); } catch (e) { return String(a); } }).join(" "); }
  g.console = { log: function () { __log(fmt(arguments)); }, info: function () { __log(fmt(arguments)); },
                warn: function () { __log("warning: " + fmt(arguments)); }, error: function () { __log("error: " + fmt(arguments)); },
                debug: function () {}, trace: function () {}, group: function () {}, groupEnd: function () {}, table: function () {} };
  var timers = [];
  var nextTimer = 1;
  g.setTimeout = function (fn, ms) { if (typeof fn === "function") timers.push([+ms || 0, nextTimer, fn, Array.prototype.slice.call(arguments, 2)]); return nextTimer++; };
  g.clearTimeout = function (id) { timers = timers.filter(function (t) { return t[1] !== id; }); };
  g.setInterval = function () { return nextTimer++; };
  g.clearInterval = function () {};
  g.requestAnimationFrame = function (fn) { return g.setTimeout(fn, 16); };
  g.cancelAnimationFrame = function (id) { g.clearTimeout(id); };
  g.queueMicrotask = function (fn) { g.setTimeout(fn, 0); };
  g.addEventListener = function (type, fn) { listeners.push([g, String(type), fn]); };
  g.removeEventListener = function () {};
  g.alert = function (m) { __log("alert: " + m); };
  g.confirm = function () { return false; };
  g.prompt = function () { return null; };
  g.getComputedStyle = function (el) { return el.style; };
  g.matchMedia = function (q) { return { matches: false, media: q, addListener: function () {}, removeListener: function () {}, addEventListener: function () {} }; };
  var store = {};
  g.localStorage = g.sessionStorage = { getItem: function (k) { return k in store ? store[k] : null; }, setItem: function (k, v) { store[k] = String(v); },
                                        removeItem: function (k) { delete store[k]; }, clear: function () { store = {}; } };
  // Called by the host when the scripts have run.
  g.__finish = function () {
    function fire(type) {
      var ev = { type: type, target: g.document, preventDefault: function () {}, stopPropagation: function () {} };
      listeners.filter(function (l) { return l[1] === type; }).forEach(function (l) {
        try { l[2].call(l[0], ev); } catch (e) { __log("error in " + type + " handler: " + e); }
      });
    }
    fire("DOMContentLoaded");
    fire("readystatechange");
    fire("load");
    // Timers in the order they would fire; those they set run too.
    for (var rounds = 0; timers.length && rounds < 200; rounds++) {
      timers.sort(function (a, b) { return a[0] - b[0] || a[1] - b[1]; });
      var t = timers.shift();
      try { t[2].apply(g, t[3]); } catch (e) { __log("error in timer: " + e); }
    }
  };
})(this);
