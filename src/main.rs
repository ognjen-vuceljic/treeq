mod clipboard;
mod color;
mod detect;
mod error_context;
mod json_tree;
mod ndjson;
mod paths;
mod render;
mod schema;
mod stats;
mod tui;
mod xml_tree;

use clap::Parser;
use detect::{Format, detect_format};
use error_context::{json_error_context, xml_error_context};
use json_tree::{JsonNode, find_json_path};
use ndjson::parse_ndjson;
use paths::{json_paths, xml_paths};
use render::{render_json, render_xml};
use schema::{json_schema, xml_schema};
use stats::{JsonStats, XmlStats, json_stats, xml_stats};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::{fs, io, process};
use xml_tree::{
    MAX_XML_ATTRIBUTES_PER_ELEMENT, MAX_XML_DEPTH, XmlNode, find_xml_path_or_attr,
    xml_attribute_count_exceeds, xml_nesting_exceeds,
};

#[derive(clap::ValueEnum, Clone, Copy)]
enum FormatArg {
    Json,
    Xml,
    Yaml,
}

#[derive(Parser)]
#[command(name = "treeq", version)]
struct Args {
    file: Option<PathBuf>,
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    depth: Option<usize>,
    #[arg(long)]
    array_limit: Option<usize>,
    #[arg(long)]
    r#static: bool,
    #[arg(long, value_enum)]
    format: Option<FormatArg>,
    // Mutually exclusive: each of these picks a different one-shot output
    // mode, and `run_json_tree`/`run_xml_tree` only ever check them in one
    // fixed order, so combining them used to silently pick whichever came
    // first in that order instead of erroring (issue #129).
    #[arg(long, conflicts_with_all = ["paths", "schema", "agent"])]
    stats: bool,
    #[arg(long, conflicts_with_all = ["schema", "agent"])]
    paths: bool,
    #[arg(long, conflicts_with_all = ["schema"])]
    agent: bool,
    #[arg(long)]
    ndjson: bool,
    /// Machine-readable output for --stats, --paths, --schema and --agent.
    #[arg(long)]
    json: bool,
    #[arg(long)]
    schema: bool,
    /// Interactive picker: Enter prints the selected path (a jq filter for
    /// JSON, the dotted path otherwise) to stdout and exits, instead of
    /// leaving the TUI a dead end. Renders over /dev/tty so stdout stays
    /// clean for piping/command substitution.
    #[arg(long)]
    pick: bool,
    /// Print a completion script for the given shell to stdout and exit.
    #[arg(long, value_enum)]
    generate: Option<clap_complete::Shell>,
}

const AGENT_DEFAULT_DEPTH: usize = 3;

fn read_input(file: &Option<PathBuf>) -> io::Result<String> {
    let buf = match file {
        Some(path) => fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    // A leading UTF-8 BOM isn't whitespace, so it would otherwise defeat
    // format detection and every parser (issue #121).
    Ok(match buf.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => buf,
    })
}

