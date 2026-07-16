use roxmltree::{Document, Node, NodeType};

use crate::mhn_matrix::MhnMatrix;
use crate::siteswap;

pub(crate) const CURRENT_JML_VERSION: &str = "3";
const JML_TAGS: [&str; 16] = [
    "jml",
    "pattern",
    "patternlist",
    "title",
    "info",
    "basepattern",
    "prop",
    "setup",
    "symmetry",
    "event",
    "throw",
    "catch",
    "softcatch",
    "line",
    "holding",
    "position",
];

#[derive(Clone, Debug, PartialEq)]
pub struct PatternLibrary {
    pub title: Option<String>,
    pub info: Option<String>,
    pub records: Vec<PatternRecord>,
    pub is_pattern_list: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatternRecord {
    pub display: String,
    pub notation: Option<String>,
    pub config: Option<String>,
    pub animprefs: Option<String>,
    pub info: Option<String>,
    pub tags: Vec<String>,
    pub raw_pattern: Option<String>,
}

impl PatternRecord {
    pub fn siteswap(display: impl Into<String>, config: impl Into<String>) -> Self {
        Self {
            display: display.into(),
            notation: Some("siteswap".to_string()),
            config: Some(config.into()),
            animprefs: None,
            info: None,
            tags: Vec::new(),
            raw_pattern: None,
        }
    }

