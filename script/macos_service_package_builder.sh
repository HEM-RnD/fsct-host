#!/bin/bash

# Get script directory and project root
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"

# Configuration variables
TARGET_NAME="fsct_native_service"                      # Name of the binary target for Cargo
APP_NAME="fsct_service"                                # Application name (final binary name)
IDENTIFIER="com.hem-e.fsctservice"                     # Unique package identifier
VERSION="0.2.0"                                        # Application version
BUILD_DIR="${ROOT_DIR}/target/release"                 # Directory where Cargo build produces the binary
INSTALL_DIR="/usr/local/bin"                           # Target install directory for the binary
DAEMON_DIR="/Library/LaunchDaemons"                    # Target install directory for the plist
PKG_NAME="${APP_NAME}-${VERSION}.pkg"                  # Output package name
INSTALLER_FILES_DIR="${ROOT_DIR}/ports/native/packages/macos"  # Directory with prepared files (plist, postinstall, distribution.xml)

# Code signing certificate (ensure the certificate is installed in your Keychain)
DEVELOPER_ID="Developer ID Installer: Your Name (TeamID)"

# Temporary directory for building the package
PACKAGE_DIR="${BUILD_DIR}/package"
COMPONENT_PKGS_DIR="${PACKAGE_DIR}/components"

# Component package paths
BIN_PKG="${COMPONENT_PKGS_DIR}/bin.pkg"
DAEMON_PKG="${COMPONENT_PKGS_DIR}/daemon.pkg"

# Exit the script if any command fails
set -e

echo "========================================"
echo "Compiling the application in release mode..."
(cd "${ROOT_DIR}" && cargo build --release --bin ${TARGET_NAME})

echo "========================================"
echo "Preparing package structure..."

# Clean up any previous package structure
rm -rf "${PACKAGE_DIR}"
mkdir -p "${COMPONENT_PKGS_DIR}"

# Create temporary directories for each component
BIN_ROOT="${PACKAGE_DIR}/bin_root"
DAEMON_ROOT="${PACKAGE_DIR}/daemon_root"
SCRIPTS_DIR="${PACKAGE_DIR}/scripts"

mkdir -p "${BIN_ROOT}${INSTALL_DIR}" "${DAEMON_ROOT}${DAEMON_DIR}" "${SCRIPTS_DIR}"

# Copy the built executable and rename it to APP_NAME
echo "Copying the binary..."
cp "${BUILD_DIR}/${TARGET_NAME}" "${BIN_ROOT}${INSTALL_DIR}/${APP_NAME}"

# Check for required files
echo "Checking for required installer files..."
if [ ! -d "${INSTALLER_FILES_DIR}" ]; then
    echo "Directory ${INSTALLER_FILES_DIR} does not exist!"
    exit 1
fi

# Check for plist file
if [ ! -f "${INSTALLER_FILES_DIR}/com.hem-e.fsctservice.xml" ]; then
    echo "File com.hem-e.fsctservice.xml not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Check for postinstall script
if [ ! -f "${INSTALLER_FILES_DIR}/service_setup_script.sh" ]; then
    echo "File service_setup_script.sh not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Check for distribution.xml file
if [ ! -f "${INSTALLER_FILES_DIR}/distribution.xml" ]; then
    echo "File distribution.xml not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Copy the prepared files
echo "Copying prepared files..."
cp "${INSTALLER_FILES_DIR}/com.hem-e.fsctservice.xml" "${DAEMON_ROOT}${DAEMON_DIR}/com.hem-e.fsctservice.plist"
cp "${INSTALLER_FILES_DIR}/service_setup_script.sh" "${SCRIPTS_DIR}/postinstall"
chmod +x "${SCRIPTS_DIR}/postinstall"

echo "========================================"
echo "Building component packages..."

# Script building modification
# Prepare scripts for the daemon component
mkdir -p "${PACKAGE_DIR}/daemon_scripts"
cp "${INSTALLER_FILES_DIR}/service_setup_script.sh" "${PACKAGE_DIR}/daemon_scripts/postinstall"
chmod +x "${PACKAGE_DIR}/daemon_scripts/postinstall"

# Build component packages
pkgbuild --root "${BIN_ROOT}" \
         --identifier "${IDENTIFIER}.bin" \
         --version "${VERSION}" \
         --install-location "/" \
         "${BIN_PKG}"

pkgbuild --root "${DAEMON_ROOT}" \
         --identifier "${IDENTIFIER}.daemon" \
         --version "${VERSION}" \
         --install-location "/" \
         --scripts "${PACKAGE_DIR}/daemon_scripts" \
         "${DAEMON_PKG}"

echo "========================================"
echo "Building the distribution package with productbuild..."

# Copy the postinstall script to work with distribution.xml
cp "${SCRIPTS_DIR}/postinstall" "${COMPONENT_PKGS_DIR}/"

# Zbuduj końcowy pakiet
productbuild --distribution "${INSTALLER_FILES_DIR}/distribution.xml" \
             --package-path "${COMPONENT_PKGS_DIR}" \
             "${BUILD_DIR}/${PKG_NAME}"

# Uncomment the following to sign the package
#echo "========================================"
#echo "Code signing the package using codesign..."
#codesign --sign "${DEVELOPER_ID}" --timestamp --options runtime "${BUILD_DIR}/${PKG_NAME}"
#
#echo "Verifying package signature..."
#spctl -a -v --type install "${BUILD_DIR}/${PKG_NAME}"

echo "========================================"
echo "Done! Package created: ${BUILD_DIR}/${PKG_NAME}"
echo "To install, use: sudo installer -pkg ${BUILD_DIR}/${PKG_NAME} -target /"