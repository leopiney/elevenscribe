#!/bin/bash
# Grant microphone permission to elevenscribe via the macOS TCC database.
#
# WKWebView (used by Tauri/Wry) doesn't trigger the macOS "Allow microphone?"
# dialog, so if the TCC entry is ever removed (e.g. by tccutil reset or a
# macOS update), the app silently fails to access the mic. This script
# re-inserts the permission directly.
#
# Usage: ./scripts/grant-mic-permission.sh

BUNDLE_ID="com.pentoai.elevenscribe"
TCC_DB="$HOME/Library/Application Support/com.apple.TCC/TCC.db"

sqlite3 "$TCC_DB" \
  "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version, flags) VALUES ('kTCCServiceMicrophone', '$BUNDLE_ID', 0, 2, 0, 1, 0);"

if [ $? -eq 0 ]; then
  echo "Microphone permission granted to $BUNDLE_ID"
  echo "Restart elevenscribe for the change to take effect."
else
  echo "Failed to update TCC database. You may need Full Disk Access for your terminal." >&2
  exit 1
fi
