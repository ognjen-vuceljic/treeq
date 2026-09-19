#[derive(Debug, Clone, PartialEq)]
pub struct XmlNode {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: Option<String>,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    pub fn from_document(doc: &roxmltree::Document) -> XmlNode {
        Self::from_element(doc.root_element())
    }

    fn from_element(el: roxmltree::Node) -> XmlNode {
        let attributes = el
            .attributes()
            .map(|a| (a.name().to_string(), a.value().to_string()))
            .collect();
        let children: Vec<XmlNode> = el
            .children()
            .filter(|n| n.is_element())
            .map(Self::from_element)
            .collect();
        let text = el
            .children()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        XmlNode {
            name: el.tag_name().name().to_string(),
            attributes,
            text: if text.is_empty() { None } else { Some(text) },
            children,
        }
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
        let node = XmlNode::from_document(&doc);
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
        let node = XmlNode::from_document(&doc);
        let found = find_xml_path(&node, "user.name").unwrap();
        assert_eq!(found.text, Some("Alice".to_string()));
    }

    #[test]
    fn reports_first_unmatched_segment() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let err = find_xml_path(&node, "user.missing.deeper").unwrap_err();
        assert_eq!(err, "missing");
    }
}
