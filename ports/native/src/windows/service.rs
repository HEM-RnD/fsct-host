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

use std::ffi::OsString;
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use std::process;
use windows_service::{
    service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    define_windows_service,
};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use anyhow::Result;
use log::{info, error, warn, debug, LevelFilter};
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use fsct_core::{run_service, run_devices_watch, run_player_watch, DevicesPlayerEventApplier, player::Player};
use crate::initialize_native_platform_player;
use clap::{Parser, Subcommand};
use windows_service::service::ServiceState as WinServiceState;

// Define service events
#[derive(Clone)]
enum ServiceEvent {
    Shutdown,
    SessionChange(windows_service::service::SessionChangeParam),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Service management commands
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
}

#[derive(Subcommand)]
enum ServiceCommands {
    /// Install the service
    Install {
        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Uninstall the service
    Uninstall {
        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Run as a service
    Run,
}

pub const SERVICE_NAME: &str = "FsctDriverService";
pub const SERVICE_DISPLAY_NAME: &str = "FSCT Driver Service";
pub const SERVICE_DESCRIPTION: &str = "Ferrum Streaming Control Technology Driver Service";

// Struct to hold the service state and abort handles
struct FsctServiceState {
    runtime: Runtime,
    device_watch_handle: Option<JoinHandle<()>>,
    player_watch_handle: Option<JoinHandle<()>>,
    assigned_session_id: Option<u32>,  // The session ID of the user who the service is assigned to
    platform_player: Option<Player>,
}

impl FsctServiceState {
    fn new() -> Result<Self> {
        Ok(Self {
            runtime: Runtime::new()?,
            device_watch_handle: None,
            player_watch_handle: None,
            assigned_session_id: None, // Will be set when service starts
            platform_player: None,
        })
    }

    fn stop_service(&mut self) {
        info!("Stopping service tasks");
        if let Some(handle) = self.device_watch_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.player_watch_handle.take() {
            handle.abort();
        }
        self.platform_player = None;
    }

    async fn start_service(&mut self) -> Result<()> {
        info!("Starting service tasks");
        if self.device_watch_handle.is_some() || self.player_watch_handle.is_some() {
            warn!("Service tasks are already running, stopping them first");
            self.stop_service();
        }

        debug!("Initializing native platform player");
        let platform_player = match initialize_native_platform_player().await {
            Ok(player) => player,
            Err(e) => {
                error!("Failed to initialize player: {}", e);
                return Err(e.into());
            }
        };
        self.platform_player = Some(platform_player.clone());

        // Create shared state for devices and player state
        let fsct_devices = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let player_state = Arc::new(Mutex::new(fsct_core::player::PlayerState::default()));

        // Set up player event listener
        let player_event_listener = DevicesPlayerEventApplier::new(fsct_devices.clone());

        // Start devices watch
        debug!("Starting devices watch");
        let device_watch_handle = run_devices_watch(fsct_devices.clone(), player_state.clone()).await?;
        self.device_watch_handle = Some(device_watch_handle);

        // Start player watch
        debug!("Starting player watch");
        let player_watch_handle = run_player_watch(platform_player, player_event_listener, player_state).await?;
        self.player_watch_handle = Some(player_watch_handle);

        info!("Service tasks started successfully");
        Ok(())
    }

    fn get_current_session_id(&self) -> Result<u32> {
        // Get the current active session ID using the Windows API
        let session_id = unsafe { WTSGetActiveConsoleSessionId() };
        if session_id == 0xFFFFFFFF {
            return Err(anyhow::anyhow!("Failed to get active console session ID"));
        }
        Ok(session_id)
    }
}

fn get_service_type() -> ServiceType
{ ServiceType::USER_OWN_PROCESS | ServiceType::INTERACTIVE_PROCESS }

pub fn install_service() -> Result<()> {
    debug!("Starting service installation");

    debug!("Connecting to service manager");
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = match ServiceManager::local_computer(None::<&str>, manager_access) {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to connect to service manager: {}", e);
            return Err(e.into());
        }
    };

    // Get the current executable path
    debug!("Getting current executable path");
    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            error!("Failed to get current executable path: {}", e);
            return Err(e.into());
        }
    };

    let service_binary_path = match current_exe.to_str() {
        Some(path) => path,
        None => {
            let err = anyhow::anyhow!("Invalid path");
            error!("Invalid executable path: {}", err);
            return Err(err);
        }
    };

    debug!("Service binary path: {}", service_binary_path);

    // Create the service info
    debug!("Creating service info");
    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: get_service_type(),
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: PathBuf::from(service_binary_path),
        launch_arguments: vec![OsString::from("service"), OsString::from("run")],
        dependencies: vec![],
        account_name: None, // Run as LocalSystem
        account_password: None,
    };

    // Create the service
    debug!("Creating service");
    let service = match service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG) {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to create service: {}", e);
            return Err(e.into());
        }
    };

    // Set the service description
    debug!("Setting service description");
    if let Err(e) = service.set_description(SERVICE_DESCRIPTION) {
        error!("Failed to set service description: {}", e);
        return Err(e.into());
    }

    info!("Service installed successfully");
    println!("Service installed successfully");
    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    debug!("Starting service uninstallation");

    debug!("Connecting to service manager");
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = match ServiceManager::local_computer(None::<&str>, manager_access) {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to connect to service manager: {}", e);
            return Err(e.into());
        }
    };

    debug!("Opening service: {}", SERVICE_NAME);
    let service_access = ServiceAccess::DELETE;
    let service = match service_manager.open_service(SERVICE_NAME, service_access) {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to open service: {}", e);
            return Err(e.into());
        }
    };

    // Delete the service
    debug!("Deleting service");
    if let Err(e) = service.delete() {
        error!("Failed to delete service: {}", e);
        return Err(e.into());
    }
    info!("Service uninstalled successfully");
    println!("Service uninstalled successfully");
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

