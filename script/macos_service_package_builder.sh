#!/bin/bash

# Get script directory and project root
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"



# Configuration variables
TARGET_NAME="fsct_native_service"                      # Name of the binary target for Cargo
APP_NAME="fsct_service"                                # Application name (final binary name)
IDENTIFIER="com.hem-e.fsctservice"                     # Unique package identifier
VERSION="0.2.0"                                        # Application version
BUILD_DIR="${ROOT_DIR}/target/release"                             # Directory where Cargo build produces the binary
INSTALL_DIR="/usr/local/bin"                           # Target install directory for the binary
DAEMON_DIR="/Library/LaunchDaemons"                    # Target install directory for the plist
PKG_NAME="${APP_NAME}-${VERSION}.pkg"                  # Output package name
INSTALLER_FILES_DIR="ports/native/packages/macos"      # Directory with prepared files (plist, postinstall)

# Code signing certificate (ensure the certificate is installed in your Keychain)
DEVELOPER_ID="Developer ID Installer: Your Name (TeamID)"

# Temporary directory for building the package, located in target/package
PACKAGE_DIR="${BUILD_DIR}/package"
PACKAGE_ROOT="${PACKAGE_DIR}/root"
BIN_PACKAGE_DIR="${PACKAGE_ROOT}${INSTALL_DIR}"
DAEMON_PACKAGE_DIR="${PACKAGE_ROOT}${DAEMON_DIR}"
SCRIPTS_PACKAGE_DIR="${PACKAGE_ROOT}/scripts"

# Exit the script if any command fails
set -e

echo "========================================"
echo "Compiling the application in release mode..."
(cd "${ROOT_DIR}" && cargo build --release --bin ${TARGET_NAME})

echo "========================================"
echo "Preparing package structure..."

# Clean up any previous package structure
rm -rf "${PACKAGE_ROOT}"
mkdir -p "${BIN_PACKAGE_DIR}" "${DAEMON_PACKAGE_DIR}" "${SCRIPTS_PACKAGE_DIR}"

# Copy the built executable and rename it to APP_NAME
echo "Copying the binary..."
cp "${BUILD_DIR}/${TARGET_NAME}" "${BIN_PACKAGE_DIR}/${APP_NAME}"

# Copy the prepared files: plist and postinstall script
echo "Copying prepared files..."
if [ -d "${INSTALLER_FILES_DIR}" ]; then
    if [ -f "${INSTALLER_FILES_DIR}/com.hem-e.fsctservice.xml" ]; then
        cp "${INSTALLER_FILES_DIR}/com.hem-e.fsctservice.xml" "${DAEMON_PACKAGE_DIR}/com.hem-e.fsctservice.plist"
    else
        echo "File com.hem-e.fsctservice.xml not found in ${INSTALLER_FILES_DIR}!"
        exit 1
    fi

    if [ -f "${INSTALLER_FILES_DIR}/service_setup_script.sh" ]; then
        cp "${INSTALLER_FILES_DIR}/service_setup_script.sh" "${SCRIPTS_PACKAGE_DIR}/postinstall"
        chmod +x "${SCRIPTS_PACKAGE_DIR}/postinstall"
    else
        echo "File service_setup_script.sh not found in ${INSTALLER_FILES_DIR}!"
        exit 1
    fi
else
    echo "Directory ${INSTALLER_FILES_DIR} does not exist!"
    exit 1
fi

echo "========================================"
echo "Building the .pkg package..."
pkgbuild --root "${PACKAGE_ROOT}" \
         --identifier "${IDENTIFIER}" \
         --version "${VERSION}" \
         --scripts "${SCRIPTS_PACKAGE_DIR}" \
         --install-location "/" \
         "${PACKAGE_DIR}/${PKG_NAME}"

#echo "========================================"
#echo "Code signing the package using codesign..."
#codesign --sign "${DEVELOPER_ID}" --timestamp --options runtime "${PKG_NAME}"
#
#echo "Verifying package signature..."
#spctl -a -v --type install "${PKG_NAME}"

echo "========================================"
echo "Done! Package created and signed: ${PKG_NAME}"