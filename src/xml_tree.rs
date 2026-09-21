#[derive(Debug, Clone, PartialEq)]
pub struct XmlNode {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: Option<String>,
    pub children: Vec<XmlNode>,
}

/// Matches serde_json's default recursion limit. Also empirically verified
/// safe for `from_element`'s own recursion on the smaller stack `cargo
/// test` runs worker threads on (500 was not: see git history) --
/// `accepts_xml_nested_exactly_at_the_max_depth` is the regression guard.
pub const MAX_XML_DEPTH: usize = 128;

/// Finds the `>` that actually ends the tag starting at `bytes[start]`
/// (`bytes[start] == b'<'`), skipping over any `>` inside a quoted
/// attribute value (e.g. `<a attr="/>">`) so it isn't mistaken for the
/// tag's own close.
fn find_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut j = start + 1;
    let mut quote: Option<u8> = None;
    while j < bytes.len() {
        let c = bytes[j];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => return Some(j),
            None => {}
        }
        j += 1;
    }
    None
}

/// A lightweight lexical scan for element nesting depth, run *before*
/// handing input to roxmltree: roxmltree's own parser recurses natively per
/// nested element with no depth guard, so it overflows the stack on deeply
/// nested (but otherwise well-formed) input before `XmlNode::from_document`
/// ever gets a chance to run its own check. Not a real XML parser -- a `<`
/// or `>` inside a CDATA section's payload can still throw off the count,
/// but that's fine here: on anything it can't confidently count, the worst
/// case is roxmltree's own parser then reports the real syntax error.
pub fn xml_nesting_exceeds(input: &str, max_depth: usize) -> bool {
    let bytes = input.as_bytes();
    let mut depth: usize = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = find_tag_end(bytes, i) else {
            break;
        };
        let tag = &input[i..=end];
        if tag.starts_with("<!") || tag.starts_with("<?") {
            // comment / doctype / processing instruction: no depth change
        } else if tag.starts_with("</") {
            depth = depth.saturating_sub(1);
        } else {
            let self_closing = bytes[..end]
                .iter()
                .rposition(|&b| !b.is_ascii_whitespace())
                .is_some_and(|p| bytes[p] == b'/');
            if !self_closing {
                depth += 1;
                if depth > max_depth {
                    return true;
                }
            }
        }
        i = end + 1;
    }
    false
}

/// A cap on any single element's attribute count, checked lexically before
/// handing input to roxmltree: roxmltree's attribute parsing is quadratic in
/// the number of attributes on one element (empirically, 500,000 attributes
/// on a single tag hangs for well over a minute), so a document that would
/// trigger that needs to be rejected before roxmltree ever sees it, the same
/// way `xml_nesting_exceeds` pre-empts its unguarded recursion.
pub const MAX_XML_ATTRIBUTES_PER_ELEMENT: usize = 10_000;

/// Counts `=` followed by a quote character (optionally with whitespace in
/// between, since XML's grammar allows `Eq ::= S? '=' S?`), a proxy for
/// attribute assignments that doesn't require a real parser. A quote
/// *inside* an attribute value can make this overcount (e.g. `attr='a="b"'`
/// counts 2, not 1), but must never undercount -- the failure mode of a
/// proxy check like this must be "reject a few more documents than
/// strictly necessary", not "let a genuinely oversized element slip
/// through" (an earlier version that required the quote immediately after
/// `=` did exactly that for `a = "v"`-style spacing).
fn count_attributes(tag: &str) -> usize {
    let bytes = tag.as_bytes();
    let mut count = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'=' {
            continue;
        }
        let after_ws = bytes[i + 1..]
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .map(|offset| i + 1 + offset);
        if let Some(j) = after_ws
            && (bytes[j] == b'"' || bytes[j] == b'\'')
        {
            count += 1;
        }
    }
    count
}

pub fn xml_attribute_count_exceeds(input: &str, max_attributes: usize) -> bool {
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = find_tag_end(bytes, i) else {
            break;
        };
        let tag = &input[i..=end];
        let is_element_tag =
            !tag.starts_with("<!") && !tag.starts_with("<?") && !tag.starts_with("</");
        if is_element_tag && count_attributes(tag) > max_attributes {
            return true;
        }
        i = end + 1;
    }
    false
}

impl XmlNode {
    pub fn from_document(doc: &roxmltree::Document) -> Result<XmlNode, String> {
        Self::from_element(doc.root_element(), 0)
    }

    fn from_element(el: roxmltree::Node, depth: usize) -> Result<XmlNode, String> {
        if depth > MAX_XML_DEPTH {
            return Err(format!("XML nesting exceeds max depth ({MAX_XML_DEPTH})"));
        }
        let attributes = el
            .attributes()
            .map(|a| (a.name().to_string(), a.value().to_string()))
            .collect();
        let children = el
            .children()
            .filter(|n| n.is_element())
            .map(|n| Self::from_element(n, depth + 1))
            .collect::<Result<Vec<_>, _>>()?;
        let text = el
            .children()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(XmlNode {
            name: el.tag_name().name().to_string(),
            attributes,
            text: if text.is_empty() { None } else { Some(text) },
            children,
        })
    }
}

