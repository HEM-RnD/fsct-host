#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"
PACKAGE_NAME="fsct-host"
CARGO_BIN_DRIVER="fsct_driver_service"   # Adjust if different
CARGO_BIN_USER="fsct_user_client"        # Adjust if different or omit if none yet
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
VERSION=$(cd "${ROOT_DIR}" && cargo metadata --format-version 1 --no-deps | python3 -c "import sys, json; data = json.load(sys.stdin); print(next((p['version'] for p in data['packages'] if p['name'] == '${CARGO_BIN_DRIVER}'), ''))")

# Flags
SKIP_BUILD=false
SKIP_LICENSE=false

while [[ "$#" -gt 0 ]]; do
  case $1 in
    --skip-build) SKIP_BUILD=true ;;
    --skip-license) SKIP_LICENSE=true ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
  shift
done

STAGE_BASE="${ROOT_DIR}/target/deb/${PACKAGE_NAME}-${VERSION}-${ARCH}"
ROOTFS="${STAGE_BASE}"
DEBIAN_DIR="${STAGE_BASE}/DEBIAN"
BIN_DIR_SYS="${ROOTFS}/usr/bin"
BIN_DIR_USER="${ROOTFS}/usr/lib/fsct"
SYSTEMD_SYS_DIR="${ROOTFS}/lib/systemd/system"
SYSTEMD_USER_DIR="${ROOTFS}/usr/lib/systemd/user"
DOC_DIR="${ROOTFS}/usr/share/doc/${PACKAGE_NAME}"
LICENSE_DIR_SRC="${ROOT_DIR}"
PKG_TPL_DIR="${ROOT_DIR}/ports/native/packages/linux/deb"

mkdir -p "${BIN_DIR_SYS}" "${BIN_DIR_USER}" "${SYSTEMD_SYS_DIR}" "${SYSTEMD_USER_DIR}" "${DOC_DIR}" "${DEBIAN_DIR}"

if [ "${SKIP_BUILD}" = false ]; then
  echo "Building Rust binaries..."
  (cd "${ROOT_DIR}" && cargo build --package "${CARGO_BIN_DRIVER}" --release)
fi

# Locate binaries
DRIVER_SRC="${ROOT_DIR}/target/release/${CARGO_BIN_DRIVER}"
USER_SRC="${ROOT_DIR}/target/release/${CARGO_BIN_USER}"

if [ ! -f "${DRIVER_SRC}" ]; then
  echo "Error: Driver binary not found at ${DRIVER_SRC}. Adjust CARGO_BIN_DRIVER in this script."; exit 1
fi

# Install binaries
install -Dm755 "${DRIVER_SRC}" "${BIN_DIR_SYS}/fsct-driver"
# User helper is optional; if present, install to /usr/lib/fsct and our user unit will call it in %h/.local/bin if you prefer; we place here for now.
if [ -f "${USER_SRC}" ]; then
  install -Dm755 "${USER_SRC}" "${BIN_DIR_SYS}/fsct-user"
fi

# Systemd units
install -Dm644 "${PKG_TPL_DIR}/systemd/system/fsct.socket" "${SYSTEMD_SYS_DIR}/fsct.socket"
install -Dm644 "${PKG_TPL_DIR}/systemd/system/fsct.service" "${SYSTEMD_SYS_DIR}/fsct.service"
install -Dm644 "${PKG_TPL_DIR}/systemd/user/fsct-user.service" "${SYSTEMD_USER_DIR}/fsct-user.service"

# Licenses and notices
if [ "${SKIP_LICENSE}" = false ]; then
  install -Dm644 "${LICENSE_DIR_SRC}/LICENSE" "${DOC_DIR}/LICENSE"
  install -Dm644 "${LICENSE_DIR_SRC}/LICENSE-FSCT.md" "${DOC_DIR}/LICENSE-FSCT.md"
  install -Dm644 "${LICENSE_DIR_SRC}/NOTICE" "${DOC_DIR}/NOTICE"
fi

# Control files
# Copy template DEBIAN files and substitute version and arch
CONTROL_SRC="${PKG_TPL_DIR}/DEBIAN/control"
sed -e "s/^Version: .*/Version: ${VERSION}/" -e "s/^Architecture: .*/Architecture: ${ARCH}/" "${CONTROL_SRC}" > "${DEBIAN_DIR}/control"
install -Dm755 "${PKG_TPL_DIR}/DEBIAN/postinst" "${DEBIAN_DIR}/postinst"
install -Dm755 "${PKG_TPL_DIR}/DEBIAN/prerm" "${DEBIAN_DIR}/prerm"

# md5sums (optional)
(
  cd "${STAGE_BASE}"
  find . -type f ! -path "./DEBIAN/*" -print0 | xargs -0 md5sum
) > "${DEBIAN_DIR}/md5sums"

# Build deb
DEB_OUT="${ROOT_DIR}/target/deb/${PACKAGE_NAME}_${VERSION}_${ARCH}.deb"

echo "Building ${DEB_OUT}..."
dpkg-deb --build "${STAGE_BASE}" "${DEB_OUT}"

echo "Done. Install with: sudo dpkg -i ${DEB_OUT} && sudo apt-get -f install"
