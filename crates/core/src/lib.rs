//! Core logic for `code-basics`.
//!
//! This crate deliberately has **no Tauri dependency**. Everything that decides
//! anything — project detection, building command lines, parsing test reports,
//! git operations — lives here so it can be unit tested without a windowing
//! system, and `src-tauri` stays a thin bridge between these functions and the
//! webview.

pub mod adapters;
pub mod config;
pub mod files;
pub mod git;
pub mod importers;



pub mod model;
pub mod process;
pub mod secrets;
pub mod testing;
pub mod workspace;

pub use model::*;