    pub fn is_playable(&self) -> bool {
        self.notation.is_some() && (self.config.is_some() || self.raw_pattern.is_some())
    }
}

pub fn parse_jml(xml: &str) -> Result<PatternLibrary, String> {
    let cleaned = strip_doctype(xml);
    let doc = Document::parse(&cleaned).map_err(|err| format!("Invalid XML/JML: {err}"))?;
    let root = validate_jml_tree(&doc)?;

    if let Some(patternlist) = root
        .children()
        .find(|node| node.has_tag_name("patternlist"))
    {
        parse_pattern_list(patternlist)
    } else if let Some(pattern) = root.children().find(|node| node.has_tag_name("pattern")) {
        let title = child_text(pattern, "title");
        Ok(PatternLibrary {
            title: title.clone(),
            info: child_text(pattern, "info"),
            records: vec![PatternRecord {
                display: title.unwrap_or_else(|| "Imported JML pattern".to_string()),
                notation: Some("jml".to_string()),
                config: child(pattern, "basepattern").and_then(|node| normalized_text(node)),
                animprefs: None,
                info: child_text(pattern, "info"),
                tags: child(pattern, "info")
                    .and_then(|node| node.attribute("tags"))
                    .map(split_tags)
                    .unwrap_or_default(),
                raw_pattern: Some(serialize_node(pattern)),
            }],
            is_pattern_list: false,
        })
    } else {
        Err("The JML file does not contain a pattern or patternlist".to_string())
    }
}

pub(crate) fn validate_jml_tree<'a, 'input>(
    doc: &'a Document<'input>,
) -> Result<Node<'a, 'input>, String> {
    let root = doc.root_element();
    if !root.has_tag_name("jml") {
        return Err("Missing <jml> tag".to_string());
    }

    for node in root.descendants().filter(Node::is_element) {
        let tag = node.tag_name().name();
        if !JML_TAGS.contains(&tag) {
            let position = doc.text_pos_at(node.range().start);
            return Err(format!("Unknown JML tag '{tag}' at line {}", position.row));
        }
    }

    let version = root.attribute("version").unwrap_or(CURRENT_JML_VERSION);
    if compare_versions(version, CURRENT_JML_VERSION)? == std::cmp::Ordering::Greater {
        return Err(format!(
            "JML version {version} is newer than supported version {CURRENT_JML_VERSION}"
        ));
    }
    Ok(root)
}

fn compare_versions(left: &str, right: &str) -> Result<std::cmp::Ordering, String> {
    let parse = |version: &str| {
        version
            .split('.')
            .map(|component| {
                component
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid JML version '{version}'"))
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let left = parse(left)?;
    let right = parse(right)?;
    for index in 0..left.len().max(right.len()) {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => {}
            ordering => return Ok(ordering),
        }
    }
    Ok(std::cmp::Ordering::Equal)
}

pub fn extract_pattern_xml(xml: &str) -> Result<String, String> {
    let source = xml.trim_start();
    let cleaned = if source.starts_with("<pattern") {
        format!("<jml version=\"3\">{source}</jml>")
    } else {
        strip_doctype(source)
    };
    let doc = Document::parse(&cleaned).map_err(|err| format!("Invalid XML/JML: {err}"))?;
    let pattern = doc
        .descendants()
        .find(|node| node.has_tag_name("pattern"))
        .ok_or_else(|| "Missing <pattern> tag".to_string())?;
    Ok(serialize_node(pattern))
}

pub fn write_pattern_list(title: &str, records: &[PatternRecord]) -> String {
    write_pattern_list_document(Some(title), None, records)
}

pub fn write_pattern_list_document(
    title: Option<&str>,
    info: Option<&str>,
    records: &[PatternRecord],
) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n");
    out.push_str("<!DOCTYPE jml SYSTEM \"file://jml.dtd\">\n");
    out.push_str("<jml version=\"3\">\n");
    out.push_str("<patternlist>\n");
    if let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) {
        out.push_str(&format!("<title>{}</title>\n", escape_xml(title)));
    }
    if let Some(info) = info.map(str::trim).filter(|info| !info.is_empty()) {
        out.push_str(&format!("<info>{}</info>\n", escape_xml(info)));
    }

    for record in records {
        out.push('\n');
        out.push_str(&format!(
            "<line display=\"{}\"",
            escape_xml(record.display.trim_end())
        ));
        if let Some(notation) = &record.notation {
            out.push_str(&format!(
                " notation=\"{}\"",
                escape_xml(&notation.to_lowercase())
            ));
        }
        if let Some(animprefs) = &record.animprefs {
            out.push_str(&format!(" animprefs=\"{}\"", escape_xml(animprefs)));
        }
        if record.notation.is_none() && record.animprefs.is_none() {
            out.push_str("/>\n");
            continue;
        }
        out.push('>');

        match record.notation.as_deref() {
            Some(notation) if notation.eq_ignore_ascii_case("jml") => {
                out.push('\n');
                if let Some(raw) = &record.raw_pattern {
                    if raw.trim_start().starts_with("<jml") {
                        match extract_pattern_xml(raw) {
                            Ok(pattern) => out.push_str(pattern.trim()),
                            Err(_) => out.push_str(raw.trim()),
                        }
                    } else {
                        out.push_str(raw.trim());
                    }
                    out.push('\n');
                } else if let Some(config) = &record.config {
                    out.push_str(&escape_xml(config));
                    out.push('\n');
                }
            }
            Some(_) => {
                out.push('\n');
                if let Some(config) = &record.config {
                    out.push_str(&escape_xml(config.trim()));
                    out.push('\n');
                }
                if record.info.is_some() || !record.tags.is_empty() {
                    let tags = record.tags.join(",");
                    match (&record.info, tags.is_empty()) {
                        (Some(info), true) => {
                            out.push_str(&format!("<info>{}</info>\n", escape_xml(info)));
                        }
                        (Some(info), false) => {
                            out.push_str(&format!(
                                "<info tags=\"{}\">{}</info>\n",
                                escape_xml(&tags),
                                escape_xml(info)
                            ));
                        }
                        (None, false) => {
                            out.push_str(&format!("<info tags=\"{}\"/>\n", escape_xml(&tags)));
                        }
                        (None, true) => {}
                    }
                }
            }
            None => {}
        }
        out.push_str("</line>\n");
    }

    out.push_str("</patternlist>\n</jml>\n");
    out
}

pub fn write_pattern_list_text(records: &[PatternRecord]) -> String {
    let mut out = records
        .iter()
        .map(|record| record.display.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !records.is_empty() {
        out.push('\n');
    }
    out
}

pub fn record_to_pattern_jml(record: &PatternRecord) -> Result<String, String> {
    if record
        .notation
        .as_deref()
        .is_some_and(|notation| notation.eq_ignore_ascii_case("jml"))
    {
        if let Some(raw) = &record.raw_pattern {
            if raw.trim_start().starts_with("<jml") {
                return Ok(raw.clone());
            }
            return Ok(format!(
                "<?xml version=\"1.0\"?>\n<!DOCTYPE jml SYSTEM \"file://jml.dtd\">\n<jml version=\"3\">\n{}\n</jml>\n",
                raw.trim()
            ));
        }
    }

    let config = record
        .config
        .as_deref()
        .ok_or_else(|| "Current pattern has no exportable config".to_string())?;
    let spec = siteswap::parse_config(config)?;
    let mut matrix = MhnMatrix::from_siteswap(&spec)?;
    let mut model = matrix.to_jml_pattern(&spec)?;
    model.info = record.info.clone();
    model.tags = record.tags.clone();
    model.assert_valid()?;
    Ok(model.write_jml(true, true))
}

pub fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse_pattern_list(patternlist: Node) -> Result<PatternLibrary, String> {
    let mut title = None;
    let mut info = None;
    let mut records = Vec::new();

    for node in patternlist.children().filter(|node| node.is_element()) {
        if node.has_tag_name("title") {
            title = normalized_text(node);
        } else if node.has_tag_name("info") {
            info = normalized_text(node);
        } else if node.has_tag_name("line") {
            records.push(parse_line(node)?);
        }
    }

    Ok(PatternLibrary {
        title,
        info,
        records,
        is_pattern_list: true,
    })
}

fn parse_line(line: Node) -> Result<PatternRecord, String> {
    let display = line.attribute("display").unwrap_or_default().to_string();
    let notation = line.attribute("notation").map(str::to_string);
    let animprefs = line.attribute("animprefs").map(str::to_string);
    let info_node = child(line, "info");
    let info = info_node.and_then(normalized_text);
    let tags = info_node
        .and_then(|node| node.attribute("tags"))
        .map(split_tags)
        .unwrap_or_default();

    let mut raw_pattern = None;
    let config = if notation
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("jml"))
    {
        if let Some(pattern) = child(line, "pattern") {
            raw_pattern = Some(serialize_node(pattern));
            child(pattern, "basepattern").and_then(normalized_text)
        } else {
            normalized_text(line)
        }
    } else {
        direct_text(line)
    };

    Ok(PatternRecord {
        display,
        notation,
        config,
        animprefs,
        info,
        tags,
        raw_pattern,
    })
}

fn child<'a>(node: Node<'a, 'a>, tag: &str) -> Option<Node<'a, 'a>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name().eq_ignore_ascii_case(tag))
}

