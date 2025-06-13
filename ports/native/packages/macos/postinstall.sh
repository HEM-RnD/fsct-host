#!/bin/bash

# Configuration
LOG_FILE="/tmp/fsct_installer.log"

# Logging function
log_message() {
    local message="[FSCT Driver Installer] $1"
    logger -s "$message" 2>> $LOG_FILE
}

# Initialize log file
log_message "Postinstall started"

chmod 755 /usr/local/bin/fsctdriverservice
chmod 644 /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist

launchctl load -w /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist 2>> $LOG_FILE

log_message "Postinstall finished"