#!/usr/bin/env node
/**
 * Vendor the debug adapters into the Tauri bundle.
 *
 * Usage: node scripts/fetch-debuggers.mjs   (or: pnpm debuggers:fetch)
 *
 * Produces src-tauri/resources/debuggers/{netcoredbg,js-debug}/, which
 * tauri.conf.json ships as a bundled resource so a fresh install can debug
 * .NET and Node with nothing else to download. `dap::registry` looks here
 * after the CB_DAP_* pins and before PATH.
 *
 * Both adapters are MIT and their LICENSE files travel with them — netcoredbg
 * does not ship one inside its release zip, so it is fetched from the tag.
 *
 * ## Why versions and hashes are pinned
 *
 * This is the only place in the repository where a build step downloads a
 * binary and puts it inside the installer. An unverified fetch would make
 * whatever GitHub served that morning part of the shipped product. So the
 * version and the SHA-256 are literals here, and a mismatch is a hard failure
 * rather than a warning: a corrupted or substituted archive is precisely the
 * thing this check exists to refuse.
 *
 * ## Why a network failure is *not* a build failure
 *
 * The same trade `build-sidecar.mjs` makes for a missing .NET SDK. An offline
 * or air-gapped build must still produce a working app; it just produces one
 * whose Debug button reports "no adapter installed" and says how to fix it —
 * which is exactly the behaviour that shipped before anything was bundled.
 * Failing the whole build over an optional component would be the wrong trade.
 *
 * Re-running is cheap: a stamp file records what is already extracted, and a
 * matching stamp skips the download entirely. That is what makes it safe to
 * chain into `beforeBuildCommand`.
 */

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const root = join(fileURLToPath(import.meta.url), "..", "..");
const outDir = join(root, "src-tauri", "resources", "debuggers");

/**
 * Bumped whenever the *layout* this script writes changes, so an existing
 * extraction from an older script is redone rather than trusted. It is part of
 * the stamp for that reason — a pinned version and hash alone cannot express
 * "same archive, different post-processing".
 */
const FORMAT = 2;

/**
 * What to vendor.
 *
 * `dir` is the name `dap::registry` looks for under the resource directory;
 * changing one without the other silently un-bundles an adapter, so the Rust
 * side names these same two strings in constants beside its resolver.
 */
const ADAPTERS = [
  {
    id: "netcoredbg",
    dir: "netcoredbg",
    version: "3.2.0-1092",
    url: "https://github.com/Samsung/netcoredbg/releases/download/3.2.0-1092/netcoredbg-win64.zip",
    sha256: "3c410a45fa502415203a94fcb88654af65bf8e3dac158a5527a722e7a6b9274a",
    archive: "zip",
    // The zip contains a single `netcoredbg/` directory; strip it so the
    // executable lands at <dir>/netcoredbg.exe rather than one level down.
    strip: "netcoredbg",
    // Not in the release zip, and MIT requires it in a redistribution.
    license: "https://raw.githubusercontent.com/Samsung/netcoredbg/master/LICENSE",
    expect: "netcoredbg.exe",
  },
  {
    id: "js-debug",
    dir: "js-debug",
    version: "v1.117.0",
    url: "https://github.com/microsoft/vscode-js-debug/releases/download/v1.117.0/js-debug-dap-v1.117.0.tar.gz",
    sha256: "ad8d04ede9d4b75cc290fd5438a65047a06f786d04f604b6112485b36f090772",
    archive: "tar.gz",
    strip: "js-debug",
    // Carried inside the tarball as js-debug/LICENSE.
    license: null,
    commonjs: true,
    expect: join("src", "dapDebugServer.js"),
  },
];

const stampPath = (adapter) => join(outDir, adapter.dir, ".stamp");

/** Whether this adapter is already extracted at exactly this version+hash. */
function isCurrent(adapter) {
  const stamp = stampPath(adapter);
  if (!existsSync(stamp)) return false;
  if (!existsSync(join(outDir, adapter.dir, adapter.expect))) return false;
  return readFileSync(stamp, "utf8").trim() === `${FORMAT} ${adapter.version} ${adapter.sha256}`;
}

async function download(url) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`${response.status} ${response.statusText} for ${url}`);
  return Buffer.from(await response.arrayBuffer());
}