fn child_text(node: Node, tag: &str) -> Option<String> {
    child(node, tag).and_then(normalized_text)
}

fn direct_text(node: Node) -> Option<String> {
    let text = node
        .children()
        .filter(|child| child.node_type() == NodeType::Text)
        .filter_map(|child| child.text())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();

    (!text.is_empty()).then_some(text)
}

fn normalized_text(node: Node) -> Option<String> {
    let text = node.text()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn split_tags(tags: &str) -> Vec<String> {
    let mut result = Vec::<String>::new();
    for tag in tags.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
        if result
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(tag))
        {
            continue;
        }
        result.push(tag.to_string());
    }
    result
}

fn strip_doctype(xml: &str) -> String {
    xml.lines()
        .filter(|line| !line.trim_start().starts_with("<!DOCTYPE"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn serialize_node(node: Node) -> String {
    match node.node_type() {
        NodeType::Root => node.children().map(serialize_node).collect::<String>(),
        NodeType::Element => {
            let mut out = String::new();
            out.push('<');
            out.push_str(node.tag_name().name());
            for attr in node.attributes() {
                out.push(' ');
                out.push_str(attr.name());
                out.push_str("=\"");
                out.push_str(&escape_xml(attr.value()));
                out.push('"');
            }

            let children = node.children().collect::<Vec<_>>();
            if children.is_empty() {
                out.push_str("/>");
            } else {
                out.push('>');
                for child in children {
                    out.push_str(&serialize_node(child));
                }
                out.push_str("</");
                out.push_str(node.tag_name().name());
                out.push('>');
            }
            out
        }
        NodeType::Text => escape_xml(node.text().unwrap_or_default()),
        NodeType::Comment => format!("<!--{}-->", node.text().unwrap_or_default()),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pattern_list() {
        let xml = r#"<?xml version="1.0"?>
<jml version="3"><patternlist><title>Test</title>
<line display="3" notation="siteswap">pattern=3</line>
</patternlist></jml>"#;
        let library = parse_jml(xml).unwrap();
        assert!(library.is_pattern_list);
        assert_eq!(library.title.as_deref(), Some("Test"));
        assert_eq!(library.records.len(), 1);
        assert_eq!(library.records[0].config.as_deref(), Some("pattern=3"));
    }

    #[test]
    fn pattern_list_document_round_trips_text_lines_and_metadata() {
        let records = vec![
            PatternRecord {
                display: "Section".to_string(),
                notation: None,
                config: None,
                animprefs: None,
                info: None,
                tags: Vec::new(),
                raw_pattern: None,
            },
            PatternRecord::siteswap("Cascade", "pattern=3"),
        ];

        let xml = write_pattern_list_document(Some("Favorites"), Some("Personal list"), &records);
        assert!(xml.contains("<line display=\"Section\"/>"));
        assert!(xml.contains("<info>Personal list</info>"));

        let parsed = parse_jml(&xml).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("Favorites"));
        assert_eq!(parsed.info.as_deref(), Some("Personal list"));
        assert_eq!(parsed.records, records);
        assert_eq!(write_pattern_list_text(&records), "Section\nCascade\n");
    }

    #[test]
    fn extracts_pattern_fragment_from_full_jml() {
        let xml = r#"
        <?xml version="1.0"?>
        <!DOCTYPE jml SYSTEM "file://jml.dtd">
        <jml version="3">
        <pattern>
        <title>Fragment</title>
        <setup jugglers="1" paths="1"/>
        </pattern>
        </jml>
        "#;

        let fragment = extract_pattern_xml(xml).unwrap();

        assert!(fragment.trim_start().starts_with("<pattern>"));
        assert!(fragment.contains("<title>Fragment</title>"));
        assert!(!fragment.contains("<jml"));
    }

    #[test]
    fn rejects_unknown_tags_and_future_versions() {
        let unknown = r#"<jml version="3"><pattern><widget/></pattern></jml>"#;
        assert!(
            parse_jml(unknown)
                .unwrap_err()
                .contains("Unknown JML tag 'widget'")
        );

        let future = r#"<jml version="4"><pattern/></jml>"#;
        assert!(
            parse_jml(future)
                .unwrap_err()
                .contains("newer than supported")
        );
    }

    #[test]
    fn accepts_equivalent_current_version_spelling() {
        let xml = r#"<jml version="3.0"><pattern><title>Versioned</title></pattern></jml>"#;
        let library = parse_jml(xml).unwrap();
        assert_eq!(library.title.as_deref(), Some("Versioned"));
    }

    #[test]
    fn bundled_jml_libraries_round_trip_without_losing_records() {
        let patterns_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../patterns");
        let mut fixtures = std::fs::read_dir(&patterns_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "jml"))
            .collect::<Vec<_>>();
        fixtures.sort();
        assert_eq!(fixtures.len(), 19);

        for path in fixtures {
            let xml = std::fs::read_to_string(&path).unwrap();
            let parsed = parse_jml(&xml)
                .unwrap_or_else(|err| panic!("{} did not parse: {err}", path.display()));
            let written = write_pattern_list_document(
                parsed.title.as_deref(),
                parsed.info.as_deref(),
                &parsed.records,
            );
            let reparsed = parse_jml(&written)
                .unwrap_or_else(|err| panic!("{} did not reparse: {err}", path.display()));
            assert_eq!(
                reparsed,
                parsed,
                "{} changed after a structured round trip",
                path.display()
            );
        }
    }

    #[test]
    fn record_serialization_is_core_juggling_logic() {
        let record = PatternRecord::siteswap("Cascade", "pattern=3");
        let xml = record_to_pattern_jml(&record).unwrap();

        assert!(xml.contains("<jml version=\"3\">"));
        assert!(xml.contains("<basepattern notation=\"siteswap\">"));
        assert!(xml.contains("pattern=3"));
        assert!(parse_jml(&xml).is_ok());
    }
}
