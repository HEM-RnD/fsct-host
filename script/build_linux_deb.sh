#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"
PACKAGE_NAME="fsct-driver"
CARGO_BIN_NAME="fsct_driver_service"
BUILD_ROOT="${ROOT_DIR}/target/deb/build"
STAGE_DIR="${BUILD_ROOT}/stage"
OUTPUT_DIR="${ROOT_DIR}/target/deb"
# FPM config used by fpm (must be under debian structure, no .conf)
PACKAGE_SOURCE="${ROOT_DIR}/ports/native/packages/linux/"
CONFIG_DIR="${PACKAGE_SOURCE}/debian/"

# Flags
DEBUG_BUILD=false
SKIP_BUILD=false
SKIP_LICENSING=false
ALLOW_MISSING_DEPS=false
KEEP_BUILD=false
while [[ "$#" -gt 0 ]]; do
  case $1 in
    --debug) DEBUG_BUILD=true ;;
    --skip-build) SKIP_BUILD=true ;;
    --skip-licensing) SKIP_LICENSING=true ;;
    --allow-missing-deps) ALLOW_MISSING_DEPS=true;;
    --keep-build) KEEP_BUILD=true;;
    -h|--help)
      cat <<EOF
Usage: $(basename "$0") [options]
  --debug                 Build debug binary (default: release)
  --skip-build            Skip cargo build and use existing binary in target/
  --skip-licensing        Skip generating third-party licenses (cargo-about)
  --allow-missing-deps    Continue even if some tooling is missing (for CI tests)
  --keep-build            Keep staging/build directories (do not clean)

Note: The script auto-detects system glibc version and adds a package dependency: libc6 (>= <detected>).
  -h, --help              Show this help and exit
EOF
      exit 0
      ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
  shift
 done

# Preflight: dependencies (don't auto-install; instruct the user)
need() { command -v "$1" >/dev/null 2>&1; }
missing=()
need fpm || missing+=("fpm (gem install fpm)")
need ruby || missing+=("ruby")
need pkg-config || missing+=("pkg-config")
need rustc || missing+=("rustc (via rustup preferred)")
need cargo || missing+=("cargo (via rustup preferred)")
need python3 || missing+=("python3 (for cargo metadata parsing)")

# Optional: cargo-about for license aggregation (skip if --skip-licensing)
if [[ "$SKIP_LICENSING" == true ]]; then
  CARGO_ABOUT_AVAILABLE=false
else
  if ! command -v cargo >/dev/null 2>&1 || ! cargo about -V >/dev/null 2>&1; then
    echo "Note: cargo-about not found. LICENSES.md won't be generated." >&2
    CARGO_ABOUT_AVAILABLE=false
  else
    CARGO_ABOUT_AVAILABLE=true
  fi
fi