fn get_log_dir() -> anyhow::Result<PathBuf> {
    // Create a log directory in ProgramData
    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    let log_dir = PathBuf::from(program_data).join("FSCT");

    // Create the directory if it doesn't exist
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)?;
    }

    Ok(log_dir)
}

fn init_logger() -> anyhow::Result<()> {
    let log_dir = get_log_dir()?;

    // Get the current session ID
    let session_id = unsafe { WTSGetActiveConsoleSessionId() };
    let log_file = if session_id != 0xFFFFFFFF {
        // Include session ID in the log file name if running as a service
        log_dir.join(format!("fsct_service_session_{}.log", session_id))
    } else {
        // Fallback to the default log file name if unable to get session ID
        log_dir.join("fsct_service.log")
    };

    // Create a file appender
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}\n")))
        .build(log_file)?;

    // Build the logger configuration
    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .build(Root::builder().appender("file").build(LevelFilter::Debug))?;

    // Initialize the logger
    log4rs::init_config(config)?;

    debug!("Logger initialized");
    Ok(())
}

fn init_install_logger(verbose: bool) -> anyhow::Result<()> {
    let log_dir = get_log_dir()?;
    let log_file = log_dir.join("fsct_install.log");

    // Create a file appender
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}\n")))
        .build(log_file)?;

    // Build the logger configuration
    let mut config_builder = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)));

    let mut root_builder = Root::builder().appender("file");

    // Add console appender only if verbose is true
    if verbose {
        // Create a console appender
        let console_appender = log4rs::append::console::ConsoleAppender::builder()
            .encoder(Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}\n")))
            .build();

        config_builder = config_builder
            .appender(Appender::builder().build("console", Box::new(console_appender)));

        root_builder = root_builder.appender("console");
    }

    let config = config_builder.build(root_builder.build(LevelFilter::Debug))?;

    // Initialize the logger
    log4rs::init_config(config)?;

    debug!("Install logger initialized");
    Ok(())
}

fn init_standalone_logger() -> anyhow::Result<()> {
    let log_dir = get_log_dir()?;
    let log_file = log_dir.join("fsct_standalone.log");

    // Create a file appender
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}\n")))
        .build(log_file)?;

    // Create a console appender
    let console_appender = log4rs::append::console::ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}\n")))
        .build();

    // Build the logger configuration with both file and console appenders
    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .appender(Appender::builder().build("console", Box::new(console_appender)))
        .build(Root::builder()
            .appender("file")
            .appender("console")
            .build(LevelFilter::Debug))?;

    // Initialize the logger
    log4rs::init_config(config)?;

    debug!("Standalone logger initialized");
    Ok(())
}

