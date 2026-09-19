mod clipboard;
mod color;
mod detect;
mod json_tree;
mod ndjson;
mod paths;
mod render;
mod stats;
mod tui;
mod xml_tree;

use clap::Parser;
use detect::{Format, detect_format};
use json_tree::{JsonNode, find_json_path};
use ndjson::parse_ndjson;
use paths::{json_paths, xml_paths};
use render::{render_json, render_xml};
use stats::{JsonStats, XmlStats, json_stats, xml_stats};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::{fs, io, process};
use xml_tree::{XmlNode, find_xml_path};

#[derive(clap::ValueEnum, Clone, Copy)]
enum FormatArg {
    Json,
    Xml,
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
                eprintln!("error: invalid JSON: {e}");
                process::exit(1);
            }
        }
    };
    let tree = JsonNode::from_value(&value);
    let target = match &args.path {
        Some(p) => match find_json_path(&tree, p) {
            Ok(t) => t,
            Err(seg) => {
                eprintln!("error: path segment '{seg}' not found");
                process::exit(1);
            }
        },
        None => &tree,
    };
    if args.agent {
        let s = json_stats(target);
        print_json_stats(&s, input.len());
        println!();
        let depth = args.depth.or(Some(AGENT_DEFAULT_DEPTH));
        print!("{}", render_json(target, "root", depth, use_color()));
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
    if !args.r#static && io::stdout().is_terminal() {
        tui::run_json_tui(target, use_color()).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        });
    } else {
        print!("{}", render_json(target, "root", args.depth, use_color()));
    }
}

fn run_xml(input: &str, args: &Args) {
    let doc = match roxmltree::Document::parse(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: invalid XML: {e}");
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
    let format = args
        .format
        .map(|f| match f {
            FormatArg::Json => Format::Json,
            FormatArg::Xml => Format::Xml,
        })
        .or_else(|| detect_format(&input))
        .unwrap_or_else(|| {
            eprintln!("error: empty input");
            process::exit(1);
        });
    match format {
        Format::Json => run_json(&input, &args),
        Format::Xml => run_xml(&input, &args),
    }
}
