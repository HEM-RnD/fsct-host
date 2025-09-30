#!/bin/bash

# Configuration
LOG_FILE="/tmp/fsct_installer.log"
USER_AGENT_PLIST="/Library/LaunchAgents/com.hem-e.fsct-driver-user.plist"
USER_AGENT_LABEL="com.hem-e.fsct-driver-user"

rm -f $LOG_FILE || true

# Logging function
log_message() {
    local message="[FSCT Driver Installer] $1"
    logger -s "$message" 2>> $LOG_FILE
}

# Helper: iterate active GUI user UIDs (unique)
active_user_uids() {
    who | awk '{print $1}' | sort -u | while read -r user; do
        if [ -n "$user" ] && [ "$user" != "root" ]; then
            id -u "$user" 2>/dev/null || true
        fi
    done | sort -u
}

# Initialize log file
log_message "Preinstall started"

# Function to remove a service
remove_service() {
    local binary_name=$1
    local service_name=$2
    local service_display_name=$3

    # Use explicit names for plist and binary; do not transform underscores
    local plist_path="/Library/LaunchDaemons/com.hem-e.${service_name}.plist"
    local binary_path="/usr/local/bin/${binary_name}"

    if [ -f "$plist_path" ] || [ -f "$binary_path" ]; then
        log_message "Detected $service_display_name, removing..."

        # Kill any running instances of the application
        log_message "Killing any running instances of $binary_name"
        pkill -SIGINT -f "$binary_path" 2>> $LOG_FILE || true

        # Stop the daemon service if it's running
        if [ -f "$plist_path" ]; then
            log_message "Stopping existing $service_display_name..."
            launchctl bootout system/com.hem-e.${service_name} 2>> $LOG_FILE || true
            launchctl bootout system "$plist_path" 2>> $LOG_FILE || true
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

# Stop user LaunchAgents for all active users (to allow upgrade)
for uid in $(active_user_uids); do
    log_message "Booting out user agent for UID $uid"
    launchctl bootout gui/$uid/$USER_AGENT_LABEL 2>> $LOG_FILE || true
    # Also try by path for safety (older states)
    launchctl bootout gui/$uid "$USER_AGENT_PLIST" 2>> $LOG_FILE || true
done

# Stop and remove old daemon services if running
remove_service "fsct_service" "fsctservice" "prerelease fsct service"
remove_service "fsct_driver_service" "fsctdriverservice" "legacy fsct driver service"
remove_service "fsctd" "fsct-driver" "previous fsct driver service"

log_message "Preinstall finished"

exit 0