/// Buffered, so large `--paths` output isn't one `write(2)` per line
/// (issue #128). A closed pipe (`treeq --paths f | head`) exits quietly.
fn print_lines(lines: Vec<String>) {
    use std::io::Write;
    let mut out = io::BufWriter::new(io::stdout().lock());
    let res = lines
        .iter()
        .try_for_each(|l| writeln!(out, "{l}"))
        .and_then(|()| out.flush());
    if let Err(e) = res
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

/// `print!` panics on a closed pipe (`treeq --static f | head`); this exits
/// quietly like `print_lines`.
fn print_text(text: &str) {
    use std::io::Write;
    let mut out = io::stdout().lock();
    if let Err(e) = out.write_all(text.as_bytes()).and_then(|()| out.flush())
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

const PROGRESS_MIN_BYTES: usize = 5 * 1024 * 1024;

/// "parsing 44 MB…" for big inputs, so the blank wait before the TUI opens
/// doesn't look like a hang (issue #149).
fn progress_message(len: usize) -> Option<String> {
    (len >= PROGRESS_MIN_BYTES).then(|| format!("parsing {} MB…", len / (1024 * 1024)))
}

fn show_progress(input_len: usize, args: &Args) {
    let tui_mode = !args.r#static
        && !(args.stats || args.paths || args.schema || args.agent)
        && (args.pick || io::stdout().is_terminal());
    if tui_mode
        && io::stderr().is_terminal()
        && let Some(msg) = progress_message(input_len)
    {
        eprint!("{msg}");
    }
}

/// Wipes the message `show_progress` wrote, before the alternate screen opens.
fn clear_progress(input_len: usize) {
    if progress_message(input_len).is_some() && io::stderr().is_terminal() {
        eprint!("\r\x1b[K");
    }
}

fn use_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn json_stat_pairs(s: &JsonStats, input_len: usize) -> Vec<(&'static str, usize)> {
    vec![
        ("file_bytes", input_len),
        ("max_depth", s.max_depth),
        ("objects", s.objects),
        ("arrays", s.arrays),
        ("scalars", s.scalars),
    ]
}

fn xml_stat_pairs(s: &XmlStats, input_len: usize) -> Vec<(&'static str, usize)> {
    vec![
        ("file_bytes", input_len),
        ("max_depth", s.max_depth),
        ("elements", s.elements),
        ("attributes", s.attributes),
        ("text_nodes", s.text_nodes),
    ]
}

fn stats_json(pairs: &[(&'static str, usize)]) -> serde_json::Value {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), (*v).into()))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

fn print_stats(pairs: &[(&'static str, usize)], json: bool) {
    if json {
        println!("{}", stats_json(pairs));
    } else {
        for (k, v) in pairs {
            println!("{k}: {v}");
        }
    }
}

fn print_schema(text: String, json: bool) {
    if json {
        println!("{}", serde_json::json!({ "schema": text }));
    } else {
        print_text(&text);
    }
}

fn print_agent(pairs: &[(&'static str, usize)], tree: String, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({ "stats": stats_json(pairs), "tree": tree })
        );
    } else {
        print_stats(pairs, false);
        println!();
        print_text(&tree);
    }
}

fn agent_color(args: &Args) -> color::ColorMode {
    if args.json {
        color::ColorMode::Off
    } else {
        color::ColorMode::detect(use_color())
    }
}

fn print_paths(lines: Vec<String>, json: bool) {
    if json {
        println!("{}", serde_json::json!(lines));
    } else {
        print_lines(lines);
    }
}

fn run_json(input: &str, args: &Args, ndjson: bool) {
    let value: serde_json::Value = if ndjson {
        parse_ndjson(input).unwrap_or_else(|e| {
            eprintln!("error: invalid NDJSON: {e}");
            process::exit(1);
        })
    } else {
        match serde_json::from_str(input) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}", json_error_context(input, &e));
                process::exit(1);
            }
        }
    };
    let tree = JsonNode::from_value(&value);
    run_json_tree(&tree, input, args);
}

/// Multi-document YAML (`---` separated) becomes an array of documents, like
/// NDJSON. Detected by matching serde_yaml's error text (its structural
/// `Deserializer::count()` hangs on some malformed input in 0.9), then split
/// on `---` lines. `multi_document_yaml_becomes_an_array_of_documents` guards
/// against a wording change breaking the detection.
fn parse_yaml_documents(input: &str) -> Result<serde_yaml::Value, String> {
    let single = serde_yaml::from_str::<serde_yaml::Value>(input);
    let err = match single {
        Ok(v) => return Ok(v),
        Err(e) => e.to_string(),
    };
    if !err.contains("more than one document") {
        return Err(err);
    }
    let mut docs = Vec::new();
    let mut chunk = String::new();
    for line in input.lines().chain(std::iter::once("---")) {
        if line == "---" || line.starts_with("--- ") {
            if !chunk.trim().is_empty() {
                docs.push(serde_yaml::from_str(&chunk).map_err(|e| e.to_string())?);
            }
            chunk.clear();
            if let Some(rest) = line.strip_prefix("--- ") {
                chunk.push_str(rest);
                chunk.push('\n');
            }
        } else {
            chunk.push_str(line);
            chunk.push('\n');
        }
    }
    Ok(serde_yaml::Value::Sequence(docs))
}

fn run_yaml(input: &str, args: &Args) {
    let value = parse_yaml_documents(input).unwrap_or_else(|e| {
        eprintln!("error: invalid YAML: {e}");
        process::exit(1);
    });
    let tree = JsonNode::from_yaml_value(&value).unwrap_or_else(|e| {
        eprintln!("error: invalid YAML: {e}");
        process::exit(1);
    });
    run_json_tree(&tree, input, args);
}

fn run_json_tree(tree: &JsonNode, input: &str, args: &Args) {
    let target = match &args.path {
        Some(p) => match find_json_path(tree, p) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: {}", e.message());
                process::exit(1);
            }
        },
        None => tree,
    };
    if args.agent {
        let s = json_stats(target);
        let depth = args.depth.or(Some(AGENT_DEFAULT_DEPTH));
        let tree = render_json(target, "root", depth, args.array_limit, agent_color(args));
        print_agent(&json_stat_pairs(&s, input.len()), tree, args.json);
        return;
    }
    if args.paths {
        print_paths(json_paths(target), args.json);
        return;
    }
    if args.stats {
        let s = json_stats(target);
        print_stats(&json_stat_pairs(&s, input.len()), args.json);
        return;
    }
    if args.schema {
        print_schema(json_schema(target), args.json);
        return;
    }
    if !args.r#static && (args.pick || io::stdout().is_terminal()) {
        // Renders over /dev/tty in pick mode, so stdout's own terminal-ness
        // doesn't gate whether color looks right there.
        let use_color = if args.pick {
            std::env::var_os("NO_COLOR").is_none()
        } else {
            use_color()
        };
        clear_progress(input.len());
        match tui::run_json_tui(target, use_color, args.pick) {
            Ok(Some(picked)) => println!("{picked}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    } else {
        print_text(&render_json(
            target,
            "root",
            args.depth,
            args.array_limit,
            color::ColorMode::detect(use_color()),
        ));
    }
}

fn run_xml(input: &str, args: &Args) {
    if xml_nesting_exceeds(input, MAX_XML_DEPTH) {
        eprintln!("error: XML nesting exceeds max depth ({MAX_XML_DEPTH})");
        process::exit(1);
    }
    if xml_attribute_count_exceeds(input, MAX_XML_ATTRIBUTES_PER_ELEMENT) {
        eprintln!(
            "error: an XML element exceeds the max attribute count ({MAX_XML_ATTRIBUTES_PER_ELEMENT})"
        );
        process::exit(1);
    }
    let doc = match roxmltree::Document::parse(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}", xml_error_context(input, &e));
            process::exit(1);
        }
    };
    let tree = XmlNode::from_document(&doc).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let target = match &args.path {
        Some(p) => match find_xml_path_or_attr(&tree, p) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: {}", e.message());
                process::exit(1);
            }
        },
        None => std::borrow::Cow::Borrowed(&tree),
    };
    let target: &XmlNode = &target;
    if args.agent {
        let s = xml_stats(target);
        let depth = args.depth.or(Some(AGENT_DEFAULT_DEPTH));
        let tree = render_xml(target, depth, agent_color(args));
        print_agent(&xml_stat_pairs(&s, input.len()), tree, args.json);
        return;
    }
    if args.paths {
        print_paths(xml_paths(target), args.json);
        return;
    }
    if args.stats {
        let s = xml_stats(target);
        print_stats(&xml_stat_pairs(&s, input.len()), args.json);
        return;
    }
    if args.schema {
        print_schema(xml_schema(target), args.json);
        return;
    }
    if !args.r#static && (args.pick || io::stdout().is_terminal()) {
        let use_color = if args.pick {
            std::env::var_os("NO_COLOR").is_none()
        } else {
            use_color()
        };
        clear_progress(input.len());
        match tui::run_xml_tui(target, use_color, args.pick) {
            Ok(Some(picked)) => println!("{picked}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    } else {
        print_text(&render_xml(
            target,
            args.depth,
            color::ColorMode::detect(use_color()),
        ));
    }
}

