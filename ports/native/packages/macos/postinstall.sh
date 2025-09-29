#!/bin/bash

# Configuration
LOG_FILE="/tmp/fsct_installer.log"
USER_AGENT_PLIST="/Library/LaunchAgents/com.hem-e.fsct-driver-user.plist"
USER_AGENT_LABEL="com.hem-e.fsctdriverservice.user"

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
log_message "Postinstall started"

# Ensure socket directory exists for launchd-created socket
mkdir -p /var/run/fsct
chmod 775 /var/run/fsct

# Ensure correct permissions on installed files
chmod 755 /usr/local/bin/fsctd || true
chmod 644 /Library/LaunchDaemons/com.hem-e.fsct-driver.plist || true
chmod 644 "$USER_AGENT_PLIST" || true

# Load system daemon
launchctl load -w /Library/LaunchDaemons/com.hem-e.fsct-driver.plist 2>> $LOG_FILE || true

# Bootstrap and start user agents for all active users
for uid in $(active_user_uids); do
    log_message "Bootstrapping user agent for UID $uid"
    launchctl bootstrap gui/$uid "$USER_AGENT_PLIST" 2>> $LOG_FILE || true
    launchctl enable gui/$uid/$USER_AGENT_LABEL 2>> $LOG_FILE || true
    launchctl kickstart -k gui/$uid/$USER_AGENT_LABEL 2>> $LOG_FILE || true
done

log_message "Postinstall finished"