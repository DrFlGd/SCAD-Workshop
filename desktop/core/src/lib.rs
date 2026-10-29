//! Core of the desktop app, kept free of any GUI so it builds and tests on its
//! own (and powers `workshop-cli`: native renders in CI, the website build's
//! catalog steps, and a local stand-in for the app's backend).
//!
//! - [`site`]: the app's own files (common fonts, engine version); the website build
//! - [`engine`], [`render`]: the bundled native OpenSCAD, render queue and cache
//! - [`ingest`], [`sitebuild`]: reading OpenSCAD projects (files, settings, metadata)
//! - [`config`]: what the app keeps on this computer (open library, preferences)
//! - [`library`]: the portable library folder; [`format`]: its JSON files' format numbers; [`meta`]: layered metadata
//! - [`sources`]: adding projects (GitHub, ZIP, folders); [`scan`], [`project`]: reading them
//! - [`catalog`]: the page's catalog from the library; [`merge`]: merging libraries
//! - [`docs`]: projects' reference documents (README as HTML, PDF, text)
//! - [`pdf`]: text from PDFs (the details in a Printables download)
//! - [`api`]: the commands the page calls

pub mod api;
pub mod backup;
pub mod catalog;
pub mod check;
pub mod content;
pub mod components;
pub mod config;
pub mod docs;
pub mod engine;
pub mod f3d;
pub mod format;
pub mod ingest;
pub mod library;
pub mod merge;
pub mod mesh;
pub mod pdf;
pub mod meta;
pub mod mitcad;
pub mod project;
pub mod scan;
pub mod sources;
pub mod render;
pub mod site;
pub mod sitebuild;

pub use engine::NativeEngine;
pub use render::{RenderEvent, RenderOutput, RenderRequest, Renderer};
pub use site::SiteDir;
pub use library::Library;

/// The app's version. The repository keeps `<major>.<minor>.0`; release builds
/// get their number from CI (tools/set_version.py), one higher each release.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
