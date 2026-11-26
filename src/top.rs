//! `--top N`: the N largest nodes by serialized size, so an agent (or a
//! human) can see where the bytes are before opening the document.

use crate::json_tree::{JsonNode, JsonScalar, to_compact_json};
use crate::paths::display_path;
use crate::xml_tree::{XmlNode, child_segments};

pub struct Entry {
    pub path: String,
    pub bytes: usize,
}

/// Biggest first; ties keep document order. The root itself is excluded
/// (it is always the largest and has no path).
pub fn json_top(root: &JsonNode, n: usize) -> Vec<Entry> {
    let mut all = Vec::new();
    json_size(root, &[], &mut all);
    largest(all, n)
}

pub fn xml_top(root: &XmlNode, n: usize) -> Vec<Entry> {
    let mut all = Vec::new();
    xml_size(root, &[], &mut all);
    largest(all, n)
}

fn largest(mut all: Vec<Entry>, n: usize) -> Vec<Entry> {
    all.sort_by_key(|e| std::cmp::Reverse(e.bytes)); // stable
    all.truncate(n);
    all
}

/// Compact-JSON byte size, computed bottom-up so each node is visited once.
fn json_size(node: &JsonNode, path: &[String], all: &mut Vec<Entry>) -> usize {
    let children: Vec<(String, &JsonNode)> = match node {
        JsonNode::Scalar(_) => return to_compact_json(node).len(),
        JsonNode::Object(f) => f.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
    };
    let is_object = matches!(node, JsonNode::Object(_));
    let mut total = 2 + children.len().saturating_sub(1);
    for (label, child) in &children {
        let mut p = path.to_vec();
        p.push(label.clone());
        let size = json_size(child, &p, all);
        all.push(Entry {
            path: display_path(&p),
            bytes: size,
        });
        let key = if is_object { quoted_len(label) + 1 } else { 0 };
        total += key + size;
    }
    total
}

fn quoted_len(s: &str) -> usize {
    to_compact_json(&JsonNode::Scalar(JsonScalar::Str(s.to_string()))).len()
}

/// Approximate serialized XML size: `<n a="v">text…</n>`, `<n/>` when empty.
fn xml_size(node: &XmlNode, path: &[String], all: &mut Vec<Entry>) -> usize {
    let mut total = node.text.as_ref().map_or(0, String::len);
    for (name, value) in &node.attributes {
        let size = name.len() + value.len() + 4; // ` n="v"`
        let mut p = path.to_vec();
        p.push(format!("@{name}"));
        all.push(Entry {
            path: display_path(&p),
            bytes: size,
        });
        total += size;
    }
    for (child, segment) in node.children.iter().zip(child_segments(node)) {
        let mut p = path.to_vec();
        p.push(segment);
        let size = xml_size(child, &p, all);
        all.push(Entry {
            path: display_path(&p),
            bytes: size,
        });
        total += size;
    }
    let empty = node.text.is_none() && node.children.is_empty();
    total
        + if empty {
            node.name.len() + 3
        } else {
            2 * node.name.len() + 5
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jtop(src: &str, n: usize) -> Vec<(String, usize)> {
        let node: JsonNode = serde_json::from_str(src).unwrap();
        json_top(&node, n)
            .into_iter()
            .map(|e| (e.path, e.bytes))
            .collect()
    }

    fn xtop(src: &str, n: usize) -> Vec<(String, usize)> {
        let doc = roxmltree::Document::parse(src).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        xml_top(&node, n)
            .into_iter()
            .map(|e| (e.path, e.bytes))
            .collect()
    }

    #[test]
    fn sizes_match_the_compact_json_byte_length_of_each_subtree() {
        let src = r#"{"a":"xxxxxxxxxx","b":{"c":[1,2,3],"d":"é"},"e":null}"#;
        let node: JsonNode = serde_json::from_str(src).unwrap();
        let mut all = Vec::new();
        json_size(&node, &[], &mut all);
        for e in &all {
            let sub = crate::json_tree::find_json_path(&node, &e.path).unwrap();
            assert_eq!(e.bytes, to_compact_json(sub).len(), "path {}", e.path);
        }
        assert_eq!(all.len(), 8);
    }

    #[test]
    fn root_size_matches_total_document_size() {
        let src = r#"{"k":[{"x":1},{"y":"z"}],"q":{}}"#;
        let node: JsonNode = serde_json::from_str(src).unwrap();
        assert_eq!(
            json_size(&node, &[], &mut Vec::new()),
            to_compact_json(&node).len()
        );
    }

    #[test]
    fn returns_biggest_first_with_ties_in_document_order() {
        let top = jtop(r#"{"a":"aa","b":"bb","c":"cccc"}"#, 3);
        assert_eq!(top, [("c".into(), 6), ("a".into(), 4), ("b".into(), 4)]);
    }

    #[test]
    fn n_larger_than_node_count_returns_everything() {
        assert_eq!(jtop(r#"{"a":1}"#, 50).len(), 1);
    }

    #[test]
    fn empty_containers_and_scalar_roots() {
        assert_eq!(jtop("{}", 5).len(), 0);
        assert_eq!(jtop("[]", 5).len(), 0);
        assert_eq!(jtop("42", 5).len(), 0);
        assert_eq!(
            jtop(r#"{"e":[],"o":{}}"#, 5),
            [("e".into(), 2), ("o".into(), 2)]
        );
    }

    #[test]
    fn keys_with_dots_are_escaped_like_paths_output() {
        assert_eq!(jtop(r#"{"a.b":"xx"}"#, 1), [("a\\.b".into(), 4)]);
    }

    #[test]
    fn xml_counts_text_attributes_and_children() {
        let top = xtop(
            r#"<r id="1"><a>hello</a><b/><c x="long-attr-value"/></r>"#,
            5,
        );
        assert_eq!(top[0], ("c".into(), 24));
        assert_eq!(top[1], ("c.@x".into(), 20));
        assert_eq!(top[2], ("a".into(), 12));
    }

    #[test]
    fn xml_repeated_siblings_use_indexed_paths() {
        let top = xtop("<r><i>aaaa</i><i>b</i></r>", 2);
        assert_eq!(top[0].0, "i[0]");
    }

    #[test]
    fn large_document_is_ranked_correctly() {
        let body: String = (0..50_000).map(|i| format!("\"k{i}\":{i},")).collect();
        let src = format!("{{{body}\"big\":\"{}\"}}", "x".repeat(1_000));
        assert_eq!(jtop(&src, 1)[0], ("big".into(), 1_002));
    }
}
