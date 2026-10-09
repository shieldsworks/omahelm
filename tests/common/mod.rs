//! What the integration tests share: the fixture charts, opened from a copy.

use omahelm::library::Library;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The fixture charts in a fresh temp dir. `Library::open` writes
/// `index.json` into the chart root, so it never opens `tests/fixtures`.
/// The dir is removed on drop.
pub struct Charts {
    pub lib: Library,
    dir: PathBuf,
}

impl Charts {
    pub fn open() -> Charts {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("omahelm-charts-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = fixtures().join("ENC_ROOT");
        for cell in std::fs::read_dir(&src)
            .expect("tests/fixtures/ENC_ROOT")
            .flatten()
        {
            let dst = dir.join("ENC_ROOT").join(cell.file_name());
            std::fs::create_dir_all(&dst).expect("a temp dir");
            for f in std::fs::read_dir(cell.path())
                .expect("a fixture cell")
                .flatten()
            {
                std::fs::copy(f.path(), dst.join(f.file_name())).expect("a fixture copy");
            }
        }
        let lib = Library::open(&dir, &|_, _| {});
        Charts { lib, dir }
    }
}

impl Drop for Charts {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