// Function to run the service in standalone mode (for debugging)
fn run_standalone() -> anyhow::Result<()> {
    // Initialize logger for standalone mode
    if let Err(e) = init_standalone_logger() {
        eprintln!("Failed to initialize logger: {}", e);
    }

    debug!("Starting in standalone mode");

    // Create a Tokio runtime for async operations
    debug!("Creating Tokio runtime");
    let rt = Runtime::new()?;

    // Run the service in the Tokio runtime
    rt.block_on(async {
        debug!("Initializing native platform player");
        let platform_global_player = match initialize_native_platform_player().await {
            Ok(player) => player,
            Err(e) => {
                error!("Failed to initialize player: {}", e);
                return;
            }
        };

        // Start the service
        debug!("Starting service");
        if let Err(e) = run_service(platform_global_player).await {
            error!("Service error: {}", e);
        }

        // Wait for Ctrl+C
        debug!("Press Ctrl+C to exit");
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C signal");
        info!("Received Ctrl+C signal, exiting...");
    });

    debug!("Standalone mode exited");
    Ok(())
}

pub fn fsct_main() -> anyhow::Result<()> {
    // Parse command line arguments using clap
    let cli = Cli::parse();

    // Check if a command was provided
    if let Some(command) = cli.command {
        match command {
            Commands::Service { command } => {
                match command {
                    ServiceCommands::Install { verbose } => {
                        // Initialize logger for install command
                        if let Err(e) = init_install_logger(verbose) {
                            eprintln!("Failed to initialize logger: {}", e);
                        }
                        debug!("Installing service");
                        let result = install_service();
                        if let Err(ref e) = result {
                            error!("Failed to install service: {}", e);
                        } else {
                            info!("Service installed successfully");
                        }
                        return result;
                    }
                    ServiceCommands::Uninstall { verbose } => {
                        // Initialize logger for uninstall command
                        if let Err(e) = init_install_logger(verbose) {
                            eprintln!("Failed to initialize logger: {}", e);
                        }
                        debug!("Uninstalling service");
                        let result = uninstall_service();
                        if let Err(ref e) = result {
                            error!("Failed to uninstall service: {}", e);
                        } else {
                            info!("Service uninstalled successfully");
                        }
                        return result;
                    }
                    ServiceCommands::Run => {
                        // Run as a service
                        service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
                        return Ok(());
                    }
                }
            }
        }
    }

    // If no arguments provided, run in standalone mode
    run_standalone()
}

fn service_main(arguments: Vec<OsString>) {
    // Initialize the logger first thing
    if let Err(e) = init_logger() {
        // Can't log this error since the logger failed to initialize
        eprintln!("Failed to initialize logger: {}", e);
        return;
    }

    info!("Service starting");

    if let Err(e) = run_service_main(arguments) {
        error!("Service failed: {}", e);
    }
}

