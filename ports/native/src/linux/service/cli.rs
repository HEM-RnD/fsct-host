// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

use clap::Parser;

/// Linux service CLI
///
/// Behavior:
/// - default (no flags): standalone -> local driver + OS watcher (talks directly in-process)
/// - --driver: driver mode -> start local driver and expose it via IPC (no OS watcher)
/// - --user: user mode -> start OS watcher and connect to driver via IPC
#[derive(Parser, Debug)]
#[command(author, version, about = "FSCT Linux service", long_about = None)]
pub struct Cli {
    /// Run in driver mode (expose LocalDriver over IPC)
    #[arg(long, short, conflicts_with = "user")]
    pub driver: bool,

    /// Run in user mode (OS watcher talks to IPC driver)
    #[arg(long, short, conflicts_with = "driver")]
    pub user: bool,
}
