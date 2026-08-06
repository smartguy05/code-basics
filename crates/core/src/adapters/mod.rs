//! Ecosystem adapters.

pub mod dotnet;
pub mod manifest;
pub mod msbuild;
pub mod node;
pub mod solution;

#[cfg(test)]
#[path = "dotnet_tests.rs"]
mod dotnet_tests;

#[cfg(test)]
#[path = "solution_tests.rs"]
mod solution_tests;

#[cfg(test)]
#[path = "msbuild_tests.rs"]
mod msbuild_tests;

#[cfg(test)]
#[path = "node_tests.rs"]
mod node_tests;
