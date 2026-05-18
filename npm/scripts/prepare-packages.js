#!/usr/bin/env node
// Extract release artifacts into platform-specific NPM packages and
// publish them. Designed to run inside the release.yml `publish-npm`
// job after artifacts have been downloaded via actions/download-artifact.
//
// Layout assumed in CWD:
//   artifacts/claude-quota-bar-<target>/<binary>.tar.gz   (or .zip for windows)
//   npm/main/                                          (umbrella)
//   npm/platforms/<plat>/                              (per-platform stubs)

"use strict";

const fs = require("fs");
const path = require("path");
const { execSync, spawnSync } = require("child_process");

const ROOT = path.resolve(__dirname, "..", "..");
const VERSION = readVersionFromTag();

// target → npm platform dir name
const MAP = {
  "aarch64-apple-darwin": "darwin-arm64",
  "x86_64-apple-darwin": "darwin-x64",
  "x86_64-unknown-linux-gnu": "linux-x64",
  "x86_64-unknown-linux-musl": "linux-x64-musl",
  "aarch64-unknown-linux-gnu": "linux-arm64",
  "aarch64-unknown-linux-musl": "linux-arm64-musl",
  "x86_64-pc-windows-gnu": "win32-x64",
};

function readVersionFromTag() {
  const ref = process.env.GITHUB_REF || "";
  const match = ref.match(/^refs\/tags\/v(.+)$/);
  if (match) return match[1];
  const cargo = fs.readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
  const m = cargo.match(/^version\s*=\s*"([^"]+)"/m);
  if (m) return m[1];
  throw new Error("Cannot determine version: not on a tag and no Cargo.toml version found");
}

function patchVersion(pkgJsonPath, version) {
  const raw = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
  raw.version = version;
  if (raw.optionalDependencies) {
    for (const k of Object.keys(raw.optionalDependencies)) {
      raw.optionalDependencies[k] = version;
    }
  }
  fs.writeFileSync(pkgJsonPath, JSON.stringify(raw, null, 2) + "\n");
}

function extractArtifact(target, platDir) {
  const artifactDir = path.join(ROOT, "artifacts", `claude-quota-bar-${target}`);
  const files = fs.readdirSync(artifactDir);
  const archive = files.find((f) => f.endsWith(".tar.gz") || f.endsWith(".zip"));
  if (!archive) throw new Error(`No archive in ${artifactDir}`);
  const archivePath = path.join(artifactDir, archive);

  const binDir = path.join(ROOT, "npm", "platforms", platDir, "bin");
  fs.mkdirSync(binDir, { recursive: true });

  if (archive.endsWith(".tar.gz")) {
    execSync(`tar -xzf "${archivePath}" -C "${binDir}"`, { stdio: "inherit" });
  } else {
    execSync(`unzip -o "${archivePath}" -d "${binDir}"`, { stdio: "inherit" });
  }
  // Ensure binary is executable
  const binName = platDir.startsWith("win32") ? "claude-quota-bar.exe" : "claude-quota-bar";
  const binPath = path.join(binDir, binName);
  if (fs.existsSync(binPath)) {
    fs.chmodSync(binPath, 0o755);
  } else {
    throw new Error(`Binary not found after extraction: ${binPath}`);
  }
}

function npmPublish(cwd) {
  console.log(`npm publish from ${cwd}`);
  const res = spawnSync("npm", ["publish", "--access=public"], { cwd, stdio: "inherit" });
  if (res.status !== 0) {
    throw new Error(`npm publish failed in ${cwd}`);
  }
}

function main() {
  console.log(`Preparing NPM packages for version ${VERSION}`);

  // Patch + publish each platform package
  for (const [target, plat] of Object.entries(MAP)) {
    const platDir = path.join(ROOT, "npm", "platforms", plat);
    if (!fs.existsSync(platDir)) {
      console.warn(`Platform dir missing: ${platDir}, skipping`);
      continue;
    }
    extractArtifact(target, plat);
    patchVersion(path.join(platDir, "package.json"), VERSION);
    // Copy README so npm shows something
    const readme = path.join(ROOT, "README.md");
    if (fs.existsSync(readme)) {
      fs.copyFileSync(readme, path.join(platDir, "README.md"));
    }
    npmPublish(platDir);
  }

  // Patch + publish umbrella package
  const mainDir = path.join(ROOT, "npm", "main");
  patchVersion(path.join(mainDir, "package.json"), VERSION);
  const readme = path.join(ROOT, "README.md");
  if (fs.existsSync(readme)) {
    fs.copyFileSync(readme, path.join(mainDir, "README.md"));
  }
  npmPublish(mainDir);

  console.log(`Published claude-quota-bar@${VERSION} + 7 platform packages.`);
}

main();
