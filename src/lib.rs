//! Omahelm, the Omahoy chartplotter: NOAA electronic navigational charts,
//! read and drawn from scratch.

/// A poisoned lock means another thread panicked while holding it.
#[expect(
    clippy::expect_used,
    reason = "a poisoned lock means another thread already panicked"
)]
pub(crate) fn guard<T>(result: Result<T, std::sync::PoisonError<T>>) -> T {
    result.expect("a poisoned lock means another thread already panicked")
}

pub mod chart;
pub mod fetch;
pub mod geo;
pub mod iso8211;
pub mod library;
pub mod marks;
pub mod paths;
pub mod render;
pub mod s57;
pub mod server;
pub mod style;
pub mod text;
pub mod trips;
