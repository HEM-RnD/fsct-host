#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"
PACKAGE_NAME="fsct-driver"

# Flags
DEB_BUILD_OPTIONS=""
while [[ "$#" -gt 0 ]]; do
  case $1 in
    --debug) DEB_BUILD_OPTIONS="${DEB_BUILD_OPTIONS} noopt";;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
  shift
done

export DEB_BUILD_OPTIONS

echo "Building Debian package with debhelper..."
cd "${ROOT_DIR}/ports/native/packages/linux"
# Ensure required tools are present
command -v dpkg-buildpackage >/dev/null || { echo "dpkg-buildpackage not found"; exit 1; }
command -v cargo >/dev/null || { echo "cargo not found"; exit 1; }

# Build binary package without signing
dpkg-buildpackage -us -uc -b

# Move resulting .deb into target/deb at repository root
mkdir -p "${ROOT_DIR}/target/deb"
# Find the newest generated deb matching the package name (dpkg-buildpackage puts them one level up)
DEB_FILE=$(ls -1t ../*.deb 2>/dev/null | grep "/${PACKAGE_NAME}_" | head -n1 || true)
if [[ -n "${DEB_FILE}" ]]; then
  mv -f "${DEB_FILE}" "${ROOT_DIR}/target/deb/"
  echo "Package built: ${ROOT_DIR}/target/deb/$(basename "${DEB_FILE}")"
else
  echo "Error: deb file not found after build."; exit 1
fi
