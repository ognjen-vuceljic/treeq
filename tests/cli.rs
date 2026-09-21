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
    // The CLI can exit (and close its stdin) before consuming all input on
    // early-exit paths (e.g. a flag-conflict error checked before input is
    // read), so a BrokenPipe here is expected, not a test-harness bug.
    if let Err(e) = child.stdin.take().unwrap().write_all(stdin_data.as_bytes()) {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "unexpected write error: {e}"
        );
    }
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn version_flag_reports_the_crate_version() {
    let (stdout, _stderr, code) = run_treeq(&["--version"], "");
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        format!("treeq {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn renders_json_from_stdin() {
    let (stdout, _stderr, code) = run_treeq(&["--static"], r#"{"name": "Alice"}"#);
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n└── name: \"Alice\"\n");
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
    assert_eq!(stdout, "root\n├── name: \"Alice\"\n└── age: 30\n");
}

#[test]
fn path_to_a_json_scalar_leaf_prints_its_value_instead_of_nothing() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--path", "user.name"],
        r#"{"user": {"name": "Alice", "age": 30}}"#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "root: \"Alice\"\n");
}

#[test]
fn agent_flag_on_a_json_scalar_leaf_prints_its_value_instead_of_nothing() {
    let (stdout, _stderr, code) = run_treeq(
        &["--agent", "--path", "user.age"],
        r#"{"user": {"name": "Alice", "age": 30}}"#,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("root: 30"));
}

#[test]
fn array_limit_truncates_large_arrays_in_static_output() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--array-limit", "2"],
        r#"{"tags": ["a", "b", "c", "d"]}"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "root\n└── tags\n    ├── [0]: \"a\"\n    ├── [1]: \"b\"\n    └── … (2 more)\n"
    );
}

