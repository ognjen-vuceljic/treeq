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
use xml_tree::{XmlNode, find_xml_path};

#[derive(clap::ValueEnum, Clone, Copy)]
enum FormatArg {
    Json,
    Xml,
    Yaml,
}

#[derive(Parser)]
#[command(name = "treeq")]
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
}

/// Default tree-render depth used by `--agent` when the user hasn't
/// explicitly passed `--depth`.
const AGENT_DEFAULT_DEPTH: usize = 3;

fn read_input(file: &Option<PathBuf>) -> io::Result<String> {
    match file {
        Some(path) => fs::read_to_string(path),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
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

/// Detects and rejects multi-document YAML by matching serde_yaml's own
/// error text, since `Deserializer::from_str(..).count()` (the structural
/// way to detect multiple `---`-separated documents) hangs on certain
/// malformed single-document input in serde_yaml 0.9. This message is not a
/// stable API: if a future serde_yaml release rewords it, this check stops
/// matching and the raw underlying error is shown instead of our friendlier
/// one — `reports_multi_document_yaml_as_unsupported` guards against that
/// regression going unnoticed on a dependency bump.
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
            render_json(target, "root", depth, args.array_limit, use_color())
        );
        return;
    }
    if args.paths {
        for p in json_paths(target) {
            println!("{p}");
        }
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
    if !args.r#static && io::stdout().is_terminal() {
        tui::run_json_tui(target, use_color()).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        });
    } else {
        print!(
            "{}",
            render_json(target, "root", args.depth, args.array_limit, use_color())
        );
    }
}

fn run_xml(input: &str, args: &Args) {
    let doc = match roxmltree::Document::parse(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}", xml_error_context(input, &e));
            process::exit(1);
        }
    };
    let tree = XmlNode::from_document(&doc);
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
        print!("{}", render_xml(target, depth, use_color()));
        return;
    }
    if args.paths {
        for p in xml_paths(target) {
            println!("{p}");
        }
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
    if !args.r#static && io::stdout().is_terminal() {
        tui::run_xml_tui(target, use_color()).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        });
    } else {
        print!("{}", render_xml(target, args.depth, use_color()));
    }
}

fn main() {
    let args = Args::parse();
    if args.ndjson && matches!(args.format, Some(FormatArg::Xml)) {
        eprintln!("error: --ndjson is not supported with --format xml");
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
