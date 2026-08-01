//! Ecosystem adapters.

pub mod dotnet;
pub mod manifest;
pub mod node;

#[cfg(test)]
#[path = "dotnet_tests.rs"]
mod dotnet_tests;

#[cfg(test)]
#[path = "node_tests.rs"]
mod node_tests;