function verify(bytes, expected, url) {
  const actual = createHash("sha256").update(bytes).digest("hex");
  if (actual !== expected) {
    // Deliberately fatal. See the header: an unverified archive is the one
    // thing this script must never place inside an installer.
    throw new Error(
      `checksum mismatch for ${url}\n  expected ${expected}\n  actual   ${actual}\n` +
        "Refusing to bundle it. If the upstream release was re-cut, update the " +
        "pinned sha256 in scripts/fetch-debuggers.mjs deliberately.",
    );
  }
}

/**
 * The extractor, and why it is not simply `tar`.
 *
 * Windows 10+ ships **bsdtar** at System32\tar.exe, which reads zip as well as
 * tar.gz — so no archive library is needed and package.json gains no
 * dependency. But a Git Bash or MSYS shell puts **GNU** tar first on PATH, and
 * GNU tar both fails to read zip and reads `C:\...` as a *remote host*
 * (`host:path`), so a bare `tar` here fails twice over with an error
 * ("Cannot connect to C: resolve failed") that names neither cause.
 *
 * So on Windows the system binary is addressed absolutely. Elsewhere plain
 * `tar` is right: only the .NET adapter is a zip, and it is Windows-only.
 */
function systemTar() {
  if (process.platform !== "win32") return "tar";
  const system32 = join(process.env.SystemRoot ?? "C:\Windows", "System32", "tar.exe");
  return existsSync(system32) ? system32 : "tar";
}

function extract(archivePath, into) {
  mkdirSync(into, { recursive: true });
  // `cwd` plus a bare filename, never an absolute path with a drive letter:
  // belt and braces against the host:path reading above.
  execFileSync(systemTar(), ["-xf", basename(archivePath)], {
    cwd: into,
    stdio: "inherit",
  });
}

async function vendor(adapter) {
  if (isCurrent(adapter)) {
    console.log(`${adapter.id} ${adapter.version} already bundled`);
    return true;
  }

  const staging = mkdtempSync(join(tmpdir(), `cb-debuggers-${adapter.id}-`));
  try {
    console.log(`Downloading ${adapter.id} ${adapter.version}…`);
    const bytes = await download(adapter.url);
    verify(bytes, adapter.sha256, adapter.url);

    const archivePath = join(staging, `archive.${adapter.archive}`);
    writeFileSync(archivePath, bytes);
    extract(archivePath, staging);
    rmSync(archivePath, { force: true });

    const extracted = adapter.strip ? join(staging, adapter.strip) : staging;
    if (!existsSync(join(extracted, adapter.expect))) {
      throw new Error(
        `${adapter.id} archive did not contain ${adapter.expect}; its layout changed`,
      );
    }

    if (adapter.license) {
      writeFileSync(join(extracted, "LICENSE"), await download(adapter.license));
    }

    // Pin the module system. js-debug's tarball ships **no** package.json, so
    // Node decides CommonJS-or-ESM by walking *up* from the script — and under
    // `pnpm tauri dev` the resources live in `target/debug/`, whose nearest
    // ancestor package.json is this repository's own `"type": "module"`. The
    // adapter is a CommonJS bundle, so it then dies on `Dynamic require of
    // "fs" is not supported` before printing a port, and the debug session
    // times out waiting for one. Writing the marker makes the answer local and
    // independent of wherever the install directory happens to sit.
    if (adapter.commonjs) {
      writeFileSync(
        join(extracted, "package.json"),
        JSON.stringify({ type: "commonjs" }, null, 2) + "\n",
      );
    }

    // Swap in whole. A half-extracted directory that still satisfies the
    // resolver would ship an adapter that cannot start.
    const target = join(outDir, adapter.dir);
    mkdirSync(dirname(target), { recursive: true });
    rmSync(target, { recursive: true, force: true });
    renameSync(extracted, target);
    writeFileSync(stampPath(adapter), `${FORMAT} ${adapter.version} ${adapter.sha256}\n`);
    console.log(`Bundled ${adapter.id} ${adapter.version} → ${target}`);
    return true;
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

let bundled = 0;
for (const adapter of ADAPTERS) {
  try {
    if (await vendor(adapter)) bundled += 1;
  } catch (error) {
    if (error instanceof Error && error.message.startsWith("checksum mismatch")) throw error;
    // Offline, rate-limited, or a changed layout: say so loudly and carry on.
    // The app still builds; Debug reports no adapter and how to install one.
    console.warn(`\n  Skipping ${adapter.id}: ${error.message}`);
    console.warn("  The build continues; Debug will report no bundled adapter.\n");
  }
}

console.log(`${bundled}/${ADAPTERS.length} debug adapters bundled.`);
