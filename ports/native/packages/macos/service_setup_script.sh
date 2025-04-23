#!/bin/bash

chmod 755 /usr/local/bin/fsctservice
chmod 644 /Library/LaunchDaemons/com.hem-e.fsctservice.plist

launchctl load /Library/LaunchDaemons/com.hem-e.fsctservice.plist
launchctl start com.hem-e.fsctservice