if (( ${#missing[@]} > 0 )); then
  echo "Error: missing required tools:" >&2
  for m in "${missing[@]}"; do echo "  - $m" >&2; done
  echo >&2
  echo "Install: sudo apt-get install -y ruby ruby-dev build-essential pkg-config && sudo gem install fpm" >&2
  echo "Install Rust toolchain via rustup (recommended): curl https://sh.rustup.rs -sSf | sh; source \"$HOME/.cargo/env\"" >&2
  if [[ "$ALLOW_MISSING_DEPS" != true ]]; then
    exit 1
  fi
  echo "Proceeding despite missing deps due to --allow-missing-deps" >&2
fi

# Clean staging
rm -rf "${BUILD_ROOT}"
mkdir -p "${STAGE_DIR}"
mkdir -p "${OUTPUT_DIR}"

# Build Rust binaries (unless skipped)
if [[ "$SKIP_BUILD" == true ]]; then
  echo "Skipping cargo build (per --skip-build)"
  # Prefer release binary by default; fall back to debug
  if [[ -f "${ROOT_DIR}/target/release/${CARGO_BIN_NAME}" ]]; then
    BIN_PATH="${ROOT_DIR}/target/release/${CARGO_BIN_NAME}"
  elif [[ -f "${ROOT_DIR}/target/debug/${CARGO_BIN_NAME}" ]]; then
    BIN_PATH="${ROOT_DIR}/target/debug/${CARGO_BIN_NAME}"
  else
    echo "Error: --skip-build set, but no existing binary found in target/release or target/debug" >&2
    exit 1
  fi
else
  echo "Building Rust binaries (release=${DEBUG_BUILD=false})..."
  if [[ "$DEBUG_BUILD" == true ]]; then
    cargo build
    BIN_PATH="${ROOT_DIR}/target/debug/${CARGO_BIN_NAME}"
  else
    cargo build --release
    BIN_PATH="${ROOT_DIR}/target/release/${CARGO_BIN_NAME}"
  fi
fi

if [[ ! -f "${BIN_PATH}" ]]; then
  echo "Error: built binary not found at ${BIN_PATH}" >&2
  exit 1
fi

# Extract version using cargo metadata (same as macOS script)
VERSION=$(cd "${ROOT_DIR}" && cargo metadata --format-version 1 --no-deps | python3 -c "import sys,json; data=json.load(sys.stdin); print(next((p['version'] for p in data['packages'] if p['name']=='${CARGO_BIN_NAME}'),''))")
if [[ -z "${VERSION}" ]]; then
  echo "Error: Failed to extract version using cargo metadata" >&2
  exit 1
fi
echo "Using version: ${VERSION}"

# Prepare staging tree
install -Dm0755 "${BIN_PATH}" "${STAGE_DIR}/usr/bin/fsctd"
# systemd units

# Licenses and notices
DOC_DIR="${STAGE_DIR}/usr/share/doc/${PACKAGE_NAME}"
install -Dm0644 "${ROOT_DIR}/LICENSE" "${DOC_DIR}/LICENSE"
install -Dm0644 "${ROOT_DIR}/LICENSE-FSCT.md" "${DOC_DIR}/LICENSE-FSCT.md"
install -Dm0644 "${ROOT_DIR}/NOTICE" "${DOC_DIR}/NOTICE"

# Generate third-party licenses like on macOS (cargo about)
if [[ "$SKIP_LICENSING" == true ]]; then
  echo "Skipping third-party license generation (per --skip-licensing)"
  cat > "${DOC_DIR}/LICENSES.md" <<'EOM'
# Third Party Licenses

License generation was skipped during this build (--skip-licensing).
For complete license information, build without --skip-licensing.
EOM
elif [[ "$CARGO_ABOUT_AVAILABLE" == true ]]; then
  (cd "${ROOT_DIR}" && cargo about generate -c about.toml -m ports/native/Cargo.toml licenses.hbs -o "${DOC_DIR}/LICENSES.md") || {
    echo "Warning: cargo about failed; continuing without LICENSES.md" >&2
  }
fi

# Determine Debian architecture for output filename
ARCH=$(dpkg --print-architecture 2>/dev/null || uname -m)
PACKAGE_FILE="${OUTPUT_DIR}/${PACKAGE_NAME}_${VERSION}_${ARCH}.deb"
rm -f "${PACKAGE_FILE}" || true

# Detect glibc (libc6) version and add dependency
LIBC_DEP=""
if command -v getconf >/dev/null 2>&1; then
  # Prefer getconf GNU_LIBC_VERSION (e.g., "glibc 2.31")
  glibc_line=$(getconf GNU_LIBC_VERSION 2>/dev/null || true)
  if [[ -n "$glibc_line" && "$glibc_line" == glibc* ]]; then
    libc_ver=${glibc_line#glibc }
  fi
fi
if [[ -z "${libc_ver:-}" ]]; then
  # Fallback: ldd --version first line contains "ldd (GNU libc) 2.xx"
  if command -v ldd >/dev/null 2>&1; then
    ldd_ver=$(ldd --version 2>/dev/null | head -n1 | sed -E 's/.* ([0-9]+\.[0-9]+(\.[0-9]+)?).*$/\1/')
    if [[ "$ldd_ver" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
      libc_ver=$ldd_ver
    fi
  fi
fi
if [[ -n "${libc_ver:-}" ]]; then
  # Debian/Ubuntu package providing glibc is libc6; add >= constraint
  LIBC_DEP="libc6 (>= ${libc_ver})"
  echo "Detected glibc version: ${libc_ver}; adding dependency: ${LIBC_DEP}"
else
  echo "Error: Could not detect glibc version. Skipping automatic libc6 dependency." >&2
  exit 1
fi

FPM_ARGS=(
  -v "${VERSION}"
  --package "${PACKAGE_FILE}"
  --depends "${LIBC_DEP}"
  --depends libgcc-s1
  "${STAGE_DIR}/=/"
)

echo "Running fpm to create Debian package..."
cd "${CONFIG_DIR}" && fpm "${FPM_ARGS[@]}"

# Cleanup (optional)
if [[ "$KEEP_BUILD" != true ]]; then
  rm -rf "${BUILD_ROOT}"
fi

echo "Package(s) in: ${OUTPUT_DIR}"
