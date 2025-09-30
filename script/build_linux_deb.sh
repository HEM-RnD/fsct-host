#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$( dirname "${SCRIPT_DIR}" )"
PACKAGE_NAME="fsct-driver"
CARGO_BIN_NAME="fsctd"
BUILD_ROOT="${ROOT_DIR}/target/deb/build"
STAGE_DIR="${BUILD_ROOT}/stage"
OUTPUT_DIR="${ROOT_DIR}/target/deb"
# FPM config used by fpm (must be under debian structure, no .conf)
PACKAGE_SOURCE="${ROOT_DIR}/packages/linux/"
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
# Rust toolchain is only required if we are going to build
if [[ "$SKIP_BUILD" != true ]]; then
  need rustc || missing+=("rustc (via rustup preferred)")
  need cargo || missing+=("cargo (via rustup preferred)")
fi
# python3 is optional; we'll fallback to parsing Cargo.toml if unavailable

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
else
  echo "Building Rust binaries (release=${DEBUG_BUILD=false})..."
  if [[ "$DEBUG_BUILD" == true ]]; then
    if [[ -n "${FSCT_RUST_TARGET:-}" ]]; then
      cargo build --target "${FSCT_RUST_TARGET}"
    else
      cargo build
    fi
  else
    if [[ -n "${FSCT_RUST_TARGET:-}" ]]; then
      cargo build --release --target "${FSCT_RUST_TARGET}"
    else
      cargo build --release
    fi
  fi
fi

# Resolve BIN_PATH based on build or existing artifacts (robust search)
resolve_bin() {
  local candidates=()
  if [[ -n "${FSCT_RUST_TARGET:-}" ]]; then
    if [[ "$DEBUG_BUILD" == true ]]; then
      candidates+=("${ROOT_DIR}/target/${FSCT_RUST_TARGET}/debug/${CARGO_BIN_NAME}")
      candidates+=("${ROOT_DIR}/target/${FSCT_RUST_TARGET}/release/${CARGO_BIN_NAME}")
    else
      candidates+=("${ROOT_DIR}/target/${FSCT_RUST_TARGET}/release/${CARGO_BIN_NAME}")
      candidates+=("${ROOT_DIR}/target/${FSCT_RUST_TARGET}/debug/${CARGO_BIN_NAME}")
    fi
  fi
  if [[ "$DEBUG_BUILD" == true ]]; then
    candidates+=("${ROOT_DIR}/target/debug/${CARGO_BIN_NAME}")
    candidates+=("${ROOT_DIR}/target/release/${CARGO_BIN_NAME}")
  else
    candidates+=("${ROOT_DIR}/target/release/${CARGO_BIN_NAME}")
    candidates+=("${ROOT_DIR}/target/debug/${CARGO_BIN_NAME}")
  fi
  for c in "${candidates[@]}"; do
    if [[ -f "$c" ]]; then
      echo "$c"
      return 0
    fi
  done
  return 1
}

if BIN_PATH="$(resolve_bin)"; then
  :
else
  echo "Error: built binary not found in expected locations." >&2
  echo "Tried (in order):" >&2
  echo "  - target/{${FSCT_RUST_TARGET:-<none>}}/{release,debug}/${CARGO_BIN_NAME}" >&2
  echo "  - target/{release,debug}/${CARGO_BIN_NAME}" >&2
  exit 1
fi

# Extract version
if [[ -n "${FSCT_VERSION:-}" ]]; then
  VERSION="${FSCT_VERSION}"
else
  if command -v cargo >/dev/null 2>&1; then
    VERSION=$(cd "${ROOT_DIR}" && cargo metadata --format-version 1 --no-deps | python3 -c "import sys,json; data=json.load(sys.stdin); print(next((p['version'] for p in data['packages'] if p['name']=='${PACKAGE_NAME}'),''))")
  else
    # Fallback: parse from workspace Cargo.toml [workspace.package]
    VERSION=$(grep -E '^version\s*=\s*"[0-9]+\.[0-9]+\.[0-9]+"' "${ROOT_DIR}/Cargo.toml" | head -n1 | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)"/\1/')
  fi
fi
if [[ -z "${VERSION}" ]]; then
  echo "Error: Failed to determine version" >&2
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
  (cd "${ROOT_DIR}" && cargo about generate -c about.toml -m driver/Cargo.toml licenses.hbs -o "${DOC_DIR}/LICENSES.md") || {
    echo "Warning: cargo about failed; continuing without LICENSES.md" >&2
  }
fi

# Determine Debian architecture for output filename
if [[ -n "${FSCT_DEB_ARCH:-}" ]]; then
  ARCH="${FSCT_DEB_ARCH}"
else
  ARCH=$(dpkg --print-architecture 2>/dev/null || uname -m)
fi
PACKAGE_FILE="${OUTPUT_DIR}/${PACKAGE_NAME}_${VERSION}_${ARCH}.deb"
rm -f "${PACKAGE_FILE}" || true

# Detect glibc (libc6) version required by the built binary (cross-safe)
LIBC_DEP=""
libc_ver=""
if command -v readelf >/dev/null 2>&1; then
  libc_ver=$(readelf -V "${BIN_PATH}" 2>/dev/null | grep -o 'GLIBC_[0-9]\+\.[0-9]\\+\(\.[0-9]\+\)\?' | sort -uV | tail -1 | sed 's/^GLIBC_//') || true
fi
# Fallback: try objdump if readelf not present or produced nothing
if [[ -z "${libc_ver:-}" ]] && command -v objdump >/dev/null 2>&1; then
  libc_ver=$(objdump -T "${BIN_PATH}" 2>/dev/null | grep -o 'GLIBC_[0-9]\+\.[0-9]\+\(\.[0-9]\+\)\?' | sort -uV | tail -1 | sed 's/^GLIBC_//') || true
fi
if [[ -n "${libc_ver:-}" ]]; then
  LIBC_DEP="libc6 (>= ${libc_ver})"
  echo "Detected required GLIBC from binary: ${libc_ver}; adding dependency: ${LIBC_DEP}"
else
  # If the binary is statically linked or tools unavailable, decide based on flag
  echo "Warning: Could not determine GLIBC version from binary (maybe static or tools missing)." >&2
  if [[ "${ALLOW_MISSING_DEPS}" == true ]]; then
    echo "Proceeding without explicit libc6 dependency due to --allow-missing-deps" >&2
  else
    echo "Error: GLIBC version detection failed. Install binutils (readelf) or use --allow-missing-deps." >&2
    exit 1
  fi
fi

FPM_ARGS=(
  -v "${VERSION}"
  --package "${PACKAGE_FILE}"
  --depends "${LIBC_DEP}"
  --depends libgcc-s1
  -a "${ARCH}"
  "${STAGE_DIR}/=/"
)

echo "Running fpm to create Debian package..."
cd "${CONFIG_DIR}" && fpm "${FPM_ARGS[@]}"

# Cleanup (optional)
if [[ "$KEEP_BUILD" != true ]]; then
  rm -rf "${BUILD_ROOT}"
fi

echo "Package(s) in: ${OUTPUT_DIR}"
