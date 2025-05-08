# Ferrum Streaming Control Technology™ - Host Implementation

This repository contains the official implementation of Ferrum Streaming Control Technology™ (FSCT) for controlling
FSCT-compatible audio devices. The primary language used is Rust, with bindings and platform-specific support for
Node.js.

## Features

- Core implementation of Ferrum Streaming Control Technology™ in Rust
- Node.js bindings for JavaScript integration
- Automatic platform-specific support for FSCT devices
- Comprehensive error handling and logging capabilities
- Cross-platform support for Linux architectures

## Repository Structure

- **core/**: Contains the Rust core implementation of FSCT, including decoding capabilities and device handling.
- **ports/**: Platform-specific modules and API bindings.
- **script/**: Utility scripts for building, testing, and maintaining the project.
- **Cargo.toml**: Rust project configuration that defines dependencies and build instructions.
- **LICENSE** and **LICENSE-FSCT.md**: Licensing details for the Ferrum Streaming Control Technology™ and related
  components.

## Installation

### Rust Library

To include this library in your Rust project, add it to your `Cargo.toml`:

```toml
[dependencies]
fsct_core = { git = "[https://github.com/HEM-RnD/fsct-host.git](https://github.com/HEM-RnD/fsct-host.git)", branch = "main" }
```

### Node.js Bindings

For Node.js users, the bindings are available as npm package. Install using:

```bash 
npm install @hemspzoo/fsct-lib
```

## Building the Project

Make sure you have Rust installed. To build the Rust library, run:

```bash 
cargo build --release
```

## Contributing

We welcome contributions! Please follow the guidelines:

1. Fork the repository and create a new branch.
2. Ensure all changes include appropriate tests.
3. Open a pull request, describing your changes in detail.

## License

This project is licensed under the Apache License, Version 2.0. You can find the license terms in the `LICENSE` file.

Additionally, this software implements FSCT, which is subject to the Ferrum Streaming Control Technology™ License,
Version 1.0. Please refer to the `LICENSE-FSCT.md` for details.

## Disclaimer

This software is provided "as is," without warranties or guarantees of any kind, either express or implied. Please see
the `LICENSE` file for more details.

---
HEM Sp. z o.o. © 2025. All rights reserved.
