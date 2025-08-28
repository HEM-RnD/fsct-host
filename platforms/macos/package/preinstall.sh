#!/bin/bash

# Configuration
LOG_FILE="/tmp/fsct_installer.log"

rm -f $LOG_FILE || true

# Logging function
log_message() {
    local message="[FSCT Driver Installer] $1"
    logger -s "$message" 2>> $LOG_FILE
}

# Initialize log file
log_message "Preinstall started"

# Function to remove a service
remove_service() {
    local service_name=$1
    local service_display_name=$2

    # Derive plist path by removing underscores from service name
    local plist_name=$(echo "${service_name}" | sed 's/\_//g')
    local plist_path="/Library/LaunchDaemons/com.hem-e.${plist_name}.plist"

    # Binary name is already with underscores
    local binary_path="/usr/local/bin/${service_name}"

    if [ -f "$plist_path" ] || [ -f "$binary_path" ]; then
        log_message "Detected $service_display_name, removing..."

        # Kill any running instances of the application
        log_message "Killing any running instances of $service_name"
        pkill -SIGINT -f "$binary_path" 2>> $LOG_FILE || true

        # Stop the daemon service if it's running
        if [ -f "$plist_path" ]; then
            log_message "Stopping existing $service_display_name..."
            launchctl stop "$plist_path" 2>> $LOG_FILE || true
            launchctl unload "$plist_path" 2>> $LOG_FILE || true
        fi

        # Remove service files
        if [ -f "$plist_path" ]; then
            rm -f "$plist_path"
        fi
        if [ -f "$binary_path" ]; then
            rm -f "$binary_path"
        fi
    fi
}

# Stop and remove old daemon services if running
remove_service "fsct_service" "prerelease fsct service"
remove_service "fsct_driver_service" "previous fsct driver service"

log_message "Preinstall finished"

exit 0
