#!/usr/bin/env node
/**
 * Publish the object-inspector sidecar into the Tauri bundle.
 *
 * Usage: node scripts/build-sidecar.mjs   (or: pnpm sidecar:build)
 *
 * Produces src-tauri/resources/inspector/cb-inspector-win-{x64,x86}.exe, which
 * tauri.conf.json ships as a bundled resource. Two architectures because ClrMD
 * can only read a target of its own bitness.
 *
 * Framework-dependent single-file (~4 MB each), not self-contained: every
 * machine that runs a .NET dev tool already has the runtime, and
 * self-contained would add ~70 MB per architecture to the installer.
 *
 * **Missing .NET is not an error.** code-basics builds and runs without the
 * inspector; the feature reports itself unavailable and everything else works.
 * Failing the whole build over an optional component would be the wrong
 * trade — the same reasoning as `adapters/msbuild.rs`, where every failure
 * degrades to the shallow scan rather than breaking the workspace.
 */

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const root = join(fileURLToPath(import.meta.url), "..", "..");
const project = join(root, "sidecar", "inspector", "Inspector.csproj");
const outDir = join(root, "src-tauri", "resources", "inspector");

/** Runtime identifier to the name the Rust side looks for. */
const TARGETS = [
  { rid: "win-x64", name: "cb-inspector-win-x64.exe" },
  { rid: "win-x86", name: "cb-inspector-win-x86.exe" },
];

function hasDotnet() {
  try {
    execFileSync("dotnet", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function publish({ rid, name }) {
  const staging = join(tmpdir(), `cb-inspector-${rid}-${process.pid}`);
  rmSync(staging, { recursive: true, force: true });

  execFileSync(
    "dotnet",
    [
      "publish",
      project,
      "-c", "Release",
      "-r", rid,
      // `-p:SelfContained=false`, NOT `--self-contained false`. Combined with
      // PublishSingleFile the CLI flag is silently ignored and the whole
      // runtime is bundled — 74 MB per architecture instead of 4.
      "-p:SelfContained=false",
      "-p:PublishSingleFile=true",
      "-p:DebugType=none",
      "--nologo",
      "-v", "quiet",
      "-o", staging,
    ],
    { stdio: "inherit" },
  );

  // The assembly name is architecture-neutral, so the two builds would
  // overwrite each other in the bundle; the RID goes into the file name here
  // rather than into the csproj, which keeps `dotnet build` usable directly.
  const published = join(staging, "cb-inspector.exe");
  if (!existsSync(published)) {
    throw new Error(`dotnet publish produced no cb-inspector.exe in ${staging}`);
  }

  mkdirSync(outDir, { recursive: true });
  copyFileSync(published, join(outDir, name));
  rmSync(staging, { recursive: true, force: true });

  console.log(`  ${name}`);
}

if (!hasDotnet()) {
  console.warn(
    "sidecar: no .NET SDK on PATH — skipping the object inspector.\n" +
      "         code-basics will build and run; object inspection will report\n" +
      "         itself unavailable until this is built.",
  );
  process.exit(0);
}

console.log("sidecar: publishing the object inspector");
let built = 0;
for (const target of TARGETS) {
  try {
    publish(target);
    built++;
  } catch (error) {
    // One architecture failing must not cost the other. A machine without the
    // x86 targeting pack is common and only matters for 32-bit targets.
    console.warn(`sidecar: could not publish ${target.rid}: ${error.message}`);
  }
}

if (built === 0) {
  console.warn("sidecar: nothing was published; object inspection will be unavailable.");
}