pub fn find_xml_path<'a>(node: &'a XmlNode, path: &str) -> Result<&'a XmlNode, String> {
    let mut current = node;
    for segment in path.split('.') {
        current = current
            .children
            .iter()
            .find(|c| c.name == segment)
            .ok_or_else(|| segment.to_string())?;
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_element_with_attributes_text_and_children() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        assert_eq!(
            node,
            XmlNode {
                name: "person".to_string(),
                attributes: vec![("id".to_string(), "1".to_string())],
                text: None,
                children: vec![XmlNode {
                    name: "name".to_string(),
                    attributes: vec![],
                    text: Some("Alice".to_string()),
                    children: vec![],
                }],
            }
        );
    }

    #[test]
    fn finds_nested_path() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let found = find_xml_path(&node, "user.name").unwrap();
        assert_eq!(found.text, Some("Alice".to_string()));
    }

    #[test]
    fn reports_first_unmatched_segment() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let err = find_xml_path(&node, "user.missing.deeper").unwrap_err();
        assert_eq!(err, "missing");
    }

    #[test]
    fn rejects_xml_nested_past_the_max_depth_instead_of_overflowing_the_stack() {
        let depth = MAX_XML_DEPTH + 10;
        let xml = format!("{}leaf{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let err = XmlNode::from_document(&doc).unwrap_err();
        assert!(err.contains("max depth"));
    }

    #[test]
    fn accepts_xml_nested_exactly_at_the_max_depth() {
        let depth = MAX_XML_DEPTH;
        let xml = format!("{}leaf{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert!(XmlNode::from_document(&doc).is_ok());
    }

    #[test]
    fn nesting_precheck_rejects_input_far_too_deep_for_roxmltree_to_even_parse() {
        let depth = 50_000;
        let xml = format!("{}leaf{}", "<a>".repeat(depth), "</a>".repeat(depth));
        assert!(xml_nesting_exceeds(&xml, MAX_XML_DEPTH));
    }

    #[test]
    fn nesting_precheck_does_not_miscount_self_closing_siblings_as_depth() {
        let flat = "<a/>".repeat(MAX_XML_DEPTH * 2);
        assert!(!xml_nesting_exceeds(&flat, MAX_XML_DEPTH));
    }

    #[test]
    fn nesting_precheck_ignores_comments_and_doctypes() {
        let xml = format!(
            "<!DOCTYPE root><!-- a comment --><root>{}</root>",
            "<a>".repeat(MAX_XML_DEPTH - 2)
        );
        assert!(!xml_nesting_exceeds(&xml, MAX_XML_DEPTH));
    }

    #[test]
    fn nesting_precheck_accepts_shallow_wide_documents() {
        let wide = "<item>x</item>".repeat(10_000);
        assert!(!xml_nesting_exceeds(&wide, MAX_XML_DEPTH));
    }

    #[test]
    fn nesting_precheck_is_not_fooled_by_a_quoted_attribute_ending_in_slash_gt() {
        // A naive `tag.ends_with("/>")` check would misread this as
        // self-closing (the quoted attribute value itself ends in "/>"),
        // undercounting real depth and letting a deep document slip past
        // the guard straight into roxmltree's unguarded parser.
        let depth = MAX_XML_DEPTH + 10;
        let open = "<a attr=\"/>\">".repeat(depth);
        let close = "</a>".repeat(depth);
        let xml = format!("{open}leaf{close}");
        assert!(xml_nesting_exceeds(&xml, MAX_XML_DEPTH));

        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(doc.root_element().attribute("attr"), Some("/>"));
    }

    #[test]
    fn attribute_count_precheck_rejects_an_element_with_too_many_attributes() {
        let attrs: String = (0..MAX_XML_ATTRIBUTES_PER_ELEMENT + 10)
            .map(|i| format!(" a{i}=\"v\""))
            .collect();
        let xml = format!("<root{attrs}/>");
        assert!(xml_attribute_count_exceeds(
            &xml,
            MAX_XML_ATTRIBUTES_PER_ELEMENT
        ));
    }

    #[test]
    fn attribute_count_precheck_is_not_fooled_by_whitespace_around_the_equals_sign() {
        // XML's grammar allows `Eq ::= S? '=' S?`, so `a = "v"` is just as
        // valid as `a="v"`; a version that required the quote immediately
        // after `=` undercounted this form and let it slip past the guard.
        let attrs: String = (0..MAX_XML_ATTRIBUTES_PER_ELEMENT + 10)
            .map(|i| format!(" a{i} = \"v\""))
            .collect();
        let xml = format!("<root{attrs}/>");
        assert!(xml_attribute_count_exceeds(
            &xml,
            MAX_XML_ATTRIBUTES_PER_ELEMENT
        ));
    }

    #[test]
    fn attribute_count_precheck_accepts_an_element_at_exactly_the_limit() {
        let attrs: String = (0..MAX_XML_ATTRIBUTES_PER_ELEMENT)
            .map(|i| format!(" a{i}=\"v\""))
            .collect();
        let xml = format!("<root{attrs}/>");
        assert!(!xml_attribute_count_exceeds(
            &xml,
            MAX_XML_ATTRIBUTES_PER_ELEMENT
        ));
    }

    #[test]
    fn attribute_count_precheck_sums_per_element_not_across_the_whole_document() {
        // Many elements each with a normal attribute count must not trip
        // the guard just because the document as a whole has many attributes.
        let xml: String = (0..MAX_XML_ATTRIBUTES_PER_ELEMENT * 2)
            .map(|i| format!("<item a=\"{i}\"/>"))
            .collect();
        let xml = format!("<root>{xml}</root>");
        assert!(!xml_attribute_count_exceeds(
            &xml,
            MAX_XML_ATTRIBUTES_PER_ELEMENT
        ));
    }
}
