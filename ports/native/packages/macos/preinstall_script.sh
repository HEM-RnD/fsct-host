#!/bin/bash

echo "Preinstall started" >> /tmp/preinstall.log
logger "Preinstall started"


# Kill any running instances of the application
echo "Killing any running instances of fsct_service" >> /tmp/preinstall.log
logger "Killing any running instances of fsct_service"
pkill -SIGINT -f "/usr/local/bin/fsct_service" 2>/tmp/preinstall.log || true

# Stop the daemon service if it's running
if [ -f "/Library/LaunchDaemons/com.hem-e.fsctservice.plist" ]; then
    echo "Stopping existing fsct service..." >> /tmp/preinstall.log
    logger "Stopping existing fsct service"
    launchctl stop /Library/LaunchDaemons/com.hem-e.fsctservice.plist 2>/tmp/preinstall.log || true
fi

echo "Preinstall finished" >> /tmp/preinstall.log
logger "Preinstall finished"

exit 0
