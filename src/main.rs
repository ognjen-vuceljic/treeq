mod detect;
mod json_tree;
mod render;
mod xml_tree;

use clap::Parser;
use detect::{detect_format, Format};
use json_tree::{find_json_path, JsonNode};
use render::{render_json, render_xml};
use std::io::Read;
use std::path::PathBuf;
use std::{fs, io, process};
use xml_tree::{find_xml_path, XmlNode};

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
}

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

fn run_json(input: &str, args: &Args) {
    let value: serde_json::Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid JSON: {e}");
            process::exit(1);
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
    print!("{}", render_json(target, "root", args.depth));
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
    print!("{}", render_xml(target, args.depth));
}

fn main() {
    let args = Args::parse();
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
