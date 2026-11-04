//! CSS selectors with Servo's `selectors` crate: its `SelectorImpl` for
//! this engine (plain strings for names and values) and its `Element`
//! trait for the arena DOM.

use std::fmt;

use cssparser::ToCss;
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext};
use cssparser::ParseError;
use selectors::parser::SelectorParseErrorKind;
use selectors::{OpaqueElement, SelectorImpl};

use crate::dom::{Dom, NodeData, NodeId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Impl;

/// A name, class, id or attribute value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Str(pub String);

impl ToCss for Str {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        cssparser::serialize_identifier(&self.0, dest)
    }
}

impl<'a> From<&'a str> for Str {
    fn from(s: &'a str) -> Self {
        Str(s.to_string())
    }
}

impl AsRef<str> for Str {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Str {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PrecomputedHash for Str {
    fn precomputed_hash(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for b in self.0.bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        h
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PseudoClass {
    Link,
    AnyLink,
    Visited,
    Hover,
    Active,
    Focus,
    FocusWithin,
    FocusVisible,
    Checked,
    Disabled,
    Enabled,
    /// Understood, but no element ever is one of these.
    Never(String),
}

impl ToCss for PseudoClass {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        if let PseudoClass::Never(n) = self {
            dest.write_str(":")?;
            return dest.write_str(n);
        }
        dest.write_str(match self {
            PseudoClass::Link => ":link",
            PseudoClass::AnyLink => ":any-link",
            PseudoClass::Visited => ":visited",
            PseudoClass::Hover => ":hover",
            PseudoClass::Active => ":active",
            PseudoClass::Focus => ":focus",
            PseudoClass::FocusWithin => ":focus-within",
            PseudoClass::FocusVisible => ":focus-visible",
            PseudoClass::Checked => ":checked",
            PseudoClass::Disabled => ":disabled",
            PseudoClass::Enabled => ":enabled",
            PseudoClass::Never(_) => "",
        })
    }
}

impl selectors::parser::NonTSPseudoClass for PseudoClass {
    fn is_active_or_hover(&self) -> bool {
        matches!(self, PseudoClass::Active | PseudoClass::Hover)
    }
    fn is_user_action_state(&self) -> bool {
        matches!(self, PseudoClass::Active | PseudoClass::Hover | PseudoClass::Focus)
    }
}

/// ::before and friends are parsed but never match (no generated content).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PseudoElement(String);

impl ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str("::")?;
        dest.write_str(&self.0)
    }
}

impl selectors::parser::PseudoElement for PseudoElement {}

impl SelectorImpl for Impl {
    type ExtraMatchingData<'a> = std::marker::PhantomData<&'a ()>;
    type AttrValue = Str;
    type Identifier = Str;
    type LocalName = Str;
    type NamespaceUrl = Str;
    type NamespacePrefix = Str;
    type BorrowedNamespaceUrl = str;
    type BorrowedLocalName = str;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
}

pub struct Parser;

impl<'i> selectors::parser::Parser<'i> for Parser {
    type Impl = Impl;
    type Error = SelectorParseErrorKind;

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_non_ts_pseudo_class(&self, name: &str) -> Result<PseudoClass, ParseError<Self::Error>> {
        let p = match name.to_ascii_lowercase().as_str() {
            "link" => PseudoClass::Link,
            "any-link" => PseudoClass::AnyLink,
            "visited" => PseudoClass::Visited,
            "hover" => PseudoClass::Hover,
            "active" => PseudoClass::Active,
            "focus" => PseudoClass::Focus,
            "focus-within" => PseudoClass::FocusWithin,
            "focus-visible" => PseudoClass::FocusVisible,
            "checked" => PseudoClass::Checked,
            "disabled" => PseudoClass::Disabled,
            "enabled" => PseudoClass::Enabled,
            n @ ("popover-open" | "modal" | "fullscreen" | "target" | "open" | "user-invalid" | "user-valid" | "invalid" | "autofill" | "placeholder-shown" | "indeterminate") => PseudoClass::Never(n.to_string()),
            _ => {
                return Err(ParseError::custom(SelectorParseErrorKind::UnsupportedPseudoClassOrElement));
            }
        };
        Ok(p)
    }

