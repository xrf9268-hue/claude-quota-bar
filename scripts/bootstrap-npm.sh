#!/usr/bin/env bash
# One-time bootstrap: publish 0.0.0-bootstrap placeholder versions of the
# 8 npm packages so that npm Trusted Publishing can be configured per
# package. Once configured, real releases (v0.1.0+) go through OIDC from
# CI and this script is never needed again.
#
# Prerequisites:
#   - npm login (interactive, 2FA OK — this runs locally, not in CI)
#   - cwd at repo root

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BOOTSTRAP_VERSION="0.0.0-bootstrap"

PLATFORMS=(
  darwin-arm64
  darwin-x64
  linux-x64
  linux-x64-musl
  linux-arm64
  linux-arm64-musl
  win32-x64
)

publish_platform() {
  local plat="$1"
  local dir="npm/platforms/${plat}"
  echo "==> bootstrap claude-quota-bar-${plat}"

  # Create a placeholder bin/ so npm has something to ship.
  mkdir -p "${dir}/bin"
  if [ ! -e "${dir}/bin/.placeholder" ]; then
    printf 'bootstrap placeholder — real binary lands in v0.1.0+\n' > "${dir}/bin/.placeholder"
  fi

  # Patch version in-place for the bootstrap publish (revert after).
  local original
  original="$(cat "${dir}/package.json")"
  node -e "
    const fs = require('fs');
    const p = '${dir}/package.json';
    const j = JSON.parse(fs.readFileSync(p, 'utf8'));
    j.version = '${BOOTSTRAP_VERSION}';
    fs.writeFileSync(p, JSON.stringify(j, null, 2) + '\n');
  "

  ( cd "${dir}" && npm publish --access=public )

  # Restore original 0.1.0 (will be re-patched by CI's prepare-packages.js).
  echo "${original}" > "${dir}/package.json"
}

publish_umbrella() {
  local dir="npm/main"
  echo "==> bootstrap claude-quota-bar (umbrella)"

  local original
  original="$(cat "${dir}/package.json")"
  node -e "
    const fs = require('fs');
    const p = '${dir}/package.json';
    const j = JSON.parse(fs.readFileSync(p, 'utf8'));
    j.version = '${BOOTSTRAP_VERSION}';
    if (j.optionalDependencies) {
      for (const k of Object.keys(j.optionalDependencies)) {
        j.optionalDependencies[k] = '${BOOTSTRAP_VERSION}';
      }
    }
    fs.writeFileSync(p, JSON.stringify(j, null, 2) + '\n');
  "

  ( cd "${dir}" && npm publish --access=public )

  echo "${original}" > "${dir}/package.json"
}

# Sanity check: user is logged in
if ! npm whoami >/dev/null 2>&1; then
  echo "Not logged in to npm. Run: npm login" >&2
  exit 1
fi

echo "Logged in as: $(npm whoami)"
echo "Publishing 8 placeholder packages at ${BOOTSTRAP_VERSION}..."
echo

for plat in "${PLATFORMS[@]}"; do
  publish_platform "${plat}"
done

publish_umbrella

echo
echo "Done. Next steps:"
echo "  1. https://www.npmjs.com/package/claude-quota-bar/access — add Trusted Publisher"
echo "  2. Repeat for each claude-quota-bar-<platform> package"
echo "  3. See docs/npm-trusted-publishing.md for details"
echo "  4. (optional) npm logout"
