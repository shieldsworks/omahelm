//! The chart window's runtime directory, the same rule as the engine.

#![allow(
    clippy::unwrap_used,
    reason = "a tests/*.rs file is a crate of its own; allow-unwrap-in-tests covers its #[test] fns, not their helpers"
)]

use std::process::Command;

fn runtime_js() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/runtime.js");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Runs `ui/runtime.js` the way the window does: one value in, the directory
/// and the error string out.
fn helm_runtime(value: &str) -> (String, String) {
    let script = format!(
        r#"
{source}
const value = process.argv[1] === "undefined" ? undefined
    : process.argv[1] === "null" ? null
    : process.argv[1];
process.stdout.write(runtimeDir(value) + "\n" + runtimeError(value));
"#,
        source = runtime_js()
    );
    let out = Command::new("node")
        .arg("-e")
        .arg(&script)
        .arg(value)
        .output()
        .unwrap_or_else(|e| panic!("node: {e}"));
    assert!(
        out.status.success(),
        "node {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let (dir, error) = text.split_once('\n').unwrap();
    (dir.to_string(), error.to_string())
}

#[test]
fn a_missing_runtime_dir_is_an_error_and_not_tmp() {
    let err = "XDG_RUNTIME_DIR must be set to an absolute path";
    for value in [
        "undefined",
        "null",
        "",
        "rel",
        "tmp",
        "./run",
        "run/user/1000",
    ] {
        let (dir, error) = helm_runtime(value);
        assert_eq!(dir, "", "{value}");
        assert_eq!(error, err, "{value}");
        assert!(!dir.contains("/tmp"), "{value}");
    }
}

#[test]
fn an_absolute_runtime_dir_is_the_engine_directory() {
    let (dir, error) = helm_runtime("/run/user/1000");
    assert_eq!(error, "");
    assert_eq!(dir, "/run/user/1000/omahelm/");
    let (root, root_error) = helm_runtime("/");
    assert_eq!(root_error, "");
    assert_eq!(root, "/omahelm/");
    let (slash, slash_error) = helm_runtime("/run/user/1000/");
    assert_eq!(slash_error, "");
    assert_eq!(slash, "/run/user/1000/omahelm/");
}

#[test]
fn the_window_shows_that_error_instead_of_starting() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let helm = std::fs::read_to_string(root.join("ui/Helm.qml")).unwrap();
    let window = std::fs::read_to_string(root.join("ui/ChartWindow.qml")).unwrap();
    assert!(
        !helm.contains("|| \"/tmp\""),
        "Helm.qml still falls back to /tmp"
    );
    assert!(
        helm.contains("Runtime.runtimeError"),
        "Helm.qml does not use the runtime rule"
    );
    assert!(
        window.contains("helm.runtimeError"),
        "the window does not show the runtime error"
    );
}