fn run_service_main(_arguments: Vec<OsString>) -> anyhow::Result<()> {
    // Create a Tokio runtime for async operations
    debug!("Creating Tokio runtime");
    let rt = Runtime::new()?;

    // Create a broadcast channel for events that can be used from both sync and async contexts
    let (event_tx, _) = tokio::sync::broadcast::channel::<ServiceEvent>(10);

    // Clone the sender for use in the service control handler
    let event_tx_clone = event_tx.clone();

    // Register the service control handler
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                // Send shutdown event
                debug!("Received stop control event");
                let _ = event_tx_clone.send(ServiceEvent::Shutdown);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::SessionChange(param) => {
                debug!("Received session change event: {:?}, session ID: {}", param.reason, param.notification.session_id);
                let _ = event_tx_clone.send(ServiceEvent::SessionChange(param));
                ServiceControlHandlerResult::NoError
            }
            _ => {
                debug!("Received unsupported control event: {:?}", control_event);
                ServiceControlHandlerResult::NotImplemented
            }
        }
    };

    debug!("Registering service control handler");
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    // Tell the system that the service is running
    debug!("Setting service status to Running");
    status_handle.set_service_status(ServiceStatus {
        service_type: get_service_type(),
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SESSION_CHANGE,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Run the service in the Tokio runtime
    rt.block_on(async {
        // Create a service state to manage the service tasks
        let mut service_state = match FsctServiceState::new() {
            Ok(state) => state,
            Err(e) => {
                error!("Failed to create service state: {}", e);
                return;
            }
        };

        // Get the current active console session ID
        // This is the session ID of the user who is currently logged on to the physical console
        let current_session_id = service_state.get_current_session_id().ok();
        service_state.assigned_session_id = current_session_id;  // Store the assigned session ID
        info!("Assigned session ID: {:?}", current_session_id);

        // Note: The assigned session ID is the session ID of the user who is currently logged on to the physical console
        // when the service starts. This is the session that the service is assigned to and should run for.
        // We only start service tasks for this session and stop them for all other sessions.

        // Start the service tasks
        if let Err(e) = service_state.start_service().await {
            error!("Failed to start service tasks: {}", e);
            return;
        }

        // Create a receiver for the broadcast channel
        let mut event_rx = event_tx.subscribe();

        // Also listen for Ctrl+C
        let event_tx_ctrl_c = event_tx.clone();
        tokio::spawn(async move {
            if let Ok(_) = tokio::signal::ctrl_c().await {
                debug!("Received Ctrl+C signal");
                let _ = event_tx_ctrl_c.send(ServiceEvent::Shutdown);
            }
        });

        // Wait for events
        debug!("Waiting for service events");
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    match event {
                        ServiceEvent::Shutdown => {
                            info!("Received shutdown event, stopping...");
                            break;
                        },
                        ServiceEvent::SessionChange(param) => {
                            let session_id = param.notification.session_id;
                            debug!("Processing session change event: {:?}, session ID: {}", param.reason, session_id);

                            // Handle session change based on both the reason and session ID
                            // We only care about events for the session assigned to this process (assigned_session_id)

                            // First, check if this event is for our assigned session
                            if service_state.assigned_session_id != Some(session_id) {
                                debug!("Event for session {} doesn't match assigned session {:?}, ignoring",
                                      session_id, service_state.assigned_session_id);
                                continue;
                            }

                            // Now handle events for our assigned session
                            match param.reason {
                                // For console connect, remote connect, and session logon events
                                // These events indicate our session is becoming active
                                windows_service::service::SessionChangeReason::ConsoleConnect |
                                windows_service::service::SessionChangeReason::RemoteConnect |
                                windows_service::service::SessionChangeReason::SessionLogon => {
                                    if !service_state.device_watch_handle.is_some() {
                                        info!("This session ({}) is becoming active, starting service tasks", session_id);
                                        if let Err(e) = service_state.start_service().await {
                                            error!("Failed to start service tasks: {}", e);
                                        }
                                    } else {
                                        info!("This session ({}) is becoming active, but service has been already
                                        started, ignoring...", session_id);
                                    }
                                },
                                // For session logoff events, we need to stop our service
                                windows_service::service::SessionChangeReason::SessionLogoff => {
                                    if service_state.device_watch_handle.is_some() {
                                        info!("This session ({}) is logging off, stopping service tasks", session_id);
                                        service_state.stop_service();
                                    } else {
                                        debug!("This session ({}) is logging off, but service is not started, can't \
                                        stop it, ignoring...", session_id)
                                    }
                                },
                                // For console disconnect events, we should stop our service
                                windows_service::service::SessionChangeReason::ConsoleDisconnect |
                                windows_service::service::SessionChangeReason::RemoteDisconnect => {
                                    if service_state.device_watch_handle.is_some() {
                                        info!("This session ({}) is disconnecting, stopping service tasks", session_id);
                                        service_state.stop_service();
                                    } else {
                                        debug!("This session ({}) is disconnecting, but service is not started, can't \
                                        stop it, ignoring...",
                                            session_id)
                                    }
                                },
                                // For other events, just log and continue
                                _ => {
                                    debug!("Received event {:?} for this session ({}), no action needed", param.reason,
                                        session_id);
                                    continue;
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to receive event: {}", e);
                }
            }
        }

        // Stop the service tasks
        debug!("Stopping service tasks");
        service_state.stop_service();

        info!("Exiting service");
    });

    // Tell the system that the service has stopped
    debug!("Setting service status to Stopped");
    status_handle.set_service_status(ServiceStatus {
        service_type: get_service_type(),
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    info!("Service stopped successfully");
    Ok(())
}
