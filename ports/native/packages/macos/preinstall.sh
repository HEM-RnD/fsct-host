#!/bin/bash


logger -s "Preinstall started" 2> /tmp/preinstall.log

# Stop and remove old daemon services if running
if [ -f "/Library/LaunchDaemons/com.hem-e.fsctservice.plist" ] || [ -f "/usr/local/bin/fsct_service" ]; then
    logger -s "Detected prerelease fsct_service, removing..." 2>> /tmp/preinstall.log

    # Kill any running instances of the old application
    logger -s "Killing any running instances of fsct_service" 2>> /tmp/preinstall.log
    pkill -SIGINT -f "/usr/local/bin/fsct_service" 2>> /tmp/preinstall.log || true

    logger -s "Stopping existing prerelease fsct service..." 2>> /tmp/preinstall.log
    launchctl stop /Library/LaunchDaemons/com.hem-e.fsctservice.plist 2>> /tmp/preinstall.log || true
    launchctl unload /Library/LaunchDaemons/com.hem-e.fsctservice.plist 2>> /tmp/preinstall.log || true  

    if [ -f "/Library/LaunchDaemons/com.hem-e.fsctservice.plist" ]; then
        rm -f /Library/LaunchDaemons/com.hem-e.fsctservice.plist
    fi
    if [ -f "/usr/local/bin/fsct_service" ]; then
        rm -f /usr/local/bin/fsct_service
    fi
fi

if [ -f "/Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist" ] || [ -f "/usr/local/bin/fsct_driver_service" ]; then
    logger -s "Detected previous fsct_driver_service, removing..." 2>> /tmp/preinstall.log

    # Kill any running instances of the application
    logger -s "Killing any running instances of fsct_driver_service" 2>> /tmp/preinstall.log
    pkill -SIGINT -f "/usr/local/bin/fsct_driver_service" 2>> /tmp/preinstall.log || true

    # Stop the daemon service if it's running
    if [ -f "/Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist" ]; then
        logger -s "Stopping existing fsct driver service..." 2>> /tmp/preinstall.log
        launchctl stop /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist 2>> /tmp/preinstall.log || true
        launchctl unload /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist 2>> /tmp/preinstall.log || true
    fi

    if [ -f "/Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist" ]; then
        rm -f /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist
    fi
    if [ -f "/usr/local/bin/fsct_driver_service" ]; then
        rm -f /usr/local/bin/fsct_driver_service
    fi
fi

logger -s "Preinstall finished" 2>> /tmp/preinstall.log

exit 0
