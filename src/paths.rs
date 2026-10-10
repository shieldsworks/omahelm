//! `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and `XDG_DATA_HOME` count only
//! when the value is absolute. Otherwise the directory is under `$HOME`.
//! `XDG_RUNTIME_DIR` is accepted only when it is absolute. `HOME` must be
//! an absolute path when a path uses it.

use std::ffi::OsStr;
use std::path::PathBuf;

/// An absolute value of the variable is the directory. A missing, empty,
/// or relative value is `$HOME` joined with `.config`, `.cache`, or
/// `.local/share`. An absolute value leaves `HOME` unread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Xdg {
    Config,
    Cache,
    Data,
}

impl Xdg {
    fn spec(self) -> (&'static str, &'static str) {
        match self {
            Xdg::Config => ("XDG_CONFIG_HOME", ".config"),
            Xdg::Cache => ("XDG_CACHE_HOME", ".cache"),
            Xdg::Data => ("XDG_DATA_HOME", ".local/share"),
        }
    }

    fn resolve(self, value: Option<&OsStr>, home: Option<&OsStr>) -> Result<PathBuf, &'static str> {
        if let Some(dir) = absolute(value) {
            return Ok(dir);
        }
        Ok(home_from(home)?.join(self.spec().1))
    }

    pub fn base(self) -> Result<PathBuf, String> {
        let value = std::env::var_os(self.spec().0);
        let home = if absolute(value.as_deref()).is_none() {
            std::env::var_os("HOME")
        } else {
            None
        };
        self.resolve(value.as_deref(), home.as_deref())
            .map_err(str::to_owned)
    }
}

fn home_from(value: Option<&OsStr>) -> Result<PathBuf, &'static str> {
    absolute(value).ok_or("HOME must be set to an absolute path")
}

pub fn home() -> Result<PathBuf, String> {
    let value = std::env::var_os("HOME");
    home_from(value.as_deref()).map_err(str::to_owned)
}

fn runtime_from(value: Option<&OsStr>) -> Result<PathBuf, &'static str> {
    absolute(value).ok_or("XDG_RUNTIME_DIR must be set to an absolute path")
}

pub fn runtime() -> Result<PathBuf, String> {
    let value = std::env::var_os("XDG_RUNTIME_DIR");
    runtime_from(value.as_deref()).map_err(str::to_owned)
}

fn absolute(value: Option<&OsStr>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    #[test]
    fn an_absolute_xdg_value_ignores_home() {
        for xdg in [Xdg::Config, Xdg::Cache, Xdg::Data] {
            assert_eq!(
                xdg.resolve(Some(OsStr::new("/var/lib/xdg")), None).unwrap(),
                PathBuf::from("/var/lib/xdg")
            );
        }
    }

    #[test]
    fn relative_empty_and_missing_xdg_use_home() {
        let home = Some(OsStr::new("/home/casey"));
        let cases = [
            (Xdg::Config, ".config"),
            (Xdg::Cache, ".cache"),
            (Xdg::Data, ".local/share"),
        ];
        for (xdg, under) in cases {
            let expect = PathBuf::from("/home/casey").join(under);
            assert_eq!(xdg.resolve(None, home).unwrap(), expect);
            assert_eq!(xdg.resolve(Some(OsStr::new("")), home).unwrap(), expect);
            assert_eq!(xdg.resolve(Some(OsStr::new("rel")), home).unwrap(), expect);
        }
    }

    #[test]
    fn a_home_that_is_not_absolute_cannot_make_a_fallback_relative() {
        let err = "HOME must be set to an absolute path";
        for home in [None, Some(OsStr::new("")), Some(OsStr::new("rel"))] {
            for xdg in [Xdg::Config, Xdg::Cache, Xdg::Data] {
                assert_eq!(xdg.resolve(None, home).unwrap_err(), err);
                assert_eq!(xdg.resolve(Some(OsStr::new("rel")), home).unwrap_err(), err);
            }
        }
    }

    #[test]
    fn home_must_be_an_absolute_path() {
        let err = "HOME must be set to an absolute path";
        assert_eq!(home_from(None).unwrap_err(), err);
        assert_eq!(home_from(Some(OsStr::new(""))).unwrap_err(), err);
        assert_eq!(home_from(Some(OsStr::new("rel"))).unwrap_err(), err);
        assert_eq!(
            home_from(Some(OsStr::new("/home/casey"))).unwrap(),
            PathBuf::from("/home/casey")
        );
    }

    #[test]
    fn runtime_must_be_an_absolute_path() {
        let err = "XDG_RUNTIME_DIR must be set to an absolute path";
        assert_eq!(runtime_from(None).unwrap_err(), err);
        assert_eq!(runtime_from(Some(OsStr::new(""))).unwrap_err(), err);
        assert_eq!(runtime_from(Some(OsStr::new("rel"))).unwrap_err(), err);
        assert_eq!(
            runtime_from(Some(OsStr::new("/run/user/1000"))).unwrap(),
            PathBuf::from("/run/user/1000")
        );
    }
}
