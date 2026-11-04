//! The document tree: nodes in an arena (`NodeId` indexes), built by
//! html5ever through [`TreeSink`].

use std::borrow::Cow;
use std::cell::RefCell;

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, ExpandedName, LocalName, QualName, local_name, ns};

pub type NodeId = usize;

pub struct Element {
    /// Boxed: [`Sink::elem_name`] hands out references to it.
    pub name: Box<QualName>,
    pub attrs: Vec<(LocalName, String)>,
    pub template: Option<NodeId>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| &**n == name).map(|(_, v)| v.as_str())
    }
    pub fn local(&self) -> &LocalName {
        &self.name.local
    }
    pub fn is(&self, name: &str) -> bool {
        &*self.name.local == name
    }
}

pub enum NodeData {
    Document,
    Doctype,
    Element(Element),
    Text(String),
    Comment,
}

pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

pub struct Dom {
    pub nodes: Vec<Node>,
    pub quirks: bool,
}

impl Dom {
    pub const DOCUMENT: NodeId = 0;

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes[id].data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].data {
            NodeData::Text(t) => Some(t),
            _ => None,
        }
    }

    /// The first element named `name` in document order.
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.descendants(Self::DOCUMENT).find(|&n| self.element(n).is_some_and(|e| e.is(name)))
    }

    /// Every node below `root` in document order (not `root` itself).
    pub fn descendants(&self, root: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack: Vec<NodeId> = self.nodes[root].children.iter().rev().copied().collect();
        std::iter::from_fn(move || {
            let n = stack.pop()?;
            stack.extend(self.nodes[n].children.iter().rev());
            Some(n)
        })
    }

    /// The text content of a node.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut s = String::new();
        if let Some(t) = self.text(id) {
            s.push_str(t);
        }
        for n in self.descendants(id) {
            if let Some(t) = self.text(n) {
                s.push_str(t);
            }
        }
        s
    }

    /// The document's title, whitespace collapsed.
    pub fn title(&self) -> Option<String> {
        let t = self.find("title")?;
        let s = self.text_content(t);
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        (!s.is_empty()).then_some(s)
    }

    pub fn parent_element(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes[id].parent?;
        self.element(p).map(|_| p)
    }

    /// Siblings before and after `id`.
    pub fn index_in_parent(&self, id: NodeId) -> Option<(NodeId, usize)> {
        let p = self.nodes[id].parent?;
        Some((p, self.nodes[p].children.iter().position(|&c| c == id)?))
    }

    pub fn new_node(&mut self, data: NodeData) -> NodeId {
        self.nodes.push(Node { parent: None, children: Vec::new(), data });
        self.nodes.len() - 1
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }
}

/// Parse an HTML document.
pub fn parse(html: &str) -> Dom {
    let sink = Sink {
        dom: RefCell::new(Dom {
            nodes: vec![Node { parent: None, children: Vec::new(), data: NodeData::Document }],
            quirks: false,
        }),
    };
    html5ever::parse_document(sink, Default::default()).one(html)
}

pub struct Sink {
    dom: RefCell<Dom>,
}

impl Sink {
    fn new_node(&self, data: NodeData) -> NodeId {
        self.dom.borrow_mut().new_node(data)
    }

    fn detach(dom: &mut Dom, id: NodeId) {
        if let Some(p) = dom.nodes[id].parent.take() {
            dom.nodes[p].children.retain(|&c| c != id);
        }
    }

    fn insert(&self, parent: NodeId, at: Option<usize>, child: NodeOrText<NodeId>) {
        let mut dom = self.dom.borrow_mut();
        let pos = at.unwrap_or(dom.nodes[parent].children.len());
        match child {
            NodeOrText::AppendText(t) => {
                // Join with a text node just before the insertion point.
                if pos > 0 {
                    let prev = dom.nodes[parent].children[pos - 1];
                    if let NodeData::Text(s) = &mut dom.nodes[prev].data {
                        s.push_str(&t);
                        return;
                    }
                }
                let id = dom.new_node(NodeData::Text(t.to_string()));
                dom.nodes[id].parent = Some(parent);
                dom.nodes[parent].children.insert(pos, id);
            }
            NodeOrText::AppendNode(id) => {
                Self::detach(&mut dom, id);
                let pos = pos.min(dom.nodes[parent].children.len());
                dom.nodes[id].parent = Some(parent);
                dom.nodes[parent].children.insert(pos, id);
            }
        }
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Dom;
    type ElemName<'a> = ExpandedName<'a>;

    fn finish(self) -> Dom {
        self.dom.into_inner()
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        Dom::DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> ExpandedName<'a> {
        let dom = self.dom.borrow();
        let NodeData::Element(e) = &dom.nodes[*target].data else { panic!("not an element") };
        let p: *const QualName = &*e.name;
        // The name is boxed and lives as long as the sink.
        unsafe { (*p).expanded() }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> NodeId {
        let template = flags.template.then(|| self.new_node(NodeData::Document));
        let attrs = attrs.into_iter().map(|a| (a.name.local, a.value.to_string())).collect();
        self.new_node(NodeData::Element(Element { name: Box::new(name), attrs, template }))
    }

    fn create_comment(&self, _text: StrTendril) -> NodeId {
        self.new_node(NodeData::Comment)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeId {
        self.new_node(NodeData::Comment)
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        self.insert(*parent, None, child);
    }

    fn append_based_on_parent_node(&self, element: &NodeId, prev_element: &NodeId, child: NodeOrText<NodeId>) {
        if self.dom.borrow().nodes[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _name: StrTendril, _public: StrTendril, _system: StrTendril) {
        let id = self.new_node(NodeData::Doctype);
        self.dom.borrow_mut().append_child(Dom::DOCUMENT, id);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        match &self.dom.borrow().nodes[*target].data {
            NodeData::Element(Element { template: Some(t), .. }) => *t,
            _ => *target,
        }
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.dom.borrow_mut().quirks = mode == QuirksMode::Quirks;
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let (parent, pos) = {
            let dom = self.dom.borrow();
            let Some(p) = dom.nodes[*sibling].parent else { return };
            let pos = dom.nodes[p].children.iter().position(|c| c == sibling).unwrap_or(0);
            (p, pos)
        };
        self.insert(parent, Some(pos), new_node);
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<Attribute>) {
        let mut dom = self.dom.borrow_mut();
        if let NodeData::Element(e) = &mut dom.nodes[*target].data {
            for a in attrs {
                if !e.attrs.iter().any(|(n, _)| *n == a.name.local) {
                    e.attrs.push((a.name.local, a.value.to_string()));
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        Self::detach(&mut self.dom.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut dom = self.dom.borrow_mut();
        let kids = std::mem::take(&mut dom.nodes[*node].children);
        for k in kids {
            dom.nodes[k].parent = Some(*new_parent);
            dom.nodes[*new_parent].children.push(k);
        }
    }
}

/// Is `name` an HTML element in the HTML namespace?
pub fn is_html(q: &QualName, name: LocalName) -> bool {
    q.ns == ns!(html) && q.local == name
}

#[allow(dead_code)]
fn _atoms() -> LocalName {
    local_name!("div")
}
