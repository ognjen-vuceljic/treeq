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
fn generate_zsh_prints_a_completion_script_without_reading_input() {
    // No input on stdin at all -- --generate must not try to read it.
    let (stdout, _stderr, code) = run_treeq(&["--generate", "zsh"], "");
    assert_eq!(code, 0);
    assert!(stdout.starts_with("#compdef treeq"));
}

#[test]
fn generate_bash_prints_a_completion_script() {
    let (stdout, _stderr, code) = run_treeq(&["--generate", "bash"], "");
    assert_eq!(code, 0);
    assert!(stdout.contains("_treeq()"));
}

#[test]
fn generate_fish_prints_a_completion_script() {
    let (stdout, _stderr, code) = run_treeq(&["--generate", "fish"], "");
    assert_eq!(code, 0);
    assert!(stdout.contains("complete -c treeq"));
}

#[test]
fn generate_rejects_an_unknown_shell_name() {
    let (_stdout, stderr, code) = run_treeq(&["--generate", "powershell-nonsense"], "");
    assert_eq!(code, 2, "clap's own argument-parsing exit code");
    assert!(stderr.contains("invalid value"));
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
fn resolves_a_path_to_a_key_that_itself_contains_a_literal_dot() {
    let (paths_stdout, _stderr, code) = run_treeq(&["--paths"], r#"{"a.b": {"c": "hi"}}"#);
    assert_eq!(code, 0);
    assert_eq!(paths_stdout, "a\\.b\na\\.b.c\n");

    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--path", "a\\.b.c"],
        r#"{"a.b": {"c": "hi"}}"#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "root: \"hi\"\n");
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
fn conflicting_output_mode_flags_are_rejected_instead_of_silently_resolved() {
    // Previously each mode flag was checked in a fixed order, so combining
    // them silently picked a winner instead of erroring (issue #129).
    let input = r#"{"a": 1}"#;

    let (_stdout, stderr, code) = run_treeq(&["--agent", "--stats"], input);
    assert_ne!(code, 0);
    assert!(stderr.contains("cannot be used with"), "got: {stderr}");

    let (_stdout, stderr, code) = run_treeq(&["--agent", "--paths"], input);
    assert_ne!(code, 0);
    assert!(stderr.contains("cannot be used with"), "got: {stderr}");
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
fn rejects_pick_with_static() {
    // Without this check, `--static` would silently win and `--pick` would
    // have no effect at all -- no TUI, no picked-path output, no error.
    let (_stdout, stderr, code) = run_treeq(&["--static", "--pick"], "{\"a\": 1}\n");
    assert_eq!(code, 1);
    assert!(stderr.contains("--pick"));
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
fn multi_document_yaml_becomes_an_array_of_documents() {
    let (out, err, code) = run_treeq(
        &["--paths", "--format", "yaml"],
        "---\nkind: A\n---\nkind: B\nspec:\n  n: 1\n",
    );
    assert_eq!(code, 0, "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines.contains(&"0.kind") && lines.contains(&"1.spec.n"),
        "{out}"
    );
}

#[test]
fn malformed_multi_document_yaml_errors_instead_of_hanging() {
    let (_, err, code) = run_treeq(&["--static", "--format", "yaml"], "a: [\n---\nb: 1\n");
    assert_eq!(code, 1);
    assert!(err.contains("invalid YAML"), "{err}");
}

#[test]
fn closed_stdout_pipe_does_not_panic_for_static_output() {
    let mut json = String::from("{");
    for i in 0..20000 {
        json.push_str(&format!("\"k{i}\": [1, 2, 3],"));
    }
    json.push_str("\"end\": 0}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_treeq"))
        .args(["--static"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(json.as_bytes());
    });
    drop(child.stdout.take()); // reader goes away immediately, like `| head -0`
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked"), "{err}");
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
    // Truncated containers say how much they hide (issue #134).
    assert_eq!(stdout, "root\n└── user {2}: …\n");

    let (stdout, _stderr, code) = run_treeq(&["--stats", "--format", "yaml"], input);
    assert_eq!(code, 0);
    assert!(stdout.contains("objects: 2"));

    let (stdout, _stderr, code) = run_treeq(&["--paths", "--format", "yaml"], input);
    assert_eq!(code, 0);
    assert_eq!(stdout, "user\nuser.name\nuser.age\n");
}

#[test]
fn renders_non_string_yaml_keys_nested_below_the_root() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--format", "yaml"],
        "outer:\n  1: first\n  true: second\n",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("1: \"first\""), "got: {stdout}");
    assert!(stdout.contains("true: \"second\""), "got: {stdout}");
}

#[test]
fn renders_yaml_tag_nested_below_the_root() {
    let (stdout, _stderr, code) = run_treeq(
        &["--static", "--format", "yaml"],
        "outer:\n  value: !mytag 5\n",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("value: \"!mytag 5\""), "got: {stdout}");
}

#[test]
fn yaml_complex_keys_and_tags_work_in_paths_and_stats() {
    let doc = "c: !tag v\n1: int-key\n? [1, 2]\n: complex\n";
    let (paths, _e, code) = run_treeq(&["--paths", "--format", "yaml"], doc);
    assert_eq!(code, 0);
    assert_eq!(paths, "c\n1\n[1,2]\n");
    let (stats, _e, code) = run_treeq(&["--stats", "--format", "yaml"], doc);
    assert_eq!(code, 0);
    assert!(stats.contains("scalars: 3"), "got: {stats}");
}

#[test]
fn yaml_tagged_multi_document_stream_becomes_an_array() {
    let (stdout, _e, code) = run_treeq(
        &["--static", "--format", "yaml"],
        "---\na: !t 1\n---\n2: b\n",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("[0]") && stdout.contains("[1]"),
        "got: {stdout}"
    );
}

#[test]
fn strips_a_leading_utf8_bom_for_json_and_xml() {
    let (out, _, code) = run_treeq(&["--static"], "\u{feff}{\"a\":1}");
    assert_eq!((out.as_str(), code), ("root\n└── a: 1\n", 0));
    let (out, _, code) = run_treeq(&["--static"], "\u{feff}<a><b>1</b></a>");
    assert_eq!((out.as_str(), code), ("a\n└── b: 1\n", 0));
}

#[test]
fn preserves_json_number_literals_exactly() {
    let (out, _, _) = run_treeq(
        &["--static"],
        r#"{"id": 12345678901234567890123, "f": 1e2, "p": 0.1000000000000000055511151231257827}"#,
    );
    assert!(out.contains("id: 12345678901234567890123"), "{out}");
    // serde_json's arbitrary_precision normalizes the exponent sign but
    // keeps the digits -- previously this printed the float `100.0`.
    assert!(out.contains("f: 1e+2"), "{out}");
    assert!(
        out.contains("p: 0.1000000000000000055511151231257827"),
        "{out}"
    );
}

#[test]
fn empty_arrays_and_objects_show_their_own_shape_instead_of_a_bare_key() {
    let (out, _, _) = run_treeq(&["--static"], r#"{"a": [], "b": {}, "c": ""}"#);
    assert_eq!(
        out, "root\n├── a: []\n├── b: {}\n└── c: \"\"\n",
        "got: {out}"
    );
}

#[test]
fn xml_depth_truncation_keeps_the_elements_attributes_and_text_visible() {
    let (out, _, _) = run_treeq(
        &["--static", "--depth", "1"],
        r#"<r><a id="7">t<b/></a></r>"#,
    );
    assert!(out.contains(r#"a [id="7"]: t …"#), "got: {out}");
}

const BOOKS: &str = "<c><book><t>A</t></book><book><t>B</t></book></c>";

#[test]
fn xml_paths_index_same_named_siblings_but_leave_unique_names_plain() {
    // Issue #118: repeated tags used to print as duplicate paths.
    let (out, _, code) = run_treeq(&["--paths"], BOOKS);
    assert_eq!(code, 0);
    assert_eq!(out, "book[0]\nbook[0].t\nbook[1]\nbook[1].t\n");

    let (out, _, _) = run_treeq(&["--paths"], "<c><a/><b><x/></b></c>");
    assert_eq!(out, "a\nb\nb.x\n");
}

#[test]
fn xml_path_can_select_the_nth_same_named_sibling() {
    let (out, _, code) = run_treeq(&["--path", "book[1].t"], BOOKS);
    assert_eq!((out.as_str(), code), ("t: B\n", 0));

    // A bare name still means "the first one", as before.
    let (out, _, _) = run_treeq(&["--path", "book.t"], BOOKS);
    assert_eq!(out, "t: A\n");

    // Every path `--paths` prints round-trips through `--path`.
    let (paths, _, _) = run_treeq(&["--paths"], BOOKS);
    for p in paths.lines() {
        let (_, stderr, code) = run_treeq(&["--path", p], BOOKS);
        assert_eq!(code, 0, "`--path {p}` failed: {stderr}");
    }
}

#[test]
fn xml_path_index_out_of_range_reports_the_segment() {
    let (_, stderr, code) = run_treeq(&["--path", "book[5]"], BOOKS);
    assert_eq!(code, 1);
    assert!(stderr.contains("book[5]"), "got: {stderr}");
}

#[test]
fn xml_names_keep_their_namespace_prefix() {
    // Issue #119: `x:type` and `type` used to both render as `type`.
    let xml = r#"<s:E xmlns:s="urn:s" xmlns:x="urn:x"><s:B x:type="a" type="b">v</s:B></s:E>"#;
    let (out, _, code) = run_treeq(&["--static"], xml);
    assert_eq!(code, 0);
    assert_eq!(out, "s:E\n└── s:B [x:type=\"a\" type=\"b\"]: v\n");

    let (out, _, _) = run_treeq(&["--path", "s:B"], xml);
    assert!(out.starts_with("s:B "), "got: {out}");

    // The default namespace has no prefix, so plain names are unchanged.
    let (out, _, _) = run_treeq(&["--static"], r#"<r xmlns="urn:d"><a>1</a></r>"#);
    assert_eq!(out, "r\n└── a: 1\n");
}

#[test]
fn depth_truncation_reports_how_many_children_each_cut_off_container_hides() {
    let input = r#"{"tags": ["a", "b", "c"], "owner": {"x": 1, "y": 2}, "n": 1}"#;
    let (out, _, code) = run_treeq(&["--static", "--depth", "1"], input);
    assert_eq!(code, 0);
    assert_eq!(out, "root\n├── tags [3]: …\n├── owner {2}: …\n└── n: 1\n");
    // Without --depth nothing is hidden, so the plain output is unchanged.
    let (out, _, _) = run_treeq(&["--static"], input);
    assert!(out.contains("├── tags\n"), "{out}");
}

#[test]
fn path_error_lists_available_keys_and_suggests_the_closest() {
    let (_, err, code) = run_treeq_with_file(
        &["--static", "--path", "user.nmae"],
        r#"{"user": {"name": "A", "age": 3}}"#,
    );
    assert_eq!(code, 1);
    assert!(err.contains("'nmae' not found"), "{err}");
    assert!(err.contains("did you mean 'name'?"), "{err}");
    assert!(err.contains("available: name, age"), "{err}");
}

#[test]
fn path_error_on_array_reports_length() {
    let (_, err, code) = run_treeq_with_file(&["--static", "--path", "a.5"], r#"{"a": [1, 2]}"#);
    assert_eq!(code, 1);
    assert!(err.contains("array has 2 items"), "{err}");
}

#[test]
fn path_error_in_xml_lists_child_names() {
    let (_, err, code) = run_treeq_with_file(
        &["--static", "--path", "usr"],
        "<root><user/><items/></root>",
    );
    assert_eq!(code, 1);
    assert!(err.contains("did you mean 'user'?"), "{err}");
    assert!(err.contains("available: user, items"), "{err}");
}

#[test]
fn json_flag_makes_stats_paths_schema_and_agent_machine_readable() {
    let input = r#"{"a": [1, 2], "b": "x"}"#;
    let parse = |flags: &[&str]| -> serde_json::Value {
        let mut args = vec!["--json"];
        args.extend_from_slice(flags);
        let (out, err, code) = run_treeq_with_file(&args, input);
        assert_eq!(code, 0, "{err}");
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"))
    };
    assert_eq!(parse(&["--stats"])["arrays"], 1);
    assert_eq!(
        parse(&["--paths"]),
        serde_json::json!(["a", "a.0", "a.1", "b"])
    );
    assert!(
        parse(&["--schema"])["schema"]
            .as_str()
            .unwrap()
            .contains("string")
    );
    let agent = parse(&["--agent"]);
    assert_eq!(agent["stats"]["objects"], 1);
    assert!(agent["tree"].as_str().unwrap().contains("b"));
    assert!(!agent["tree"].as_str().unwrap().contains('\x1b'));
}

#[test]
fn json_flag_works_for_xml_and_requires_a_mode() {
    let (out, _, code) = run_treeq_with_file(&["--json", "--stats"], "<r><a/></r>");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["elements"], 2);
    let (_, err, code) = run_treeq_with_file(&["--json"], "{}");
    assert_eq!(code, 1);
    assert!(err.contains("--json needs"), "{err}");
}

#[test]
fn yaml_and_ndjson_are_detected_without_flags() {
    let (out, err, code) = run_treeq_with_file(&["--static"], "name: Alice\ntags:\n  - a\n  - b\n");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("name") && out.contains("tags"), "{out}");
    let (out, err, code) = run_treeq_with_file(&["--paths"], "{\"a\":1}\n{\"a\":2}\n");
    assert_eq!(code, 0, "{err}");
    assert!(out.lines().any(|l| l == "1.a"), "{out}");
}

#[test]
fn broken_json_still_reports_a_json_error() {
    let (_, err, code) = run_treeq_with_file(&["--static"], "{\"a\": ");
    assert_eq!(code, 1);
    assert!(err.contains("line 1"), "{err}");
}

#[test]
fn xml_attributes_are_listed_and_addressable_by_path() {
    let xml = r#"<root><book id="7" lang="en"><t>x</t></book></root>"#;
    let (out, err, code) = run_treeq_with_file(&["--paths"], xml);
    assert_eq!(code, 0, "{err}");
    assert!(out.lines().any(|l| l == "book.@id"), "{out}");
    let (out, err, code) = run_treeq_with_file(&["--static", "--path", "book.@lang"], xml);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("en"), "{out}");
}

#[test]
fn closed_stdout_pipe_does_not_panic_for_stats_and_paths() {
    for flag in ["--stats", "--paths", "--schema"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_treeq"))
            .args([flag])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        drop(child.stdout.take()); // reader gone before any output
        let _ = stdin.write_all(br#"{"a": [1, 2], "b": {"c": null}}"#);
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("panicked"), "{flag}: {err}");
    }
}

#[test]
fn c1_and_bidi_controls_in_values_and_keys_are_escaped() {
    let (out, _, code) =
        run_treeq_with_file(&["--static"], "{\"k\\u009b\": \"a\\u009d0;x\\u202ec\"}");
    assert_eq!(code, 0);
    assert!(!out.contains('\u{9b}') && !out.contains('\u{9d}') && !out.contains('\u{202e}'));
    assert!(
        out.contains("\\u009b") && out.contains("\\u202e"),
        "{out:?}"
    );
}

#[test]
fn missing_file_error_names_the_path() {
    let (out, err, code) = run_treeq(&["--static", "/nonexistent/treeq-missing.json"], "");
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.contains("/nonexistent/treeq-missing.json: No such file or directory"),
        "stderr was: {err}"
    );
}

#[test]
fn directory_as_input_error_names_the_path() {
    let dir = std::env::temp_dir();
    let dir = dir.to_string_lossy();
    let (_, err, code) = run_treeq(&["--static", &dir], "");
    assert_eq!(code, 1);
    assert!(err.contains(&*dir), "stderr was: {err}");
}

#[test]
fn invalid_utf8_file_error_names_the_path() {
    let n = TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file = std::env::temp_dir().join(format!("treeq-bad-utf8-{}-{n}.json", std::process::id()));
    fs::write(&file, [0xff, 0xfe, b'{']).unwrap();
    let path = file.to_string_lossy().to_string();
    let (_, err, code) = run_treeq(&["--static", &path], "");
    fs::remove_file(&file).ok();
    assert_eq!(code, 1);
    assert!(err.contains(&path), "stderr was: {err}");
    assert!(err.contains("valid UTF-8"), "stderr was: {err}");
}

#[test]
fn stdin_read_error_is_not_path_prefixed() {
    let (_, err, code) = run_treeq(&["--static"], "");
    assert_eq!(code, 1);
    assert!(err.starts_with("error:"), "stderr was: {err}");
}

const MIXED: &str = "<r>lead<b/>mid<c x=\"1\">t</c>tail</r>";

#[test]
fn xml_mixed_content_renders_text_runs_in_order() {
    let (out, _e, code) = run_treeq(&["--static"], MIXED);
    assert_eq!(code, 0);
    let rows: Vec<&str> = out
        .lines()
        .map(|l| l.trim_start_matches(['├', '└', '─', ' ']))
        .collect();
    assert_eq!(
        rows,
        [
            "r",
            "#text: lead",
            "b",
            "#text: mid",
            "c [x=\"1\"]: t",
            "#text: tail"
        ]
    );
}

#[test]
fn xml_mixed_content_paths_and_path_selection() {
    let (out, _e, _c) = run_treeq(&["--paths"], MIXED);
    assert_eq!(out, "#text[0]\nb\n#text[1]\nc\nc.@x\n#text[2]\n");
    let (out, _e, code) = run_treeq(&["--static", "--path", "#text[1]"], MIXED);
    assert_eq!(code, 0);
    assert!(out.contains("#text: mid"), "got: {out}");
}

#[test]
fn xml_mixed_content_stats_count_text_runs_separately() {
    let (out, _e, _c) = run_treeq(&["--stats", "--json"], MIXED);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["elements"], 3);
    assert_eq!(v["text_nodes"], 4);
}

#[test]
fn xml_text_leaf_output_is_unchanged() {
    let (out, _e, _c) = run_treeq(&["--static"], "<r><a>x</a><b>y</b></r>");
    assert!(out.contains("a: x") && out.contains("b: y"), "got: {out}");
    assert!(!out.contains("#text"), "got: {out}");
}

#[test]
fn xml_depth_truncation_previews_mixed_text() {
    let (out, _e, _c) = run_treeq(
        &["--static", "--depth", "1"],
        r#"<r><a id="7">t<b/>u</a></r>"#,
    );
    assert!(out.contains(r#"a [id="7"]: t u …"#), "got: {out}");
}

#[test]
fn xml_with_a_dtd_is_rejected_with_an_explanation() {
    let (out, err, code) = run_treeq(&["--static"], r#"<!DOCTYPE r [<!ENTITY e "x">]><r>&e;</r>"#);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.contains("DTD") && err.contains("not supported"),
        "got: {err}"
    );
}

#[test]
fn xml_entity_bombs_and_external_entities_are_rejected_fast() {
    let mut bomb = String::from(r#"<!DOCTYPE r [<!ENTITY a0 "lol">"#);
    for i in 1..10 {
        bomb.push_str(&format!(
            r#"<!ENTITY a{i} "{}">"#,
            format!("&a{};", i - 1).repeat(10)
        ));
    }
    bomb.push_str("]><r>&a9;</r>");
    let ext = r#"<!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd">]><r>&x;</r>"#;
    for doc in [bomb.as_str(), ext] {
        let start = std::time::Instant::now();
        let (out, _e, code) = run_treeq(&["--static"], doc);
        assert_eq!(code, 1);
        assert!(!out.contains("root:"), "leaked file content: {out}");
        assert!(start.elapsed().as_secs() < 5);
    }
}

const CARGO_LIKE: &str = "[package]\nname = \"treeq\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"a\"\n\n[[bin]]\nname = \"b\"\n";

#[test]
fn toml_is_detected_from_the_extension() {
    let n = TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file = std::env::temp_dir().join(format!("treeq-{}-{n}.toml", std::process::id()));
    fs::write(&file, CARGO_LIKE).unwrap();
    let path = file.to_string_lossy().to_string();
    let (out, _e, code) = run_treeq(&["--static", &path], "");
    fs::remove_file(&file).ok();
    assert_eq!(code, 0);
    assert!(
        out.contains("name: \"treeq\"") && out.contains("[1]"),
        "got: {out}"
    );
}

#[test]
fn toml_is_sniffed_from_stdin_and_forced_with_format() {
    let (out, _e, code) = run_treeq(&["--paths"], CARGO_LIKE);
    assert_eq!(code, 0);
    assert!(out.starts_with("package\npackage.name\n"), "got: {out}");
    let (out, _e, code) = run_treeq(&["--static", "--format", "toml"], "a = 1\n");
    assert_eq!(code, 0);
    assert!(out.contains("a: 1"), "got: {out}");
}

#[test]
fn toml_path_stats_schema_and_json_flags_work() {
    let (out, _e, code) = run_treeq(&["--static", "--path", "bin.1"], CARGO_LIKE);
    assert_eq!(code, 0);
    assert!(out.contains("name: \"b\""), "got: {out}");
    let (out, _e, _c) = run_treeq(&["--stats", "--json"], CARGO_LIKE);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["arrays"], 1);
    let (out, _e, code) = run_treeq(&["--schema"], CARGO_LIKE);
    assert_eq!(code, 0);
    assert!(out.contains("package: object"), "got: {out}");
}

#[test]
fn invalid_toml_with_format_flag_reports_a_toml_error() {
    let (out, err, code) = run_treeq(&["--static", "--format", "toml"], "a = 1\nb = \n");
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.contains("invalid TOML at line 2"), "got: {err}");
}

#[test]
fn json_and_yaml_are_not_misdetected_as_toml() {
    let (out, _e, _c) = run_treeq(&["--static"], r#"{"a": 1}"#);
    assert!(out.contains("a: 1"));
    let (out, _e, _c) = run_treeq(&["--static"], "a: 1\nb: [2]\n");
    assert!(out.contains("b"), "got: {out}");
    let (out, _e, _c) = run_treeq(&["--static"], "[1, 2, 3]");
    assert!(out.contains("[2]: 3"), "got: {out}");
}

const JSONC_DOC: &str =
    "// header\n{\n  \"a\": 1, /* inline */\n  \"u\": \"http://x/*y*/\",\n  \"l\": [1, 2,],\n}\n";

#[test]
fn jsonc_is_detected_from_the_extension() {
    let n = TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file = std::env::temp_dir().join(format!("treeq-{}-{n}.jsonc", std::process::id()));
    fs::write(&file, JSONC_DOC).unwrap();
    let path = file.to_string_lossy().to_string();
    let (out, _e, code) = run_treeq(&["--static", &path], "");
    fs::remove_file(&file).ok();
    assert_eq!(code, 0);
    assert!(out.contains("a: 1") && out.contains("[1]: 2"), "got: {out}");
    assert!(out.contains("u: \"http://x/*y*/\""), "got: {out}");
}

#[test]
fn jsonc_is_sniffed_from_stdin_and_forced_with_format() {
    let (out, _e, code) = run_treeq(&["--paths"], JSONC_DOC);
    assert_eq!(code, 0);
    assert_eq!(out, "a\nu\nl\nl.0\nl.1\n");
    let (out, _e, code) = run_treeq(&["--static", "--format", "jsonc"], "[1,]");
    assert_eq!(code, 0);
    assert!(out.contains("[0]: 1"), "got: {out}");
}

#[test]
fn jsonc_works_with_path_stats_and_agent() {
    let (out, _e, code) = run_treeq(&["--static", "--path", "l.1"], JSONC_DOC);
    assert_eq!(code, 0);
    assert!(out.contains("2"), "got: {out}");
    let (out, _e, _c) = run_treeq(&["--stats", "--json"], JSONC_DOC);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["scalars"], 4);
    let (out, _e, code) = run_treeq(&["--agent"], JSONC_DOC);
    assert_eq!(code, 0);
    assert!(out.contains("scalars: 4"), "got: {out}");
}

#[test]
fn jsonc_error_points_at_the_original_line_and_column() {
    let (_o, err, code) = run_treeq(
        &["--static", "--format", "jsonc"],
        "{\"a\": 1, // note\n \"b\": oops}",
    );
    assert_eq!(code, 1);
    assert!(err.contains("line 2, column 7"), "got: {err}");
    assert!(
        err.contains("// note"),
        "snippet should show the original: {err}"
    );
}

#[test]
fn plain_json_keeps_rejecting_comments_and_trailing_commas() {
    let (_o, err, code) = run_treeq(&["--static", "--format", "json"], "{\"a\":1,}");
    assert_eq!(code, 1);
    assert!(err.contains("invalid JSON"), "got: {err}");
}

#[test]
fn broken_json_is_not_sniffed_as_jsonc() {
    let (_o, err, code) = run_treeq(&["--static"], "{\"a\":");
    assert_eq!(code, 1);
    assert!(err.contains("invalid JSON"), "got: {err}");
}

#[test]
fn jsonc_reload_state_and_ndjson_are_unaffected() {
    let (out, _e, code) = run_treeq(&["--static", "--ndjson"], "{\"a\":1}\n{\"a\":2}\n");
    assert_eq!(code, 0);
    assert!(out.contains("[1]"), "got: {out}");
}

const TOP_DOC: &str = r#"{"a":"xxxxxxxxxx","b":{"c":[1,2,3],"d":"é"},"e":null}"#;

#[test]
fn top_lists_largest_nodes_as_bytes_tab_path() {
    let (out, _e, code) = run_treeq(&["--top", "3"], TOP_DOC);
    assert_eq!(code, 0);
    assert_eq!(out, "22\tb\n12\ta\n7\tb.c\n");
}

#[test]
fn top_json_output_is_an_array_of_path_and_bytes() {
    let (out, _e, code) = run_treeq(&["--top", "2", "--json"], TOP_DOC);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["path"], "b");
    assert_eq!(v[0]["bytes"], 22);
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn top_respects_path_and_works_for_xml_yaml_toml() {
    let (out, _e, _c) = run_treeq(&["--top", "1", "--path", "b"], TOP_DOC);
    assert_eq!(out, "7\tc\n");
    let (out, _e, _c) = run_treeq(&["--top", "1"], "<r><a>hello</a><b/></r>");
    assert_eq!(out, "12\ta\n");
    let (out, _e, _c) = run_treeq(&["--top", "1", "--format", "yaml"], "a: xx\nb: yyyy\n");
    assert_eq!(out, "6\tb\n");
    let (out, _e, _c) = run_treeq(&["--top", "1", "--format", "json"], "[1,22,3]");
    assert_eq!(out, "2\t1\n");
}

#[test]
fn top_rejects_zero_and_conflicting_modes() {
    let (_o, err, code) = run_treeq(&["--top", "0"], TOP_DOC);
    assert_ne!(code, 0);
    assert!(err.contains("--top"), "got: {err}");
    for other in ["--stats", "--paths", "--schema", "--agent"] {
        let (_o, err, code) = run_treeq(&["--top", "3", other], TOP_DOC);
        assert_ne!(code, 0, "{other}");
        assert!(err.contains("cannot be used"), "{other}: {err}");
    }
}

#[test]
fn top_beyond_node_count_lists_everything_and_empty_docs_print_nothing() {
    let (out, _e, code) = run_treeq(&["--top", "99"], r#"{"a":1}"#);
    assert_eq!(code, 0);
    assert_eq!(out, "1\ta\n");
    let (out, _e, code) = run_treeq(&["--top", "5"], "[]");
    assert_eq!(code, 0);
    assert_eq!(out, "");
}

const GREP_DOC: &str = r#"{"user":{"name":"A","tags":["x","y"]},"a.b":1,"id":2,"é":3}"#;

#[test]
fn grep_keeps_only_matching_paths_and_supports_anchors() {
    let (out, _e, code) = run_treeq(&["--paths", "--grep", "^user\\."], GREP_DOC);
    assert_eq!(code, 0);
    assert_eq!(out, "user.name\nuser.tags\nuser.tags.0\nuser.tags.1\n");
    let (out, _e, _c) = run_treeq(&["--paths", "--grep", "name$|^id$"], GREP_DOC);
    assert_eq!(out, "user.name\nid\n");
}

#[test]
fn grep_matches_the_escaped_form_printed_by_paths() {
    let (out, _e, code) = run_treeq(&["--paths", "--grep", "a\\\\\\.b"], GREP_DOC);
    assert_eq!(code, 0);
    assert_eq!(out, "a\\.b\n");
    let (out, _e, _c) = run_treeq(&["--paths", "--grep", "é"], GREP_DOC);
    assert_eq!(out, "é\n");
}

#[test]
fn grep_with_no_match_exits_1_and_json_prints_an_empty_array() {
    let (out, _e, code) = run_treeq(&["--paths", "--grep", "zzz"], GREP_DOC);
    assert_eq!(code, 1);
    assert_eq!(out, "");
    let (out, _e, code) = run_treeq(&["--paths", "--grep", "zzz", "--json"], GREP_DOC);
    assert_eq!(code, 1);
    assert_eq!(out.trim(), "[]");
}

#[test]
fn grep_json_output_is_the_filtered_array() {
    let (out, _e, code) = run_treeq(&["--paths", "--grep", "tags\\.", "--json"], GREP_DOC);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v, serde_json::json!(["user.tags.0", "user.tags.1"]));
}

#[test]
fn grep_rejects_a_bad_regex_and_requires_paths() {
    let (_o, err, code) = run_treeq(&["--paths", "--grep", "("], GREP_DOC);
    assert_ne!(code, 0);
    assert!(
        err.contains("--grep") && err.contains("unclosed group"),
        "got: {err}"
    );
    let (_o, err, code) = run_treeq(&["--grep", "x"], GREP_DOC);
    assert_ne!(code, 0);
    assert!(err.contains("--paths"), "got: {err}");
}

#[test]
fn grep_works_for_xml_and_each_result_round_trips_into_path() {
    let xml = "<r><item id=\"1\"><n>a</n></item><item id=\"2\"><n>b</n></item></r>";
    let (out, _e, code) = run_treeq(&["--paths", "--grep", "^item\\[1\\]"], xml);
    assert_eq!(code, 0);
    assert_eq!(out, "item[1]\nitem[1].@id\nitem[1].n\n");
    for p in out.lines() {
        let (_o, _e, code) = run_treeq(&["--static", "--path", p], xml);
        assert_eq!(code, 0, "path {p}");
    }
}


fn run_diff_files(args: &[&str], a: &str, b: &str) -> (String, String, i32) {
    let n = TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("treeq-diff-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (fa, fb) = (dir.join("a.json"), dir.join("b.json"));
    fs::write(&fa, a).unwrap();
    fs::write(&fb, b).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_treeq"))
        .arg(&fa)
        .arg("--diff")
        .arg(&fb)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    fs::remove_dir_all(&dir).ok();
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn diff_identical_files_exit_0_with_no_output() {
    let (out, _e, code) = run_diff_files(&[], r#"{"a":1,"b":2}"#, r#"{"b":2,"a":1}"#);
    assert_eq!(code, 0);
    assert_eq!(out, "");
}

#[test]
fn diff_reports_changes_and_exits_1() {
    let (out, _e, code) = run_diff_files(
        &[],
        r#"{"a":1,"b":{"c":[1,2,3]},"gone":true}"#,
        r#"{"b":{"c":[1,2]},"a":2,"new":null}"#,
    );
    assert_eq!(code, 1);
    assert_eq!(out, "~ a: 1 -> 2\n- b.c.2: 3\n- gone: true\n+ new: null\n");
}

#[test]
fn diff_json_output_has_real_json_values() {
    let (out, _e, code) = run_diff_files(&["--json"], r#"{"a":[1,2]}"#, r#"{"a":[1,3]}"#);
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["op"], "changed");
    assert_eq!(v[0]["path"], "a.1");
    assert_eq!(v[0]["from"], 2);
    assert_eq!(v[0]["to"], 3);
}

#[test]
fn diff_can_be_scoped_with_path() {
    let (out, _e, code) = run_diff_files(
        &["--path", "b"],
        r#"{"a":1,"b":{"x":1}}"#,
        r#"{"a":9,"b":{"x":2}}"#,
    );
    assert_eq!(code, 1);
    assert_eq!(out, "~ x: 1 -> 2\n");
}

#[test]
fn diff_errors_exit_2_not_1() {
    let (_o, err, code) = run_diff_files(&[], r#"{"a":"#, r#"{"a":1}"#);
    assert_eq!(code, 2, "{err}");
    let (_o, err, code) = run_diff_files(&[], r#"{"a":1}"#, "<r/>");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("does not support XML"), "{err}");
    let (_o, _e, code) = run_treeq(&["--diff", "/no/such/file.json"], r#"{"a":1}"#);
    assert_eq!(code, 2);
}

#[test]
fn diff_conflicts_with_other_output_modes() {
    for other in ["--stats", "--paths", "--schema", "--agent"] {
        let (_o, err, code) = run_treeq(&["--diff", "x.json", other], "{}");
        assert_ne!(code, 0, "{other}");
        assert!(err.contains("cannot be used"), "{other}: {err}");
    }
}

#[test]
fn diff_compares_across_formats() {
    let n = TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file = std::env::temp_dir().join(format!("treeq-{}-{n}.yaml", std::process::id()));
    fs::write(&file, "a: 1\nb: [1, 2]\n").unwrap();
    let path = file.to_string_lossy().to_string();
    let (out, _e, code) = run_treeq(&["--diff", &path], r#"{"a":1,"b":[1,2]}"#);
    fs::remove_file(&file).ok();
    assert_eq!(code, 0, "{out}");
}
