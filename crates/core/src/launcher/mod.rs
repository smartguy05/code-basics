//! The app launcher: arbitrary command lines the user wants to run beside the
//! detected configurations, and the memory of what they have run before.
//!
//! This exists **next to** `config`/`invocation`, not inside them, and the split
//! is the point. A [`crate::model::RunConfig`] has no program of its own — every
//! path resolves one through an ecosystem adapter, and an adapter only speaks for
//! a project it detected. A local Redis, a Python script or `docker compose up`
//! belongs to no project, so giving it a `RunConfig` would mean either inventing
//! a fake ecosystem or letting a free-form program into the model everything
//! holds. Instead a launchable is its own small thing that resolves *directly*
//! into an [`crate::model::Invocation`], which the existing
//! [`crate::process::Supervisor`] already knows how to run headlessly and track
//! in the Running panel.
//!
//! The store is **user-global, not per-workspace** — `code-basics/launchers.json`
//! beside `notes.json`, resolved by [`store::launchers_path`]. "The commands I
//! run" is a property of the person, not of a repository, and writing them into a
//! checked-in `.code-basics/config.json` would share one developer's local tool
//! shortcuts with their whole team. Each entry records the `cwd` it ran in, which
//! is what lets [`recents::group`] show the open codebase's commands first
//! without a second, per-repository store.
//!
//! Layering mirrors the rest of the crate: `model` is pure data + serde with its
//! camelCase keys pinned by a test, `parse` and `recents` are pure decisions, and
//! `store` is the only filesystem seam. Same abstain rule as everywhere else — a
//! command line that cannot be split is an **error naming the problem**, never a
//! guess at what the user meant, and a command needing a shell is refused with
//! the fix rather than silently run as a bare argv that would treat `|` as an
//! argument.

pub mod model;
pub mod parse;
pub mod recents;
pub mod store;

pub use model::{Launchable, LauncherFile, LauncherGroups};
pub use parse::{program_and_args, shell_args, shell_flag, split_command};
pub use recents::{group, record_run, remove, rename, set_pinned, within_root, MAX_UNPINNED};
pub use store::{launchers_path, load, save, LAUNCHERS_FILE};
