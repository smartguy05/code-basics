//! Core logic for `code-basics`.
//!
//! This crate deliberately has **no Tauri dependency**. Everything that decides
//! anything — project detection, building command lines, parsing test reports,
//! git operations — lives here so it can be unit tested without a windowing
//! system, and `src-tauri` stays a thin bridge between these functions and the
//! webview.

pub mod adapters;
pub mod architecture;
pub mod behavioral;
pub mod changelists;
pub mod config;
pub mod enhancements;
pub mod erosion;
pub mod files;
pub mod git;
pub mod importers;
pub mod inspect;
pub mod intents;
pub mod invocation;
pub mod lsp;
pub mod model;
pub mod process;
pub mod qgate;
pub mod review;
pub mod rules;
pub mod secrets;
pub mod symbols;
pub mod testing;
pub mod workspace;

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod invocation_tests;

#[cfg(test)]
#[path = "review_tests.rs"]
mod review_tests;

pub use model::*;