    fn parse_pseudo_element(&self, name: &str) -> Result<PseudoElement, ParseError<Self::Error>> {
        Ok(PseudoElement(name.to_string()))
    }
}

/// An element of a document, for matching.
#[derive(Clone, Copy)]
pub struct El<'a> {
    pub dom: &'a Dom,
    pub id: NodeId,
}

impl fmt::Debug for El<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "El({})", self.id)
    }
}

impl<'a> El<'a> {
    fn elem(&self) -> &'a crate::dom::Element {
        self.dom.element(self.id).expect("element")
    }

    fn sibling(&self, step: isize) -> Option<Self> {
        let p = self.dom.nodes[self.id].parent?;
        let kids = &self.dom.nodes[p].children;
        let mut i = kids.iter().position(|&c| c == self.id)? as isize;
        loop {
            i += step;
            if i < 0 || i as usize >= kids.len() {
                return None;
            }
            let c = kids[i as usize];
            if self.dom.element(c).is_some() {
                return Some(El { dom: self.dom, id: c });
            }
        }
    }
}

impl selectors::Element for El<'_> {
    type Impl = Impl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(&self.dom.nodes[self.id])
    }

    fn parent_element(&self) -> Option<Self> {
        self.dom.parent_element(self.id).map(|p| El { dom: self.dom, id: p })
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn first_element_child(&self) -> Option<Self> {
        self.dom.nodes[self.id].children.iter().find(|&&c| self.dom.element(c).is_some()).map(|&c| El { dom: self.dom, id: c })
    }

    fn is_html_element_in_html_document(&self) -> bool {
        true
    }

    fn has_local_name(&self, name: &str) -> bool {
        &*self.elem().name.local == name
    }

    fn has_namespace(&self, ns: &str) -> bool {
        &*self.elem().name.ns == ns
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.elem().name == other.elem().name
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&Str>,
        local_name: &Str,
        operation: &AttrSelectorOperation<&Str>,
    ) -> bool {
        if let NamespaceConstraint::Specific(n) = ns {
            if !n.0.is_empty() {
                return false;
            }
        }
        self.elem().attrs.iter().any(|(n, v)| (&**n).eq_ignore_ascii_case(&local_name.0) && operation.eval_str(v))
    }

    fn match_non_ts_pseudo_class(&self, pc: &PseudoClass, _context: &mut MatchingContext<Impl>) -> bool {
        let e = self.elem();
        match pc {
            PseudoClass::Link | PseudoClass::AnyLink => self.is_link(),
            PseudoClass::Checked => e.attr("checked").is_some() || e.attr("selected").is_some(),
            PseudoClass::Disabled => e.attr("disabled").is_some(),
            PseudoClass::Enabled => matches!(&*e.name.local, "input" | "button" | "select" | "textarea") && e.attr("disabled").is_none(),
            _ => false,
        }
    }

    fn match_pseudo_element(&self, _pe: &PseudoElement, _context: &mut MatchingContext<Impl>) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        let e = self.elem();
        matches!(&*e.name.local, "a" | "area" | "link") && e.attr("href").is_some()
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &Str, case: CaseSensitivity) -> bool {
        self.elem().attr("id").is_some_and(|v| case.eq(v.as_bytes(), id.0.as_bytes()))
    }

    fn has_class(&self, name: &Str, case: CaseSensitivity) -> bool {
        self.elem().attr("class").is_some_and(|v| v.split_ascii_whitespace().any(|c| case.eq(c.as_bytes(), name.0.as_bytes())))
    }

    fn has_custom_state(&self, _name: &Str) -> bool {
        false
    }

    fn imported_part(&self, _name: &Str) -> Option<Str> {
        None
    }

    fn is_part(&self, _name: &Str) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.dom.nodes[self.id].children.iter().all(|&c| match &self.dom.nodes[c].data {
            NodeData::Element(_) => false,
            NodeData::Text(t) => t.is_empty(),
            _ => true,
        })
    }

    fn is_root(&self) -> bool {
        self.dom.nodes[self.id].parent == Some(Dom::DOCUMENT)
    }

    fn add_element_unique_hashes(&self, _filter: &mut BloomFilter) -> bool {
        false
    }
}
