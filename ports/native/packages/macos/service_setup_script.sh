#!/bin/bash

echo "Postinstall started" >> /tmp/postinstall.log
logger "Postinstall started"

chmod 755 /usr/local/bin/fsctservice
chmod 644 /Library/LaunchDaemons/com.hem-e.fsctservice.plist

launchctl disable system com.hem-e.fsctservice
launchctl remove com.hem-e.fsctservice

launchctl load -w /Library/LaunchDaemons/com.hem-e.fsctservice.plist

echo "Postinstall finished" >> /tmp/postinstall.log
logger "Postinstall finished"