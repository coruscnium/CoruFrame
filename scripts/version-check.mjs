#!/usr/bin/env node
// One version and one app id everywhere. Both Rust workspaces and the two JSON
// manifests carry a copy, and a tag must describe exactly what was built.
import { readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const read = (path) => readFileSync(new URL(path, root), "utf8");

function section(text, name) {
  const pattern = new RegExp(`\\[${name.replaceAll(".", "\\.")}\\]([\\s\\S]*?)(\\n\\[|$)`);
  const match = text.match(pattern);
  if (!match) throw new Error(`missing [${name}] section`);
  return match[1];
}

function versionOf(text, sectionName) {
  const match = section(text, sectionName).match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error(`missing version in [${sectionName}]`);
  return match[1];
}

const pkg = JSON.parse(read("app/package.json"));
const tauri = JSON.parse(read("app/src-tauri/tauri.conf.json"));
const versions = {
  "app/package.json": pkg.version,
  "app/src-tauri/tauri.conf.json": tauri.version,
  "Cargo.toml [workspace.package]": versionOf(read("Cargo.toml"), "workspace.package"),
  "app/src-tauri/Cargo.toml [package]": versionOf(read("app/src-tauri/Cargo.toml"), "package"),
};

const unique = new Set(Object.values(versions));
if (unique.size !== 1) {
  for (const [where, version] of Object.entries(versions)) {
    console.error(`${where}: ${version}`);
  }
  throw new Error("versions disagree");
}

const expectedId = "xyz.coruscnium.coruframe";
if (tauri.identifier !== expectedId) {
  throw new Error(`app id is ${tauri.identifier}, expected ${expectedId}`);
}
if (pkg.name !== "coruframe-app") {
  throw new Error(`package name is ${pkg.name}`);
}

// Called as `node scripts/version-check.mjs --tag v1.0.0` from the release workflow.
const tagIndex = process.argv.indexOf("--tag");
const tag = tagIndex === -1 ? undefined : process.argv[tagIndex + 1];
const version = [...unique][0];
if (tag !== undefined && tag !== `v${version}`) {
  throw new Error(`tag ${tag} does not match version v${version}`);
}

console.log(`version ${version}, app id ${expectedId}${tag ? `, tag ${tag}` : ""}`);
