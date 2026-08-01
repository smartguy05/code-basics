//! Git operations.
//!
//! # Why two implementations
//!
//! Reads go through libgit2 (`git2`): it is fast, in-process, and gives
//! structured diffs without parsing porcelain output.
//!
//! Anything touching the network — push, pull, fetch — shells out to the
//! system `git` instead. libgit2 requires the host application to implement
//! credential callbacks itself, which means reimplementing SSH agent
//! discovery, macOS Keychain, Windows Credential Manager and Git Credential
//! Manager, and getting all of them right on three platforms. The system `git`
//! already has all of that configured for the user. Shelling out means their
//! existing credentials simply work, with no prompt inside this app.
//!
//! `git apply` is likewise delegated, because it is the only correct
//! implementation of partial patch application.

pub mod patch;
pub mod repo;

pub use patch::{Direction, DiffLine, FileDiff, Hunk, LineOrigin};
pub use repo::{
    Branch, ChangeKind, Commit, ComparisonMode, FileChange, Repo, StageTarget, WorkingStatus,
};