fn main() {
    let args = Args::parse();
    if let Some(shell) = args.generate {
        let mut cmd = <Args as clap::CommandFactory>::command();
        let name = cmd.get_name().to_string();
        clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
        return;
    }
    if args.ndjson && matches!(args.format, Some(FormatArg::Xml)) {
        eprintln!("error: --ndjson is not supported with --format xml");
        process::exit(1);
    }
    if args.json && !(args.stats || args.paths || args.schema || args.agent) {
        eprintln!("error: --json needs one of --stats, --paths, --schema or --agent");
        process::exit(1);
    }
    if args.pick && args.r#static {
        eprintln!("error: --pick is not supported with --static");
        process::exit(1);
    }
    let input = read_input(&args.file).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    show_progress(input.len(), &args);
    if args.ndjson {
        run_json(&input, &args, true);
        return;
    }
    let format = args
        .format
        .map(|f| match f {
            FormatArg::Json => Format::Json,
            FormatArg::Xml => Format::Xml,
            FormatArg::Yaml => Format::Yaml,
        })
        .or_else(|| detect_format(&input, args.file.as_deref()))
        .unwrap_or_else(|| {
            eprintln!("error: empty input");
            process::exit(1);
        });
    if args.array_limit.is_some() && matches!(format, Format::Xml) {
        eprintln!("error: --array-limit is not supported for XML input");
        process::exit(1);
    }
    match format {
        Format::Json => run_json(&input, &args, false),
        Format::Xml => run_xml(&input, &args),
        Format::Yaml => run_yaml(&input, &args),
        Format::Ndjson => run_json(&input, &args, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_message_only_for_large_inputs() {
        assert_eq!(progress_message(1024), None);
        assert_eq!(
            progress_message(44 * 1024 * 1024 + 5).as_deref(),
            Some("parsing 44 MB…")
        );
    }
}
