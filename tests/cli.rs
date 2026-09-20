use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static TMP_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn run_treeq_with_file(args: &[&str], contents: &str) -> (String, String, i32) {
    let n = TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("treeq-cli-test-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("input.txt");
    fs::write(&file, contents).unwrap();
    let full_args: Vec<String> = args
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once(file.to_string_lossy().to_string()))
        .collect();
    let output = Command::new(env!("CARGO_BIN_EXE_treeq"))
        .args(&full_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to start treeq");
    fs::remove_dir_all(&dir).ok();
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn run_treeq(args: &[&str], stdin_data: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_treeq"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start treeq");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_data.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn renders_json_from_stdin() {
    let (stdout, _stderr, code) = run_treeq(&["--static"], r#"{"name": "Alice"}"#);
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n└── name: Alice\n");
}

#[test]
fn renders_xml_from_stdin() {
    let (stdout, _stderr, code) = run_treeq(&["--static"], "<person><name>Alice</name></person>");
    assert_eq!(code, 0);
    assert_eq!(stdout, "person\n└── name: Alice\n");
}

#[test]
fn applies_path_and_depth() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--path", "user"],
        r#"{"user": {"name": "Alice", "age": 30}}"#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n├── name: Alice\n└── age: 30\n");
}

#[test]
fn reports_invalid_json() {
    let (_stdout, stderr, code) = run_treeq(&["--static"], "{not json");
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid JSON"));
}

#[test]
fn prints_json_stats() {
    let (stdout, _stderr, code) = run_treeq(&["--stats"], r#"{"a": {"b": [1, 2]}}"#);
    assert_eq!(code, 0);
    assert!(stdout.contains("max_depth: 3"));
    assert!(stdout.contains("objects: 2"));
    assert!(stdout.contains("arrays: 1"));
    assert!(stdout.contains("scalars: 2"));
}

#[test]
fn lists_json_paths() {
    let (stdout, _stderr, code) = run_treeq(&["--paths"], r#"{"user": {"name": "Alice"}}"#);
    assert_eq!(code, 0);
    assert_eq!(stdout, "user\nuser.name\n");
}

#[test]
fn reports_unresolved_path_segment() {
    let (_stdout, stderr, code) =
        run_treeq(&["--static", "--path", "missing"], r#"{"user": "Alice"}"#);
    assert_eq!(code, 1);
    assert!(stderr.contains("missing"));
}

#[test]
fn reads_input_from_a_file_argument() {
    let (stdout, _stderr, code) = run_treeq_with_file(&["--static"], r#"{"name": "Alice"}"#);
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n└── name: Alice\n");
}

#[test]
fn reports_missing_file() {
    let (_stdout, stderr, code) = run_treeq(&["--static", "/no/such/file.json"], "");
    assert_eq!(code, 1);
    assert!(stderr.contains("error:"));
}

#[test]
fn reports_empty_input() {
    let (_stdout, stderr, code) = run_treeq(&["--static"], "   ");
    assert_eq!(code, 1);
    assert!(stderr.contains("empty input"));
}

#[test]
fn explicit_format_flag_overrides_detection() {
    let (stdout, _stderr, code) =
        run_treeq(&["--static", "--format", "json"], r#"{"name": "Alice"}"#);
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n└── name: Alice\n");

    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--format", "xml"],
        "<person><name>Alice</name></person>",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "person\n└── name: Alice\n");
}

#[test]
fn reports_invalid_xml() {
    let (_stdout, stderr, code) = run_treeq(&["--static"], "<not xml");
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid XML"));
}

#[test]
fn reports_unresolved_xml_path_segment() {
    let (_stdout, stderr, code) = run_treeq(
        &["--static", "--path", "missing"],
        "<person><name>Alice</name></person>",
    );
    assert_eq!(code, 1);
    assert!(stderr.contains("missing"));
}

#[test]
fn lists_xml_paths() {
    let (stdout, _stderr, code) = run_treeq(&["--paths"], "<person><name>Alice</name></person>");
    assert_eq!(code, 0);
    assert_eq!(stdout, "name\n");
}

#[test]
fn prints_xml_stats() {
    let (stdout, _stderr, code) = run_treeq(&["--stats"], "<person><name>Alice</name></person>");
    assert_eq!(code, 0);
    assert!(stdout.contains("elements: 2"));
    assert!(stdout.contains("text_nodes: 1"));
}

#[test]
fn agent_flag_prints_stats_then_tree() {
    let (stdout, _stderr, code) = run_treeq(&["--agent"], r#"{"a": {"b": {"c": {"d": "deep"}}}}"#);
    assert_eq!(code, 0);
    assert!(stdout.contains("max_depth:"));
    assert!(stdout.contains("root"));
    assert!(stdout.contains("\n\n"));
}

#[test]
fn agent_flag_truncates_tree_beyond_default_depth() {
    let nested = r#"{"a": {"b": {"c": {"d": {"e": "deep"}}}}}"#;

    let (default_stdout, _stderr, code) = run_treeq(&["--agent"], nested);
    assert_eq!(code, 0);
    assert!(default_stdout.contains('…'));

    let (deep_stdout, _stderr, code) = run_treeq(&["--agent", "--depth", "10"], nested);
    assert_eq!(code, 0);
    assert!(!deep_stdout.contains('…'));
    assert!(deep_stdout.contains("deep"));
}

#[test]
fn agent_flag_works_on_xml_input() {
    let (stdout, _stderr, code) = run_treeq(&["--agent"], "<root><a><b>1</b></a></root>");
    assert_eq!(code, 0);
    assert!(stdout.contains("elements:"));
    assert!(stdout.contains("root"));
    assert!(stdout.contains("\n\n"));
}

#[test]
fn agent_flag_respects_path_scoping() {
    let (stdout, _stderr, code) = run_treeq(
        &["--agent", "--path", "user"],
        r#"{"user": {"name": "Alice"}, "other": 1}"#,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("name: Alice"));
    assert!(!stdout.contains("other"));
}

#[test]
fn agent_flag_takes_precedence_over_stats_and_paths() {
    let input = r#"{"a": 1}"#;

    let (agent_stats, _stderr, code) = run_treeq(&["--agent", "--stats"], input);
    assert_eq!(code, 0);
    assert!(
        agent_stats.contains("root"),
        "--agent should still render the tree"
    );

    let (agent_paths, _stderr, code) = run_treeq(&["--agent", "--paths"], input);
    assert_eq!(code, 0);
    assert!(
        agent_paths.contains("root") && agent_paths.contains("max_depth:"),
        "--agent must take precedence over --paths too, got: {agent_paths}"
    );
}

#[test]
fn parses_ndjson_input_as_array() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--ndjson"],
        "{\"name\": \"Alice\"}\n{\"name\": \"Bob\"}\n",
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "root\n├── [0]\n│   └── name: Alice\n└── [1]\n    └── name: Bob\n"
    );
}

#[test]
fn reports_invalid_ndjson_line() {
    let (_stdout, stderr, code) = run_treeq(&["--static", "--ndjson"], "{\"a\": 1}\nnot json\n");
    assert_eq!(code, 1);
    assert!(stderr.contains("line 2"));
}

#[test]
fn rejects_ndjson_with_xml_format() {
    let (_stdout, stderr, code) =
        run_treeq(&["--static", "--ndjson", "--format", "xml"], "{\"a\": 1}\n");
    assert_eq!(code, 1);
    assert!(stderr.contains("--ndjson"));
}

#[test]
fn ndjson_ignores_format_auto_detection_even_when_input_looks_like_xml() {
    // A malformed/garbage first line starting with '<' would normally
    // auto-detect as XML; --ndjson must force JSON parsing regardless.
    let (_stdout, stderr, code) = run_treeq(&["--static", "--ndjson"], "<not>\nvalid\n");
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid NDJSON"), "got: {stderr}");
}

#[test]
fn ndjson_empty_input_yields_an_empty_tree_instead_of_an_empty_input_error() {
    let (stdout, stderr, code) = run_treeq(&["--static", "--ndjson"], "");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "root\n");
}

#[test]
fn ndjson_stats_and_path_flags_compose() {
    let (stdout, _stderr, code) = run_treeq(
        &["--stats", "--ndjson"],
        "{\"name\": \"Alice\"}\n{\"name\": \"Bob\"}\n",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("arrays: 1"));

    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--ndjson", "--path", "0"],
        "{\"name\": \"Alice\"}\n{\"name\": \"Bob\"}\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n└── name: Alice\n");
}