#[test]
fn array_limit_defaults_to_no_truncation() {
    let (stdout, _stderr, code) = run_treeq(&["--static"], r#"{"tags": ["a", "b", "c", "d"]}"#);
    assert_eq!(code, 0);
    assert!(!stdout.contains("more"));
}

#[test]
fn array_limit_is_rejected_for_xml_input() {
    let (_stdout, stderr, code) =
        run_treeq(&["--static", "--array-limit", "1"], "<r><a/><a/><a/></r>");
    assert_eq!(code, 1);
    assert!(stderr.contains("--array-limit"));
}

#[test]
fn reports_invalid_json() {
    let (_stdout, stderr, code) = run_treeq(&["--static"], "{not json");
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid JSON"));
}

#[test]
fn reports_invalid_json_with_snippet_and_caret() {
    let input = "{\n  \"a\": 1,\n  \"b\": 2\n  \"c\": 3\n}";
    let (_stdout, stderr, code) = run_treeq(&["--static"], input);
    assert_eq!(code, 1);
    // Line-numbered snippet, not just the bare parser message.
    assert!(stderr.contains("4 |   \"c\": 3"));
    assert!(stderr.contains("^"));
    assert!(stderr.contains("line"));
    assert!(stderr.contains("column"));
}

#[test]
fn reports_invalid_xml_with_snippet_and_caret() {
    let (_stdout, stderr, code) = run_treeq(&["--static"], "<root><unclosed></root>");
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid XML"));
    assert!(stderr.contains("1 | <root><unclosed></root>"));
    assert!(stderr.contains('^'));
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
fn escapes_control_bytes_in_a_key_when_listing_paths() {
    let (stdout, _stderr, code) = run_treeq(&["--paths"], r#"{"before\u001bafter": 1}"#);
    assert_eq!(code, 0);
    assert!(!stdout.contains('\u{1b}'));
    assert_eq!(stdout, "before\\u001bafter\n");
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
    assert_eq!(stdout, "root\n└── name: \"Alice\"\n");
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
    assert_eq!(stdout, "root\n└── name: \"Alice\"\n");

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
fn reports_deeply_nested_xml_as_a_graceful_error_instead_of_crashing() {
    let depth = 50_000;
    let xml = format!("{}leaf{}", "<a>".repeat(depth), "</a>".repeat(depth));
    let (_stdout, stderr, code) = run_treeq(&["--stats"], &xml);
    assert_eq!(code, 1);
    assert!(stderr.contains("max depth"));
}

#[test]
fn reports_an_xml_element_with_too_many_attributes_instead_of_hanging() {
    // roxmltree's attribute parsing is quadratic in attribute count; without
    // the lexical pre-check this would hang for seconds on a document this
    // size, and minutes on the size that originally exposed the bug.
    let attrs: String = (0..10_010).map(|i| format!(" a{i}=\"v\"")).collect();
    let xml = format!("<root{attrs}/>");
    let (_stdout, stderr, code) = run_treeq(&["--stats"], &xml);
    assert_eq!(code, 1);
    assert!(stderr.contains("max attribute count"));
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
    assert!(stdout.contains("name: \"Alice\""));
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
        "root\n├── [0]\n│   └── name: \"Alice\"\n└── [1]\n    └── name: \"Bob\"\n"
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
    assert_eq!(stdout, "root\n└── name: \"Alice\"\n");
}

#[test]
fn escapes_control_bytes_in_a_field_name_when_printing_schema() {
    let (stdout, _stderr, code) = run_treeq(&["--schema"], r#"{"before\u001bafter": 1}"#);
    assert_eq!(code, 0);
    assert!(!stdout.contains('\u{1b}'));
    assert_eq!(stdout, "before\\u001bafter: number\n");
}

#[test]
fn prints_json_schema() {
    let (stdout, _stderr, code) = run_treeq(
        &["--schema"],
        r#"{"user": {"name": "Alice", "tags": ["admin", "user"]}}"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "user: object\n  name: string\n  tags: array<string>\n"
    );
}

#[test]
fn prints_xml_schema() {
    let (stdout, _stderr, code) = run_treeq(
        &["--schema"],
        r#"<root><user id="1"/><user id="2"/></root>"#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "user [id] (repeated)\n");
}

#[test]
fn schema_of_empty_root_object_is_explicit_not_silent() {
    let (stdout, stderr, code) = run_treeq(&["--schema"], "{}");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "object<empty>\n");
}

#[test]
fn schema_of_nullable_array_element_keeps_field_info() {
    let (stdout, _stderr, code) = run_treeq(
        &["--schema"],
        r#"{"items": [{"a": 1, "b": 2}, {"a": 3, "b": 4}, null]}"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "items: array<object|null>\n  a: number\n  b: number\n"
    );
}

#[test]
fn renders_yaml_from_stdin() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--format", "yaml"],
        "name: Alice\ntags:\n  - admin\n  - user\n",
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "root\n├── name: \"Alice\"\n└── tags\n    ├── [0]: \"admin\"\n    └── [1]: \"user\"\n"
    );
}

#[test]
fn reports_invalid_yaml() {
    let (_stdout, stderr, code) =
        run_treeq(&["--static", "--format", "yaml"], "key: [unterminated");
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid YAML"));
}

#[test]
fn reports_multi_document_yaml_as_unsupported() {
    let (_stdout, stderr, code) = run_treeq(&["--static", "--format", "yaml"], "a: 1\n---\nb: 2\n");
    assert_eq!(code, 1);
    assert!(stderr.contains("multi-document YAML is not supported yet"));
}

#[test]
fn yaml_composes_with_path_depth_stats_and_paths_flags() {
    let input = "user:\n  name: Alice\n  age: 30\n";

    let (stdout, _stderr, code) =
        run_treeq(&["--static", "--format", "yaml", "--path", "user"], input);
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n├── name: \"Alice\"\n└── age: 30\n");

    let (stdout, _stderr, code) =
        run_treeq(&["--static", "--format", "yaml", "--depth", "1"], input);
    assert_eq!(code, 0);
    assert_eq!(stdout, "root\n└── user: …\n");

    let (stdout, _stderr, code) = run_treeq(&["--stats", "--format", "yaml"], input);
    assert_eq!(code, 0);
    assert!(stdout.contains("objects: 2"));

    let (stdout, _stderr, code) = run_treeq(&["--paths", "--format", "yaml"], input);
    assert_eq!(code, 0);
    assert_eq!(stdout, "user\nuser.name\nuser.age\n");
}

#[test]
fn reports_non_string_yaml_key_when_nested_below_the_root() {
    let (_stdout, stderr, code) = run_treeq(
        &["--static", "--format", "yaml"],
        "outer:\n  1: not-a-string-key\n",
    );
    assert_eq!(code, 1);
    assert!(
        stderr.contains("YAML mapping keys must be strings"),
        "got: {stderr}"
    );
}

#[test]
fn reports_yaml_tag_when_nested_below_the_root() {
    let (_stdout, stderr, code) = run_treeq(
        &["--static", "--format", "yaml"],
        "outer:\n  value: !mytag 5\n",
    );
    assert_eq!(code, 1);
    assert!(
        stderr.contains("YAML tags are not supported"),
        "got: {stderr}"
    );
}
