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
DEVELOPER_ID_APP="Developer ID Application: HEM Sp. z o.o. (342MS6WA5D)"
DEVELOPER_ID_INSTALLER="Developer ID Installer: HEM Sp. z o.o. (342MS6WA5D)"

KEYCHAIN_PROFILE="APPLE_NOTARY_PROFILE"

# Control flags
SKIP_SIGNING=false
SKIP_NOTARIZATION=false

# Parse command line arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --skip-signing) SKIP_SIGNING=true ;;
        --skip-notarization) SKIP_NOTARIZATION=true ;;
        *) echo "Unknown parameter: $1"; exit 1 ;;
    esac
    shift
done

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

# Sign the binary if signing is enabled
if [ "$SKIP_SIGNING" = false ]; then
    echo "========================================"
    echo "Signing the binary..."
    codesign --force --options runtime --sign "${DEVELOPER_ID_APP}" "${BIN_ROOT}${INSTALL_DIR}/${APP_NAME}"
    echo "Verifying binary signature..."
    codesign --verify --verbose "${BIN_ROOT}${INSTALL_DIR}/${APP_NAME}"
fi

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

# Check for preinstall script
if [ ! -f "${INSTALLER_FILES_DIR}/preinstall_script.sh" ]; then
    echo "File preinstall_script.sh not found in ${INSTALLER_FILES_DIR}!"
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

# Prepare scripts for the daemon component
mkdir -p "${PACKAGE_DIR}/daemon_scripts"
cp "${INSTALLER_FILES_DIR}/service_setup_script.sh" "${PACKAGE_DIR}/daemon_scripts/postinstall"
chmod +x "${PACKAGE_DIR}/daemon_scripts/postinstall"

# Prepare scripts for the bin component
mkdir -p "${PACKAGE_DIR}/bin_scripts"
cp "${INSTALLER_FILES_DIR}/preinstall_script.sh" "${PACKAGE_DIR}/bin_scripts/preinstall"
chmod +x "${PACKAGE_DIR}/bin_scripts/preinstall"

# Build component packages
if [ "$SKIP_SIGNING" = false ]; then
    echo "Building and signing component packages..."
    pkgbuild --root "${BIN_ROOT}" \
             --identifier "${IDENTIFIER}.bin" \
             --version "${VERSION}" \
             --install-location "/" \
             --scripts "${PACKAGE_DIR}/bin_scripts" \
             --sign "${DEVELOPER_ID_INSTALLER}" \
             "${BIN_PKG}"

    pkgbuild --root "${DAEMON_ROOT}" \
             --identifier "${IDENTIFIER}.daemon" \
             --version "${VERSION}" \
             --install-location "/" \
             --scripts "${PACKAGE_DIR}/daemon_scripts" \
             --sign "${DEVELOPER_ID_INSTALLER}" \
             "${DAEMON_PKG}"
else
    echo "Building unsigned component packages..."
    pkgbuild --root "${BIN_ROOT}" \
             --identifier "${IDENTIFIER}.bin" \
             --version "${VERSION}" \
             --install-location "/" \
             --scripts "${PACKAGE_DIR}/bin_scripts" \
             "${BIN_PKG}"

    pkgbuild --root "${DAEMON_ROOT}" \
             --identifier "${IDENTIFIER}.daemon" \
             --version "${VERSION}" \
             --install-location "/" \
             --scripts "${PACKAGE_DIR}/daemon_scripts" \
             "${DAEMON_PKG}"
fi

echo "========================================"
echo "Building the distribution package with productbuild..."

# Build final package with or without signing
if [ "$SKIP_SIGNING" = false ]; then
    productbuild --distribution "${INSTALLER_FILES_DIR}/distribution.xml" \
                 --package-path "${COMPONENT_PKGS_DIR}" \
                 --sign "${DEVELOPER_ID_INSTALLER}" \
                 "${BUILD_DIR}/${PKG_NAME}"
    
    echo "Verifying package signature..."
    pkgutil --check-signature "${BUILD_DIR}/${PKG_NAME}"
else
    productbuild --distribution "${INSTALLER_FILES_DIR}/distribution.xml" \
                 --package-path "${COMPONENT_PKGS_DIR}" \
                 "${BUILD_DIR}/${PKG_NAME}"
fi

# Notarize the package if enabled
if [ "$SKIP_NOTARIZATION" = false ] && [ "$SKIP_SIGNING" = false ]; then
    echo "========================================"
    echo "Submitting package for notarization..."
    
    # Create a temporary ZIP archive for notarization
    NOTARIZE_ZIP="${BUILD_DIR}/${APP_NAME}-${VERSION}.zip"
    ditto -c -k --keepParent "${BUILD_DIR}/${PKG_NAME}" "${NOTARIZE_ZIP}"
    
    # Submit for notarization
    xcrun notarytool submit "${NOTARIZE_ZIP}" \
        --keychain-profile "${KEYCHAIN_PROFILE}" \
        --wait
    
    # Check notarization result - use JSON format for reliable parsing
    NOTARIZATION_STATUS=$(xcrun notarytool history \
    --keychain-profile "${KEYCHAIN_PROFILE}" \
    --output-format json | grep -o '"status"[ ]*:[ ]*"[^"]*"' | head -n 1 | sed 's/.*"status"[ ]*:[ ]*"\([^"]*\)".*/\1/')
    
    echo "Notarization status: ${NOTARIZATION_STATUS}"
    
    if [ "${NOTARIZATION_STATUS}" = "Accepted" ]; then
        echo "Notarization successful!"
    else
        echo "Notarization failed with status: ${NOTARIZATION_STATUS}"
        exit 1
    fi
    
    echo "Notarization successful!"
    
    # Staple the notarization ticket to the package
    echo "Stapling notarization ticket to package..."
    xcrun stapler staple "${BUILD_DIR}/${PKG_NAME}"
    
    # Verify stapling
    xcrun stapler validate "${BUILD_DIR}/${PKG_NAME}"
    
    # Clean up
    rm "${NOTARIZE_ZIP}"
fi

echo "========================================"
echo "Done! Package created: ${BUILD_DIR}/${PKG_NAME}"
if [ "$SKIP_SIGNING" = false ] && [ "$SKIP_NOTARIZATION" = false ]; then
    echo "Package is signed and notarized"
elif [ "$SKIP_SIGNING" = false ]; then
    echo "Package is signed but not notarized"
else
    echo "Package is unsigned"
    echo "WARNING: Unsigned packages will display security warnings during installation"
fi
echo "To install, use: sudo installer -pkg ${BUILD_DIR}/${PKG_NAME} -target /"