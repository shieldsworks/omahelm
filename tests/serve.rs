#![allow(
    clippy::unwrap_used,
    reason = "a tests/*.rs file is a crate of its own; allow-unwrap-in-tests covers its #[test] fns, not their helpers"
)]

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "omahelm-serve-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("charts")).unwrap();
    dir
}

fn serve(dir: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_omahelm"))
        .args(["serve", "--charts"])
        .arg(dir.join("charts"))
        .env("HOME", dir.join("home"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn socket(dir: &Path) -> PathBuf {
    dir.join("run/omahelm/helm.sock")
}

fn wait_until(
    dir: &Path,
    child: &mut Child,
    timeout: Duration,
    ready: impl Fn(&Path) -> bool,
) -> String {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready(dir) {
            return String::new();
        }
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            if let Some(stderr) = child.stderr.as_mut() {
                stderr.read_to_string(&mut err).unwrap();
            }
            return format!("exited {status}: {err}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    "timed out".into()
}

struct Stop(Child);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_second_serve_exits_1_and_the_lock_is_private() {
    let dir = scratch("second");
    let mut first = Stop(serve(&dir));
    let sock = socket(&dir);
    let early = wait_until(&dir, &mut first.0, Duration::from_secs(30), |_| {
        sock.exists()
    });
    assert!(early.is_empty(), "{early}");

    let second = Command::new(env!("CARGO_BIN_EXE_omahelm"))
        .args(["serve", "--charts"])
        .arg(dir.join("charts"))
        .env("HOME", dir.join("home"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&second.stderr);
    assert_eq!(
        second.status.code(),
        Some(1),
        "a second engine is an error:\n{err}"
    );
    assert_eq!(
        err.trim_end(),
        format!("omahelm: already running on {}", sock.display())
    );
    assert!(sock.exists(), "the first engine keeps the socket");
    let lock = PathBuf::from(format!("{}.lock", sock.display()));
    let mode = fs::metadata(&lock).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the lock file is private to the user");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_path_that_is_not_a_socket_is_left_alone() {
    let dir = scratch("file");
    let sock = socket(&dir);
    fs::create_dir_all(sock.parent().unwrap()).unwrap();
    fs::write(&sock, b"not a socket").unwrap();
    let mut child = serve(&dir);
    let early = wait_until(&dir, &mut child, Duration::from_secs(5), |_| false);
    let message = if early == "timed out" {
        let _ = child.kill();
        let _ = child.wait();
        String::from("serve stayed up and would have replaced the file")
    } else {
        early
    };
    let kind = fs::symlink_metadata(&sock).unwrap().file_type();
    assert!(
        kind.is_file(),
        "serve replaced the file with a socket: {message}"
    );
    assert_eq!(fs::read(&sock).unwrap(), b"not a socket", "{message}");
    assert!(message.contains("exit status: 1"), "{message}");
    assert!(message.contains("isn't a socket"), "{message}");
    let _ = fs::remove_dir_all(&dir);
}
