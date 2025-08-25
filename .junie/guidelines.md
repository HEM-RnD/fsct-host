# FSCT Host Development Guidelines

## Natural Language
All project documentation and comments are written in English

## Project Overview
This is a Rust workspace implementing Ferrum Streaming Control Technology™ (FSCT) for controlling FSCT-compatible audio devices. The project consists of three main components:
- **core**: Core Rust library with USB communication and device handling
- **ports/native**: Native platform-specific implementations
- **ports/node**: Node.js bindings for JavaScript integration

## Build/Configuration Instructions

### Prerequisites
- Rust toolchain (edition 2024)
- For cross-compilation: Docker and `cross` tool
- For Node.js bindings: Node.js and npm

### Standard Build
```bash
# Build all workspace members
cargo build --release

# Build specific package
cargo build --package fsct_core --release
cargo build --package fsct_native --release
cargo build --package fsct_node --release
```

### Cross-Compilation Setup
The project uses `cross` for cross-compilation with custom Docker configurations:

```bash
# Install cross if not already installed
cargo install cross

# Cross-compile for different architectures
cross build --target aarch64-unknown-linux-gnu --release
cross build --target armv7-unknown-linux-gnueabihf --release
cross build --target arm-unknown-linux-gnueabihf --release
cross build --target x86_64-unknown-linux-gnu --release
```

**Important**: Custom Docker files are located in `.cross/` directory for each target architecture.

### Platform-Specific Builds
The `script/` directory contains specialized build scripts:
- `build_node_lib_multitarget.sh`: Multi-target Node.js library builds
- `build_windows_installer.ps1`: Windows installer creation
- `macos_service_package_builder.sh`: macOS service packages

### Node.js Bindings
```bash
cd ports/node
npm install
npm run build
```

## Testing Information

### Running Rust Tests
```bash
# Run all tests in workspace
cargo test

# Run tests for specific package
cargo test --package fsct_core

# Run specific test module
cargo test --package fsct_core test_example

# Run tests with output
cargo test -- --nocapture
```

### Test Organization
- Unit tests are embedded in source files using `#[cfg(test)]` modules
- Main test coverage is in:
  - `core/src/usb/fsct_bos_finder.rs` (USB BOS descriptor parsing)
  - `core/src/usb/fsct_device.rs` (UTF-8/UTF-16 encoding)
- Example programs are in `core/examples/` directory

### Adding New Tests
1. Create test module in the relevant source file:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_your_functionality() {
        // Test implementation
        assert_eq!(expected, actual);
    }
}
```

2. For separate test files, add module declaration to `lib.rs`:
```rust
mod your_test_module;
```

### Node.js Testing
```bash
cd ports/node
npm test  # Runs AVA-based tests in __test__/ directory
```

### Example Test Execution
The project includes working unit tests. Example test run:
```bash
$ cargo test --package fsct_core
running 22 tests  # (20 existing + 2 example tests)
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Additional Development Information

### Code Style Configuration
- **Line width**: 120 characters (configured in `rustfmt.toml`)
- Use `cargo fmt` to format code according to project standards
- Follow Rust 2024 edition conventions

### Key Dependencies
- **nusb**: Custom fork for USB communication (`https://github.com/HEM-RnD/nusb.git`)
- **tokio**: Async runtime with "full" features
- **uuid**: UUID generation and handling
- **bitflags**: Bitfield operations
- **log**: Logging framework

### USB Communication
The project implements USB communication for FSCT devices:
- BOS (Binary Object Store) descriptor parsing
- Platform capability descriptors
- UTF-8/UTF-16 text encoding for device metadata

### Logging
- Uses `env_logger` for development
- Production logging configured per platform (systemd for Linux services)
- Initialize logging in applications:
```rust
env_logger::init();
```

### Cross-Platform Considerations
- Linux: Primary target with ARM support for embedded devices
- Windows: Service installation and driver management
- macOS: Service packaging and notarization support

### Debugging Tips
- Use `RUST_LOG=debug` environment variable for detailed logging
- USB communication can be debugged using the example programs in `core/examples/`
- For Node.js bindings, use the test module: `node ports/node/test_module.mjs`

### License Compliance
- Project uses Apache 2.0 license
- Additional FSCT technology licensing terms in `LICENSE-FSCT.md`
- Use `cargo about` for license compliance reporting (templates in `licenses.hbs`)

### Release Configuration
The workspace uses optimized release builds:
- LTO (Link Time Optimization) enabled
- Debug symbols stripped
- Profile configured in root `Cargo.toml`

---
*Generated on 2025-08-07 for FSCT Host v0.2.13*