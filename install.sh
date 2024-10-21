#!/bin/bash -e

cargo build

sudo cp ./target/debug/hotkeyd /Library/PrivilegedHelperTools/

sudo launchctl unload -w /Library/LaunchDaemons/hotkeyd.plist

sudo cp hotkeyd.plist /Library/LaunchDaemons/

sudo truncate -s 0 /Library/Logs/hotkeyd-stdout.log
sudo truncate -s 0 /Library/Logs/hotkeyd-stderr.log

sudo launchctl load -w /Library/LaunchDaemons/hotkeyd.plist
sudo launchctl start hotkeyd
