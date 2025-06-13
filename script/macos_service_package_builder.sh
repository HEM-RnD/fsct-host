#!/bin/bash

# Get script directory and project root
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"

# Configuration variables
CARGO_BIN_NAME="fsct_driver_service"                      # Name of the binary target for Cargo
APP_NAME="fsct_driver_service"                                # Application name (final binary name)
IDENTIFIER="com.hem-e.fsctdriverservice"                     # Unique package identifier
BUILD_DIR="${ROOT_DIR}/target"                          # Directory where build will be performed
INSTALL_DIR="/usr/local/bin"                           # Target install directory for the binary
DAEMON_DIR="/Library/LaunchDaemons"                    # Target install directory for the plist
INSTALLER_FILES_DIR="${ROOT_DIR}/ports/native/packages/macos"  # Directory with prepared files (plist, postinstall, distribution.xml)

# Code signing certificate (ensure the certificate is installed in your Keychain)
DEVELOPER_ID_APP="Developer ID Application: HEM Sp. z o.o. (342MS6WA5D)"
DEVELOPER_ID_INSTALLER="Developer ID Installer: HEM Sp. z o.o. (342MS6WA5D)"

KEYCHAIN_PROFILE="APPLE_NOTARY_PROFILE"

# Control flags
SKIP_SIGNING=false
SKIP_NOTARIZATION=false
SKIP_LICENSE=false

# Parse command line arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --skip-signing) SKIP_SIGNING=true ;;
        --skip-notarization) SKIP_NOTARIZATION=true ;;
        --skip-license) SKIP_LICENSE=true ;;
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

# Function to check if a command exists
check_command() {
    if ! command -v "$1" &> /dev/null; then
        echo "Error: $1 is not installed or not in PATH"
        echo "Please install $1 to continue"
        exit 1
    fi
}

# Check for required tools
echo "Checking required tools..."
check_command cargo
check_command python3
check_command pkgbuild
check_command productbuild
check_command codesign
check_command xcrun
check_command ditto
check_command pandoc
check_command lipo

echo "Using cargo: $(cargo --version)"
echo "Using python3: $(python3 --version)"
echo "Using pandoc: $(pandoc --version | head -n 1)"

# Check if cargo about is installed (only if license generation is enabled)
if [ "$SKIP_LICENSE" = false ]; then
    if ! cargo about -V &> /dev/null; then
        echo "Error: cargo about is not installed"
        echo "Please install it with: cargo install cargo-about"
        exit 1
    fi
    echo "Using cargo about: $(cargo about -V)"
fi

# Extract version using cargo
VERSION=$(cd "${ROOT_DIR}" && cargo metadata --format-version 1 --no-deps | python3 -c "import sys, json; data = json.load(sys.stdin); print(next((p['version'] for p in data['packages'] if p['name'] == '${CARGO_BIN_NAME}'), ''))")
if [ -z "$VERSION" ]; then
    echo "Error: Failed to extract version using cargo metadata"
    exit 1
fi
echo "Using version from cargo metadata: ${VERSION}"

echo "Checking required Rust targets for universal build..."

PLATFORM_TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin")
for target in "${PLATFORM_TARGETS[@]}"; do
    if ! rustup target list --installed | grep -q "$target"; then
        echo "Rust target $target is not installed."
        echo "Install it with: rustup target add $target"
        exit 1
    fi
done
echo "All required targets are installed."

echo "========================================"
echo "Compiling universal (fat) binary..."

FAT_BUILD_DIR="${BUILD_DIR}/universal/release"
mkdir -p "${FAT_BUILD_DIR}"
# Loop through required targets and build
targets_array=()
for target in "${PLATFORM_TARGETS[@]}"; do
    (cd "${ROOT_DIR}" && cargo build --release --bin ${CARGO_BIN_NAME} --target ${target})
    targets_array+=("${ROOT_DIR}/target/${target}/release/${CARGO_BIN_NAME}")  
done

# Combine using lipo
lipo -create "${targets_array[@]}" -output "${FAT_BUILD_DIR}/${CARGO_BIN_NAME}"

echo "Fat binary created at ${FAT_BUILD_DIR}/${CARGO_BIN_NAME}"

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

mkdir -p "${BIN_ROOT}/usr/local/share/fsct-driver"
cp "${ROOT_DIR}/LICENSE-FSCT.md" "${BIN_ROOT}/usr/local/share/fsct-driver/"

# Copy the built executable and rename it to APP_NAME
echo "Copying the binary..."
cp "${FAT_BUILD_DIR}/${CARGO_BIN_NAME}" "${BIN_ROOT}${INSTALL_DIR}/${APP_NAME}"

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
if [ ! -f "${INSTALLER_FILES_DIR}/$IDENTIFIER.xml" ]; then
    echo "File $IDENTIFIER.xml not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Check for postinstall script
if [ ! -f "${INSTALLER_FILES_DIR}/postinstall.sh" ]; then
    echo "File postinstall.sh not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Check for preinstall script
if [ ! -f "${INSTALLER_FILES_DIR}/preinstall.sh" ]; then
    echo "File preinstall.sh not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Check for distribution.xml file
if [ ! -f "${INSTALLER_FILES_DIR}/distribution.xml" ]; then
    echo "File distribution.xml not found in ${INSTALLER_FILES_DIR}!"
    exit 1
fi

# Copy the prepared files
echo "Copying prepared files..."
cp "${INSTALLER_FILES_DIR}/$IDENTIFIER.xml" "${DAEMON_ROOT}${DAEMON_DIR}/$IDENTIFIER.plist"
cp "${INSTALLER_FILES_DIR}/postinstall.sh" "${SCRIPTS_DIR}/postinstall"
chmod +x "${SCRIPTS_DIR}/postinstall"

