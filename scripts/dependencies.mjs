#!/usr/bin/env node
// Regenerate docs/legal/dependencies.md from both Cargo workspaces. Run after
// changing a dependency: `direnv exec . node scripts/dependencies.mjs`.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

const root = new URL("../", import.meta.url);

function packages(dir) {
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
      cwd: new URL(dir, root),
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    }),
  );
  const members = new Set(metadata.workspace_members);
  return metadata.packages.filter((entry) => !members.has(entry.id));
}

const seen = new Map();
for (const dir of [".", "app/src-tauri"]) {
  for (const entry of packages(dir)) {
    const key = `${entry.name}@${entry.version}`;
    if (!seen.has(key)) seen.set(key, entry);
  }
}

const rows = [...seen.values()]
  .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
  .map((entry) => `| ${entry.name} | ${entry.version} | ${entry.license ?? "see crate"} |`);

writeFileSync(
  new URL("docs/legal/dependencies.md", root),
  `# Rust dependencies

Generated from both Cargo workspaces by \`node scripts/dependencies.mjs\`. It lists every crate the two lockfiles resolve, including build-only ones, so attribution errs on the generous side.

The license texts are in [LICENSES/](../../LICENSES/), and the asset and data notices are in [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) and [assets.md](assets.md).

| Crate | Version | License |
|---|---|---|
${rows.join("\n")}
`,
);
console.log(`${rows.length} crates written to docs/legal/dependencies.md`);
