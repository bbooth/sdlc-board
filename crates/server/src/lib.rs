//! The board server: reads each source on an interval and serves the
//! latest snapshot, plus metrics on how fresh each source is.

pub mod access;
pub mod board;
pub mod config;
pub mod gates;
pub mod github;
pub mod github_app;
pub mod linear;
pub mod metrics;
pub mod routes;
pub mod sources;
