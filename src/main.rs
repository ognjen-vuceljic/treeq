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
    MAX_XML_ATTRIBUTES_PER_ELEMENT, MAX_XML_DEPTH, XmlNode, find_xml_path,
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
    #[arg(long)]
    stats: bool,
    #[arg(long)]
    paths: bool,
    #[arg(long)]
    agent: bool,
    #[arg(long)]
    ndjson: bool,
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

fn use_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn print_json_stats(s: &JsonStats, input_len: usize) {
    println!("input_bytes: {input_len}");
    println!("max_depth: {}", s.max_depth);
    println!("objects: {}", s.objects);
    println!("arrays: {}", s.arrays);
    println!("scalars: {}", s.scalars);
}

fn print_xml_stats(s: &XmlStats, input_len: usize) {
    println!("input_bytes: {input_len}");
    println!("max_depth: {}", s.max_depth);
    println!("elements: {}", s.elements);
    println!("attributes: {}", s.attributes);
    println!("text_nodes: {}", s.text_nodes);
}

fn run_json(input: &str, args: &Args) {
    let value: serde_json::Value = if args.ndjson {
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

/// Detects multi-document YAML by matching serde_yaml's error text: the
/// structural check (`Deserializer::from_str(..).count()`) hangs on some
/// malformed single-document input in serde_yaml 0.9. Not a stable API —
/// `reports_multi_document_yaml_as_unsupported` guards against a wording
/// change silently breaking this.
fn parse_single_yaml_document(input: &str) -> Result<serde_yaml::Value, String> {
    serde_yaml::from_str::<serde_yaml::Value>(input).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("more than one document") {
            "multi-document YAML is not supported yet".to_string()
        } else {
            msg
        }
    })
}

fn run_yaml(input: &str, args: &Args) {
    let value = parse_single_yaml_document(input).unwrap_or_else(|e| {
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
            Err(seg) => {
                eprintln!("error: path segment '{seg}' not found");
                process::exit(1);
            }
        },
        None => tree,
    };
    if args.agent {
        let s = json_stats(target);
        print_json_stats(&s, input.len());
        println!();
        let depth = args.depth.or(Some(AGENT_DEFAULT_DEPTH));
        print!(
            "{}",
            render_json(
                target,
                "root",
                depth,
                args.array_limit,
                color::ColorMode::detect(use_color())
            )
        );
        return;
    }
    if args.paths {
        print_lines(json_paths(target));
        return;
    }
    if args.stats {
        let s = json_stats(target);
        print_json_stats(&s, input.len());
        return;
    }
    if args.schema {
        print!("{}", json_schema(target));
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
        match tui::run_json_tui(target, use_color, args.pick) {
            Ok(Some(picked)) => println!("{picked}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    } else {
        print!(
            "{}",
            render_json(
                target,
                "root",
                args.depth,
                args.array_limit,
                color::ColorMode::detect(use_color())
            )
        );
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
        Some(p) => match find_xml_path(&tree, p) {
            Ok(t) => t,
            Err(seg) => {
                eprintln!("error: path segment '{seg}' not found");
                process::exit(1);
            }
        },
        None => &tree,
    };
    if args.agent {
        let s = xml_stats(target);
        print_xml_stats(&s, input.len());
        println!();
        let depth = args.depth.or(Some(AGENT_DEFAULT_DEPTH));
        print!(
            "{}",
            render_xml(target, depth, color::ColorMode::detect(use_color()))
        );
        return;
    }
    if args.paths {
        print_lines(xml_paths(target));
        return;
    }
    if args.stats {
        let s = xml_stats(target);
        print_xml_stats(&s, input.len());
        return;
    }
    if args.schema {
        print!("{}", xml_schema(target));
        return;
    }
    if !args.r#static && (args.pick || io::stdout().is_terminal()) {
        let use_color = if args.pick {
            std::env::var_os("NO_COLOR").is_none()
        } else {
            use_color()
        };
        match tui::run_xml_tui(target, use_color, args.pick) {
            Ok(Some(picked)) => println!("{picked}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    } else {
        print!(
            "{}",
            render_xml(target, args.depth, color::ColorMode::detect(use_color()))
        );
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
    if args.pick && args.r#static {
        eprintln!("error: --pick is not supported with --static");
        process::exit(1);
    }
    let input = read_input(&args.file).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    if args.ndjson {
        run_json(&input, &args);
        return;
    }
    let format = args
        .format
        .map(|f| match f {
            FormatArg::Json => Format::Json,
            FormatArg::Xml => Format::Xml,
            FormatArg::Yaml => Format::Yaml,
        })
        .or_else(|| detect_format(&input))
        .unwrap_or_else(|| {
            eprintln!("error: empty input");
            process::exit(1);
        });
    if args.array_limit.is_some() && matches!(format, Format::Xml) {
        eprintln!("error: --array-limit is not supported for XML input");
        process::exit(1);
    }
    match format {
        Format::Json => run_json(&input, &args),
        Format::Xml => run_xml(&input, &args),
        Format::Yaml => run_yaml(&input, &args),
    }
}
