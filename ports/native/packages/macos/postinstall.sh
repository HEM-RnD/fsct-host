#!/bin/bash

logger -s "Postinstall started" 2> /tmp/postinstall.log

chmod 755 /usr/local/bin/fsctdriverservice
chmod 644 /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist

launchctl load -w /Library/LaunchDaemons/com.hem-e.fsctdriverservice.plist 2>> /tmp/postinstall.log

logger -s "Postinstall finished" 2>> /tmp/postinstall.log