#!/usr/bin/env node
// Verifies that every relative link and image target in repository Markdown
// exists. Remote links, mail links and bare anchors are skipped.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const topLevel = [
  "README.md",
  "CONTRIBUTING.md",
  "SECURITY.md",
  "THIRD_PARTY_NOTICES.md",
  "CHANGELOG.md",
  "AGENTS.md",
];

function markdownFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) {
      return entry.name === "node_modules" ? [] : markdownFiles(path);
    }
    return entry.name.endsWith(".md") ? [path] : [];
  });
}

function lineAt(text, index) {
  let line = 1;
  for (let cursor = 0; cursor < index; cursor += 1) {
    if (text[cursor] === "\n") line += 1;
  }
  return line;
}

const docs = markdownFiles(resolve(root, "docs"));
const files = [
  ...topLevel.map((name) => resolve(root, name)),
  ...docs,
];
const failures = [];
let checked = 0;

for (const file of files) {
  if (!existsSync(file)) continue;
  const text = readFileSync(file, "utf8").replace(/```[\s\S]*?```/g, (block) =>
    "\n".repeat(block.split("\n").length - 1),
  );
  const linkPattern = /!?\[[^\]]*\]\(([^)]+)\)/g;
  for (const match of text.matchAll(linkPattern)) {
    const raw = match[1].trim();
    const target = raw.replace(/^<|>$/g, "").split(/\s+/)[0];
    if (!target || /^(?:https?:|mailto:|tel:|#|\/\/)/.test(target)) continue;
    const path = target.split(/[?#]/)[0];
    if (!path) continue;
    checked += 1;
    let decoded = path;
    try {
      decoded = decodeURIComponent(path);
    } catch {
      // Keep the raw target when it is not valid percent-encoding.
    }
    if (!existsSync(resolve(dirname(file), decoded))) {
      failures.push(`${relative(root, file)}:${lineAt(text, match.index)} -> ${target}`);
    }
  }
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  console.error(`${failures.length} broken link(s) in ${checked} checked`);
  process.exit(1);
}
console.log(`${checked} links checked in ${files.length} Markdown files`);
