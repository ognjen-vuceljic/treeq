use std::io::Write;
use std::process::{Command, Stdio};

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
