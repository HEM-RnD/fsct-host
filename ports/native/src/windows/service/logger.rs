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

use std::path::PathBuf;
use log::debug;
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
};
use crate::windows::service::cli::LogLevel;
use crate::windows::service::runtime::get_current_session_id;

pub fn get_log_dir() -> anyhow::Result<PathBuf> {
    // Create a log directory in ProgramData
    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    let log_dir = PathBuf::from(program_data).join("FSCT");

    // Create the directory if it doesn't exist
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)?;
    }

    Ok(log_dir)
}

pub fn get_logger_pattern() -> PatternEncoder
{
    PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S%.3f)} - {l} - {m}\n")
}

pub fn build_logger_config(
    log_file: PathBuf, 
    log_level: LogLevel, 
    include_console: bool
) -> anyhow::Result<Config> {
    // Create a file appender
    let file_appender = FileAppender::builder()
        .encoder(Box::new(get_logger_pattern()))
        .build(log_file)?;

    // Get LevelFilter from LogLevel
    let level_filter = log_level.to_level_filter();

    // Build the logger configuration
    let mut config_builder = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)));

    let mut root_builder = Root::builder().appender("file");

    // Add console appender if requested
    if include_console {
        // Create a console appender
        let console_appender = log4rs::append::console::ConsoleAppender::builder()
            .encoder(Box::new(get_logger_pattern()))
            .build();

        config_builder = config_builder
            .appender(Appender::builder().build("console", Box::new(console_appender)));

        root_builder = root_builder.appender("console");
    }

    // Build and return the config
    Ok(config_builder.build(root_builder.build(level_filter))?)
}

pub fn init_logger_common(log_file_name: &str, log_level: LogLevel, include_console: bool) -> anyhow::Result<()> {
    let log_dir = get_log_dir()?;
    let log_file = log_dir.join(log_file_name);
    let config = build_logger_config(log_file, log_level, include_console)?;
    log4rs::init_config(config)?;
    debug!("Logger initialized with level: {}", log_level);
    Ok(())
}

pub fn init_service_logger(log_level: LogLevel) -> anyhow::Result<()> {
    let session_id = get_current_session_id();
    let log_file_name = session_id
        .map(|session_id| format!("fsct_service_session_{}.log", session_id))
        .unwrap_or_else(|| "fsct_service.log".to_string());

    init_logger_common(&log_file_name, log_level, false)
}

pub fn init_install_logger(verbose: bool, log_level: LogLevel) -> anyhow::Result<()> {
    init_logger_common("fsct_install.log", log_level, verbose)
}

pub fn init_standalone_logger(log_level: LogLevel) -> anyhow::Result<()> {
    init_logger_common("fsct_standalone.log", log_level, true)
}