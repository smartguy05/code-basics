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
pub mod browser;
pub mod changelists;
pub mod config;
pub mod dap;
pub mod editor_context;
pub mod enhancements;
pub mod erosion;
pub mod features;
pub mod files;
pub mod git;
pub mod importers;
pub mod inspect;
pub mod intents;
pub mod invocation;
pub mod launcher;
pub mod lsp;
pub mod mcp;
pub mod model;
pub mod notes;
pub mod process;
pub mod pty;
pub mod qgate;
pub mod redis;
pub mod review;
pub mod roslyn;
pub mod rules;
pub mod running;
pub mod secrets;
pub mod setup;
pub mod sql;
pub mod symbols;
pub mod tasks;
pub mod testing;
pub mod tool_gate;
pub mod workspace;

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod invocation_tests;

#[cfg(test)]
#[path = "review_tests.rs"]
mod review_tests;

pub use model::*;
