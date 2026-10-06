#!/usr/bin/env node
// Writes third_party/NOTICES.md: the license texts and copyright notices of every third-party
// component compiled into the app or bundled into its UI, which their licenses (MIT, BSD, ISC,
// Apache-2.0 and others) require in every copy of the binary. The app bundles the file next to
// LICENSE and THIRD_PARTY.md (bundle.resources in src-tauri/tauri.conf.json).
//
// Usage: node scripts/third-party-notices.mjs [--check]
//
//   --check   compare with the committed file and fail when it is out of date (CI runs this).
//
// Rust: `cargo metadata` for each release target; every crate reachable from the app through
// normal (not build or dev) dependencies on that target. Crates compiled only into build scripts
// do not ship. Each crate's own license, notice and copyright files are copied; whisper.cpp and
// ggml (compiled from whisper-rs-sys's sources) and SQLite (from libsqlite3-sys) are added from
// the sources the crates build. A crate published without a license file is listed with the
// license its manifest names. UI: the production npm dependencies (`npm ls --omit=dev`), which
// Vite bundles into the app's JavaScript. Each distinct text is printed once and referred to by
// number. FFmpeg's notice is in third_party/ffmpeg/.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "third_party", "NOTICES.md");
const TARGETS = ["aarch64-apple-darwin", "x86_64-pc-windows-msvc"];
const APP_PACKAGE = "media-identifier";
const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-._].*)?$/i;
// Sources a crate compiles but keeps in a subfolder, with their own license files. whisper.cpp's
// LICENSE covers ggml too ("The ggml authors").
const VENDORED = {
  "whisper-rs-sys": ["whisper.cpp/LICENSE"],
};
// Components without a license file whose terms the notice states in words.
const STATED = {
  "libsqlite3-sys":
    "Includes SQLite (https://sqlite.org), which is in the public domain: \"The author disclaims copyright to this source code.\"",
};

function run(cmd, args, cwd = ROOT) {
  return execFileSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
}

function licenseFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((name) => LICENSE_FILE.test(name) && statSync(join(dir, name)).isFile())
    .sort()
    .map((name) => join(dir, name));
}

function normalise(text) {
  return text.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();
}

/** Crates reachable from the app through normal dependencies on `target`. */
function rustCrates(target) {
  const meta = JSON.parse(
    run("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", target]),
  );
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const start = meta.packages.find((p) => p.name === APP_PACKAGE && members.has(p.id));
  if (!start) throw new Error(`package ${APP_PACKAGE} not found`);
  const seen = new Set([start.id]);
  const queue = [start.id];
  while (queue.length > 0) {
    const node = nodes.get(queue.shift());
    for (const dep of node?.deps ?? []) {
      const normal = dep.dep_kinds.some((k) => k.kind === null);
      if (normal && !seen.has(dep.pkg)) {
        seen.add(dep.pkg);
        queue.push(dep.pkg);
      }
    }
  }
  return [...seen]
    .filter((id) => !members.has(id))
    .map((id) => byId.get(id))
    .map((p) => {
      const dir = dirname(p.manifest_path);
      const files = [...licenseFiles(dir), ...(VENDORED[p.name] ?? []).map((f) => join(dir, f))];
      return {
        name: p.name,
        version: p.version,
        license: p.license ?? (p.license_file ? `see ${p.license_file}` : "not stated"),
        repository: p.repository ?? p.homepage ?? null,
        texts: files.filter(existsSync).map((f) => ({
          file: relative(dir, f),
          text: normalise(readFileSync(f, "utf8")),
        })),
        stated: STATED[p.name] ?? null,
      };
    });
}

/** Production npm packages bundled into the UI. */
function npmPackages() {
  const ui = join(ROOT, "ui");
  const tree = JSON.parse(run("npm", ["ls", "--omit=dev", "--all", "--json"], ui));
  const found = new Map();
  const walk = (deps) => {
    for (const [name, info] of Object.entries(deps ?? {})) {
      const key = `${name}@${info.version}`;
      if (!found.has(key)) {
        found.set(key, { name, version: info.version });
        walk(info.dependencies);
      }
    }
  };
  walk(tree.dependencies);
  return [...found.values()].map(({ name, version }) => {
    const dir = join(ui, "node_modules", name);
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    return {
      name: `${name} (npm)`,
      version,
      license: typeof pkg.license === "string" ? pkg.license : "not stated",
      repository: typeof pkg.repository === "string" ? pkg.repository : (pkg.repository?.url ?? null),
      texts: licenseFiles(dir).map((f) => ({ file: relative(dir, f), text: normalise(readFileSync(f, "utf8")) })),
      stated: null,
    };
  });
}

function render(components) {
  const byKey = new Map();
  for (const c of components) byKey.set(`${c.name}@${c.version}`, c);
  const list = [...byKey.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
  // Each distinct text is printed once and numbered; components refer to the numbers, because
  // hundreds of crates ship the same Apache-2.0 text.
  const numbers = new Map();
  const texts = [];
  const number = (text) => {
    if (!numbers.has(text)) {
      numbers.set(text, texts.length + 1);
      texts.push(text);
    }
    return numbers.get(text);
  };
  const lines = [
    "# Third-party notices",
    "",
    "Media Identifier includes the following third-party components. Their licenses ask that these",
    "notices accompany every copy of the app. Each component lists the numbers of its license and",
    "notice texts, which follow the list. FFmpeg, included as separate programs, has its own notice",
    "in `ffmpeg/NOTICE.md`. This file is generated by `scripts/third-party-notices.mjs`; do not edit",
    "it.",
    "",
    "## Components",
    "",
  ];
  for (const c of list) {
    const where = c.repository ? ` (${c.repository})` : "";
    const refs = c.texts.map((t) => `${t.file}: [${number(t.text)}](#text-${number(t.text)})`);
    const extra = c.stated ? ` ${c.stated}` : refs.length === 0 ? " No license file is included in the package." : "";
    lines.push(`- **${c.name} ${c.version}**, ${c.license}${where}${refs.length ? `. ${refs.join(", ")}` : "."}${extra}`);
  }
  lines.push("", "## Texts", "");
  texts.forEach((text, i) => {
    lines.push(`### Text ${i + 1}`, "", `<a id="text-${i + 1}"></a>`, "", "```text", text, "```", "");
  });
  return lines.join("\n");
}

const components = [...TARGETS.flatMap(rustCrates), ...npmPackages()];
const output = render(components);
if (process.argv.includes("--check")) {
  const current = existsSync(OUT) ? readFileSync(OUT, "utf8") : "";
  if (current !== output) {
    console.error("third_party/NOTICES.md is out of date: run node scripts/third-party-notices.mjs");
    process.exit(1);
  }
  console.log("third_party/NOTICES.md is current");
} else {
  writeFileSync(OUT, output);
  console.log(`wrote ${relative(ROOT, OUT)} (${components.length} entries)`);
}
