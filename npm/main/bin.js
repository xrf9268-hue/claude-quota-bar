#!/usr/bin/env node
// Umbrella launcher: locates the platform-specific package installed via
// optionalDependencies and exec's its binary. Falls through to a clear
// error if the user is on an unsupported platform or the platform package
// failed to install (network, permissions).

"use strict";

const { spawnSync } = require("child_process");
const path = require("path");
const fs = require("fs");

function detectLibc() {
  // Best-effort musl vs glibc detection on Linux. Alpine and other musl
  // distros ship /lib/ld-musl-* loaders; glibc has /lib*/libc.so.6 etc.
  if (process.platform !== "linux") return "gnu";
  try {
    const entries = fs.readdirSync("/lib");
    if (entries.some((e) => e.startsWith("ld-musl-"))) return "musl";
  } catch {}
  return "gnu";
}

function packageName() {
  const { platform, arch } = process;
  if (platform === "darwin" && arch === "arm64") return "claude-quota-bar-darwin-arm64";
  if (platform === "darwin" && arch === "x64") return "claude-quota-bar-darwin-x64";
  if (platform === "win32" && arch === "x64") return "claude-quota-bar-win32-x64";
  if (platform === "linux") {
    const libc = detectLibc();
    if (arch === "x64") return libc === "musl"
      ? "claude-quota-bar-linux-x64-musl"
      : "claude-quota-bar-linux-x64";
    if (arch === "arm64") return libc === "musl"
      ? "claude-quota-bar-linux-arm64-musl"
      : "claude-quota-bar-linux-arm64";
  }
  return null;
}

const pkg = packageName();
if (!pkg) {
  console.error(
    `claude-quota-bar: unsupported platform ${process.platform}/${process.arch}. ` +
      "Download a binary from https://github.com/xrf9268-hue/claude-quota-bar/releases."
  );
  process.exit(1);
}

const binName = process.platform === "win32" ? "claude-quota-bar.exe" : "claude-quota-bar";
let binPath;
try {
  binPath = require.resolve(`${pkg}/bin/${binName}`);
} catch {
  console.error(
    `claude-quota-bar: platform package ${pkg} is not installed.\n` +
      "Try reinstalling: npm install -g claude-quota-bar"
  );
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`claude-quota-bar: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
