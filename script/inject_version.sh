#!/usr/bin/env bash
#
# Inject a computed version (from CI / GitVersion) into the workspace.
#
# - Patches the workspace.package.version line in the root Cargo.toml. Internal crates use
#   `version.workspace = true`, so a single root-level substitution is enough.
# - Patches clients/node/package.json via `npm version`.
#
# Idempotent only against the `0.0.0-dev` placeholder. CI always starts from a fresh checkout,
# so this is safe.

set -euo pipefail

VERSION="${1:-${FSCT_VERSION:-}}"
if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>" >&2
  echo "   or: FSCT_VERSION=<version> $0" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[inject_version] Setting workspace version: $VERSION"

# Cargo.toml — replace the placeholder line. Anchored to start-of-line to avoid touching
# unrelated `version = ...` lines in [workspace.dependencies].
if ! grep -qE '^version = "0\.0\.0-dev"$' Cargo.toml; then
  echo "[inject_version] ERROR: expected placeholder 'version = \"0.0.0-dev\"' not found in Cargo.toml" >&2
  echo "[inject_version] Has the placeholder been changed? Refusing to patch to avoid corruption." >&2
  exit 1
fi
ESCAPED_VERSION=$(printf '%s' "$VERSION" | sed 's/[&|\/\\]/\\&/g')
sed -i.bak -E "s|^version = \"0\\.0\\.0-dev\"$|version = \"$ESCAPED_VERSION\"|" Cargo.toml
rm -f Cargo.toml.bak

awk -v version="$VERSION" '
  /^name = "fsct-(client|core|driver)"$/ {
    in_workspace_package = 1
    print
    next
  }
  in_workspace_package && /^version = / {
    print "version = \"" version "\""
    in_workspace_package = 0
    next
  }
  /^\[\[package\]\]$/ {
    in_workspace_package = 0
  }
  { print }
' Cargo.lock > Cargo.lock.tmp
mv Cargo.lock.tmp Cargo.lock

# package.json — `npm version` is the canonical tool; it also updates package-lock.json.
# Linux .deb cross-build containers do not have npm — skip there.
if command -v npm >/dev/null 2>&1; then
  (
    cd clients/node
    npm version "$VERSION" --no-git-tag-version --allow-same-version >/dev/null
  )
  NODE_VERSION_LINE=$(grep -E '"version":' clients/node/package.json | head -n1)
else
  NODE_VERSION_LINE="(npm not available — skipped)"
fi

echo "[inject_version] Cargo.toml:        $(grep -E '^version = ' Cargo.toml | head -n1)"
echo "[inject_version] package.json:      $NODE_VERSION_LINE"
