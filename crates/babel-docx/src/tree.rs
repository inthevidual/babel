//! A small mutable XML tree for one element and what is inside it (a
//! paragraph), enough for Babel's write-back: elements keep their qualified
//! names and attributes in order, text is kept unescaped, anything else
//! (comments, processing instructions) is carried through as written.
//!
//! Nodes live in an arena and are addressed by index, so inserting and
//! removing siblings never invalidates a reference to another node.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

pub type Id = usize;

#[derive(Debug, Clone)]
pub enum Kind {
    Element { name: String, attrs: Vec<(String, String)> },
    Text(String),
    /// Written out exactly as read.
    Raw(String),
}

#[derive(Debug, Clone)]
struct Node {
    kind: Kind,
    parent: Option<Id>,
    children: Vec<Id>,
}

#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<Node>,
    pub root: Id,
}

#[derive(Debug, thiserror::Error)]
#[error("XML: {0}")]
pub struct XmlError(pub String);

fn unescape(s: &str) -> Result<String, XmlError> {
    quick_xml::escape::unescape(s).map(|c| c.into_owned()).map_err(|e| XmlError(e.to_string()))
}

fn start_kind(e: &BytesStart) -> Result<Kind, XmlError> {
    let name = String::from_utf8_lossy(e.name().into_inner().as_bytes()).into_owned();
    let mut attrs = vec![];
    for a in e.attributes() {
        let a = a.map_err(|e| XmlError(e.to_string()))?;
        let k = String::from_utf8_lossy(a.key.into_inner().as_bytes()).into_owned();
        attrs.push((k, unescape(&a.value)?));
    }
    Ok(Kind::Element { name, attrs })
}

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

impl Tree {
    /// Parses XML holding one root element (a fragment such as one `w:p`).
    pub fn parse(xml: &str) -> Result<Tree, XmlError> {
        let mut r = Reader::from_str(xml);
        let mut t = Tree { nodes: vec![], root: 0 };
        let mut stack: Vec<Id> = vec![];
        let mut root = None;
        loop {
            let ev = r.read_event().map_err(|e| XmlError(e.to_string()))?;
            let kind = match &ev {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => start_kind(e)?,
                Event::End(_) => {
                    stack.pop();
                    continue;
                }
                Event::Text(e) => {
                    if stack.is_empty() {
                        continue;
                    }
                    Kind::Text(unescape(e.as_ref())?)
                }
                Event::GeneralRef(e) => {
                    if stack.is_empty() {
                        continue;
                    }
                    Kind::Text(unescape(&format!("&{};", &*e.as_ref()))?)
                }
                Event::CData(e) => Kind::Text(e.as_ref().to_owned()),
                Event::Comment(e) => Kind::Raw(format!("<!--{}-->", &*e.as_ref())),
                Event::PI(e) => Kind::Raw(format!("<?{}?>", &*e.as_ref())),
                Event::Decl(_) | Event::DocType(_) => continue,
            };
            let id = t.push(kind, stack.last().copied());
            if let Some(&p) = stack.last() {
                t.nodes[p].children.push(id);
            } else if root.is_none() {
                root = Some(id);
            }
            if matches!(ev, Event::Start(_)) {
                stack.push(id);
            }
        }
        t.root = root.ok_or_else(|| XmlError("no element".into()))?;
        // Adjacent text pieces (text and character references) become one node.
        t.join_text(t.root);
        Ok(t)
    }

    fn join_text(&mut self, id: Id) {
        let kids = self.nodes[id].children.clone();
        let mut keep: Vec<Id> = vec![];
        for k in kids {
            if let (Some(&last), Kind::Text(s)) = (keep.last(), &self.nodes[k].kind) {
                if let Kind::Text(prev) = &self.nodes[last].kind {
                    let joined = format!("{prev}{s}");
                    self.nodes[last].kind = Kind::Text(joined);
                    continue;
                }
            }
            keep.push(k);
        }
        self.nodes[id].children = keep.clone();
        for k in keep {
            if matches!(self.nodes[k].kind, Kind::Element { .. }) {
                self.join_text(k);
            }
        }
    }

    fn push(&mut self, kind: Kind, parent: Option<Id>) -> Id {
        self.nodes.push(Node { kind, parent, children: vec![] });
        self.nodes.len() - 1
    }

    pub fn kind(&self, id: Id) -> &Kind {
        &self.nodes[id].kind
    }

    pub fn name(&self, id: Id) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn local_name(&self, id: Id) -> Option<&str> {
        self.name(id).map(|n| n.rsplit(':').next().unwrap_or(n))
    }

    pub fn attr(&self, id: Id, key: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn set_attr(&mut self, id: Id, key: &str, value: &str) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[id].kind {
            match attrs.iter_mut().find(|(k, _)| k == key) {
                Some(a) => a.1 = value.to_owned(),
                None => attrs.push((key.to_owned(), value.to_owned())),
            }
        }
    }

