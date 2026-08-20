# code-basics documentation

Start here. Each section links to focused documents kept under 500 lines; related documents cross-link to each other.

## Getting started

- [Installation](getting-started/installation.md) — prerequisites, first run, building a distributable
- [Using the app](getting-started/using-the-app.md) — opening workspaces and a tour of the five views

## Architecture

- [Overview](architecture/overview.md) — the three layers, the dependency rule, and how data flows
- [The core crate (`cb-core`)](architecture/core-crate.md) — workspace scanning, adapters, test parsing, git, process supervision
- [The Tauri shell (`src-tauri`)](architecture/tauri-shell.md) — state, adapter dispatch, and the command surface
- [The frontend (`src/`)](architecture/frontend.md) — views, components, and IPC wrappers
- [The IPC type contract](architecture/ipc-contract.md) — how Rust and TypeScript types stay in sync
- [Live inspection](architecture/live-inspection.md) — the object-inspector sidecar, why it is one-shot, and why it is bundled

## Guides

- [Development guide](guides/development.md) — build/test commands, the local agent hooks (quality gate + intent capture), conventions, and adding a command end-to-end
- [Adding an ecosystem](guides/adding-an-ecosystem.md) — declarative adapter manifests (pytest, cargo-nextest, ...)
- [Rider import](guides/rider-import.md) — how JetBrains Rider run configurations are converted
- [Agent intent capture](guides/agent-intent-capture.md) — grouping a diff into the decisions behind it, with Claude Code or Codex
- [Instruction enhancements](guides/instruction-enhancements.md) — adding reusable instruction sections to CLAUDE.md / AGENTS.md from a menu of `.md` templates
- [Inspecting objects](guides/inspecting-objects.md) — reading the real heap of a crash dump, and what crash dump capture costs
- [Language servers](guides/language-servers.md) — which server per language, how to install each, and how to point config at one

## Reference

- [Tauri command surface](reference/commands.md) — every IPC command with parameters and return types
- [Configuration](reference/configuration.md) — `.code-basics/` layout, `config.json`, and `RunConfig` fields
- [Test report formats](reference/test-reports.md) — TRX, Jest/Vitest JSON, and JUnit XML

## Index

- [Code index](INDEX.md) — **generated** map of every source file, Tauri command, IPC wrapper, and public core API. Regenerate with `pnpm docs:index`; do not edit by hand.

## Keeping docs healthy

Two scripts in [`scripts/`](../scripts) keep this tree uniform:

- `pnpm docs:index` regenerates [INDEX.md](INDEX.md) from the source tree.
- `pnpm docs:check` fails if any doc exceeds 500 lines or contains a broken relative link.

Run both after adding source files, Tauri commands, or documents.