echo "========================================"
echo "Building component packages..."

# Prepare scripts for the daemon component
mkdir -p "${PACKAGE_DIR}/daemon_scripts"
cp "${INSTALLER_FILES_DIR}/postinstall.sh" "${PACKAGE_DIR}/daemon_scripts/postinstall"
chmod +x "${PACKAGE_DIR}/daemon_scripts/postinstall"

# Prepare scripts for the bin component
mkdir -p "${PACKAGE_DIR}/bin_scripts"
cp "${INSTALLER_FILES_DIR}/preinstall.sh" "${PACKAGE_DIR}/bin_scripts/preinstall"
chmod +x "${PACKAGE_DIR}/bin_scripts/preinstall"


echo "========================================"
echo "Generating EULA from FSCT_Driver_EULA.md..."
# Generate EULA.rtf from FSCT_Driver_EULA.md using pandoc
pandoc --from markdown --to rtf -s -o "${PACKAGE_DIR}/EULA.rtf" "${ROOT_DIR}/ports/native/FSCT_Driver_EULA.md"
if [ $? -ne 0 ]; then
    echo "Error: Failed to generate EULA.rtf using pandoc"
    exit 1
fi

# Verify that EULA.rtf was created
if [ ! -f "${PACKAGE_DIR}/EULA.rtf" ]; then
    echo "Error: EULA.rtf file was not created"
    exit 1
fi
echo "EULA generated successfully: ${PACKAGE_DIR}/EULA.rtf"

# Copy the EULA.md to the binary root directory
cp "${ROOT_DIR}/ports/native/FSCT_Driver_EULA.md" "${BIN_ROOT}/usr/local/share/fsct-driver/EULA.md"

echo "========================================"
echo "Generating license information..."
if [ "$SKIP_LICENSE" = false ]; then
    echo "Generating LICENSES.md using cargo about..."
    (cd "${ROOT_DIR}" && cargo about generate -c about.toml -m ports/native/Cargo.toml licenses.hbs -o "${PACKAGE_DIR}/LICENSES.md")
    if [ $? -ne 0 ]; then
        echo "Error: Failed to generate LICENSES.md using cargo about"
        exit 1
    fi
    # Copy the generated LICENSES.md to the binary root directory
    cp "${PACKAGE_DIR}/LICENSES.md" "${BIN_ROOT}/usr/local/share/fsct-driver/"
    echo "LICENSES.md generated successfully: ${PACKAGE_DIR}/LICENSES.md"
else
    echo "Skipping LICENSES.md generation (--skip-license option provided)"
    # Create a minimal LICENSES.md file
    cat > "${PACKAGE_DIR}/LICENSES.md" << EOF
# Third Party Licenses

This file contains license information for the dependencies used in this project.
License generation was skipped during build.

For complete license information, please build without the --skip-license option.
EOF
    # Copy the minimal LICENSES.md to the binary root directory
    cp "${PACKAGE_DIR}/LICENSES.md" "${BIN_ROOT}/usr/local/share/fsct-driver/"
fi

echo "========================================"
echo "Copying distribution.xml to package build directory..."
cp "${INSTALLER_FILES_DIR}/distribution.xml" "${PACKAGE_DIR}/distribution.xml"
if [ $? -ne 0 ]; then
    echo "Error: Failed to copy distribution.xml to package build directory"
    exit 1
fi

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
PKG_NAME="${APP_NAME}-${VERSION}.pkg"                  # Output package name

# Build final package with or without signing
if [ "$SKIP_SIGNING" = false ]; then
    productbuild --distribution "${PACKAGE_DIR}/distribution.xml" \
                 --package-path "${COMPONENT_PKGS_DIR}" \
                 --sign "${DEVELOPER_ID_INSTALLER}" \
                 --resources "${PACKAGE_DIR}" \
                 "${PACKAGE_DIR}/${PKG_NAME}"

    echo "Verifying package signature..."
    pkgutil --check-signature "${PACKAGE_DIR}/${PKG_NAME}"
else
    productbuild --distribution "${PACKAGE_DIR}/distribution.xml" \
                 --package-path "${COMPONENT_PKGS_DIR}" \
                 --resources "${PACKAGE_DIR}" \
                 "${PACKAGE_DIR}/${PKG_NAME}"
fi

# Notarize the package if enabled
if [ "$SKIP_NOTARIZATION" = false ] && [ "$SKIP_SIGNING" = false ]; then
    echo "========================================"
    echo "Submitting package for notarization..."

    # Create a temporary ZIP archive for notarization
    NOTARIZE_ZIP="${BUILD_DIR}/${APP_NAME}-${VERSION}.zip"
    ditto -c -k --keepParent "${PACKAGE_DIR}/${PKG_NAME}" "${NOTARIZE_ZIP}"

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
    xcrun stapler staple "${PACKAGE_DIR}/${PKG_NAME}"

    # Verify stapling
    xcrun stapler validate "${PACKAGE_DIR}/${PKG_NAME}"

    # Clean up
    rm "${NOTARIZE_ZIP}"
fi

echo "========================================"
echo "Done! Package created: ${PACKAGE_DIR}/${PKG_NAME}"
if [ "$SKIP_SIGNING" = false ] && [ "$SKIP_NOTARIZATION" = false ]; then
    echo "Package is signed and notarized"
elif [ "$SKIP_SIGNING" = false ]; then
    echo "Package is signed but not notarized"
else
    echo "Package is unsigned"
    echo "WARNING: Unsigned packages will display security warnings during installation"
fi
echo "To install, use: sudo installer -pkg ${PACKAGE_DIR}/${PKG_NAME} -target LocalSystem"