    pub fn remove_attr(&mut self, id: Id, key: &str) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[id].kind {
            attrs.retain(|(k, _)| k != key);
        }
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.nodes[id].parent
    }

    pub fn children(&self, id: Id) -> &[Id] {
        &self.nodes[id].children
    }

    /// Element children only.
    pub fn elements(&self, id: Id) -> Vec<Id> {
        self.nodes[id].children.iter().copied().filter(|&c| matches!(self.nodes[c].kind, Kind::Element { .. })).collect()
    }

    /// The text inside, all of it.
    pub fn text_content(&self, id: Id) -> String {
        let mut s = String::new();
        self.collect_text(id, &mut s);
        s
    }

    fn collect_text(&self, id: Id, out: &mut String) {
        match &self.nodes[id].kind {
            Kind::Text(t) => out.push_str(t),
            Kind::Element { .. } => {
                for &c in &self.nodes[id].children {
                    self.collect_text(c, out);
                }
            }
            Kind::Raw(_) => {}
        }
    }

    /// Elements under `id` (not `id` itself) in document order.
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = vec![];
        let mut stack: Vec<Id> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            if matches!(self.nodes[n].kind, Kind::Element { .. }) {
                out.push(n);
                stack.extend(self.nodes[n].children.iter().rev().copied());
            }
        }
        out
    }

    pub fn new_element(&mut self, name: &str, attrs: &[(&str, &str)]) -> Id {
        let attrs = attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self.push(Kind::Element { name: name.to_owned(), attrs }, None)
    }

    pub fn new_text(&mut self, text: &str) -> Id {
        self.push(Kind::Text(text.to_owned()), None)
    }

    /// A detached deep copy of `id`.
    pub fn deep_clone(&mut self, id: Id) -> Id {
        let kind = self.nodes[id].kind.clone();
        let copy = self.push(kind, None);
        for c in self.nodes[id].children.clone() {
            let cc = self.deep_clone(c);
            self.append(copy, cc);
        }
        copy
    }

    /// A detached shallow copy (the element without its children).
    pub fn shallow_clone(&mut self, id: Id) -> Id {
        let kind = self.nodes[id].kind.clone();
        self.push(kind, None)
    }

    pub fn detach(&mut self, id: Id) {
        if let Some(p) = self.nodes[id].parent.take() {
            self.nodes[p].children.retain(|&c| c != id);
        }
    }

    pub fn append(&mut self, parent: Id, child: Id) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    pub fn insert_before(&mut self, reference: Id, node: Id) {
        self.detach(node);
        let p = self.nodes[reference].parent.expect("reference node has a parent");
        let i = self.nodes[p].children.iter().position(|&c| c == reference).expect("child of its parent");
        self.nodes[node].parent = Some(p);
        self.nodes[p].children.insert(i, node);
    }

    pub fn insert_after(&mut self, reference: Id, node: Id) {
        self.detach(node);
        let p = self.nodes[reference].parent.expect("reference node has a parent");
        let i = self.nodes[p].children.iter().position(|&c| c == reference).expect("child of its parent");
        self.nodes[node].parent = Some(p);
        self.nodes[p].children.insert(i + 1, node);
    }

    pub fn ancestors(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        std::iter::successors(self.nodes[id].parent, |&p| self.nodes[p].parent)
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        self.write(self.root, &mut out);
        out
    }

    pub fn serialize_node(&self, id: Id) -> String {
        let mut out = String::new();
        self.write(id, &mut out);
        out
    }

    fn write(&self, id: Id, out: &mut String) {
        match &self.nodes[id].kind {
            Kind::Text(t) => escape_text(t, out),
            Kind::Raw(r) => out.push_str(r),
            Kind::Element { name, attrs } => {
                out.push('<');
                out.push_str(name);
                for (k, v) in attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    escape_attr(v, out);
                    out.push('"');
                }
                if self.nodes[id].children.is_empty() {
                    out.push_str("/>");
                    return;
                }
                out.push('>');
                for &c in &self.nodes[id].children {
                    self.write(c, out);
                }
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_paragraph() {
        let xml = r#"<w:p xmlns:w="urn:w" data-bp="d:1"><w:r><w:t xml:space="preserve">a &amp; b &lt;c&gt;</w:t></w:r><!-- note --><w:r><w:tab/></w:r></w:p>"#;
        let t = Tree::parse(xml).unwrap();
        assert_eq!(t.serialize(), xml);
        assert_eq!(t.text_content(t.root), "a & b <c>");
    }

    #[test]
    fn inserts_keep_other_references_valid() {
        let mut t = Tree::parse(r#"<p><a/><b/></p>"#).unwrap();
        let kids = t.elements(t.root);
        let (a, b) = (kids[0], kids[1]);
        let x = t.new_element("x", &[("k", "v\"")]);
        t.insert_after(a, x);
        let y = t.new_element("y", &[]);
        t.insert_before(a, y);
        t.detach(b);
        assert_eq!(t.serialize(), r#"<p><y/><a/><x k="v&quot;"/></p>"#);
    }
}
