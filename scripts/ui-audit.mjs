#!/usr/bin/env node
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../app/src/", import.meta.url));
const files = [];

function walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path);
    else if (/\.(vue|ts|css)$/.test(entry.name)) files.push(path);
  }
}

walk(root);

const checks = [
  {
    name: "raw font sizes",
    pattern: /font-size:\s*[\d.]+px/g,
    fail: true,
    hint: "use a --text-* token",
  },
  {
    name: "cryptic order notation",
    pattern: /\b[ChAC]\$\{/g,
    fail: true,
    hint: "use an icon or a labelled value",
  },
  {
    name: "letter emblem",
    pattern: /\.name\.charAt\(0\)/g,
    fail: true,
    hint: "use ItemArt or TierMark",
  },
  {
    name: "dead settings marker",
    pattern: /(Midnight|Overlay duration|Compact item cards|Start with Warframe|Minimize to tray)/g,
    fail: true,
    hint: "removed in the settings cleanup",
  },
  {
    name: "unlabelled icon button",
    pattern: /<button(?![^>]*aria-label)[^>]*class="[^"]*icon-button/g,
    fail: false,
    hint: "check the accessible name",
  },
];

let failed = false;

for (const check of checks) {
  const hits = [];
  for (const file of files) {
    const text = readFileSync(file, "utf8");
    text.split("\n").forEach((line, index) => {
      if (check.pattern.test(line)) {
        hits.push(`${file.slice(root.length)}:${index + 1}  ${line.trim()}`);
      }
      check.pattern.lastIndex = 0;
    });
  }
  if (!hits.length) continue;
  if (check.fail) failed = true;
  console.log(`${check.fail ? "FAIL" : "WARN"} ${check.name} — ${check.hint}`);
  for (const hit of hits) console.log(`  ${hit}`);
}

if (!failed) console.log("UI audit passed.");
process.exit(failed ? 1 : 0);
