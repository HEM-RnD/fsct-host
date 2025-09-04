#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"
PACKAGE_NAME="fsct-driver"
PKG_DIR_REL="ports/native/packages/linux"
BUILD_ROOT="${ROOT_DIR}/target/deb/build"
BUILD_DIR="${BUILD_ROOT}/fsct-host-src"

# Flags
DEB_BUILD_OPTIONS=""
ALLOW_MISSING_DEPS=false
KEEP_BUILD=false
while [[ "$#" -gt 0 ]]; do
  case $1 in
    --debug) DEB_BUILD_OPTIONS="${DEB_BUILD_OPTIONS} noopt";;
    --allow-missing-deps) ALLOW_MISSING_DEPS=true;;
    --keep-build) KEEP_BUILD=true;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
  shift
done

export DEB_BUILD_OPTIONS

# Preflight: dependencies (don't auto-install; instruct the user)
need() { command -v "$1" >/dev/null 2>&1; }
missing=()
need dpkg-buildpackage || missing+=("dpkg-dev (dpkg-buildpackage)")
need dh || missing+=("debhelper (dh)")
need pkg-config || missing+=("pkg-config")
need rustc || missing+=("rustc (via rustup preferred)")
need cargo || missing+=("cargo (via rustup preferred)")

if (( ${#missing[@]} > 0 )); then
  echo "Error: missing required tools:" >&2
  for m in "${missing[@]}"; do echo "  - $m" >&2; done
  echo >&2
  echo "Install build deps (Debian/Ubuntu): sudo apt-get install -y build-essential debhelper dpkg-dev pkg-config" >&2
  echo "Install Rust toolchain via rustup (recommended): curl https://sh.rustup.rs -sSf | sh; source \"$HOME/.cargo/env\"" >&2
  if [[ "$ALLOW_MISSING_DEPS" != true ]]; then
    exit 1
  fi
  echo "Proceeding despite missing deps due to --allow-missing-deps" >&2
fi

# Ensure out-of-source build directory
mkdir -p "${BUILD_ROOT}"
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}"

# Copy a snapshot of the source into BUILD_DIR (excluding target and .git)
rsync -a --delete --exclude '/target' --exclude '/.git' "${ROOT_DIR}/" "${BUILD_DIR}/"

echo "Building Debian package with debhelper (out-of-source)..."
cd "${BUILD_DIR}/${PKG_DIR_REL}"

# Build binary package without signing
# dpkg-buildpackage will place artifacts one level up from the package dir
DEB_BUILD_OPTIONS="${DEB_BUILD_OPTIONS}" dpkg-buildpackage -us -uc -b

# Move resulting .deb into target/deb at repository root
mkdir -p "${ROOT_DIR}/target/deb"
DEB_FILE=$(ls -1t ../*.deb 2>/dev/null | grep "/${PACKAGE_NAME}_" | head -n1 || true)
if [[ -n "${DEB_FILE}" ]]; then
  mv -f "${DEB_FILE}" "${ROOT_DIR}/target/deb/"
  echo "Package built: ${ROOT_DIR}/target/deb/$(basename "${DEB_FILE}")"
else
  echo "Error: deb file not found after build."; exit 1
fi

# Optionally clean build directory
if [[ "$KEEP_BUILD" != true ]]; then
  rm -rf "${BUILD_DIR}"
fi
