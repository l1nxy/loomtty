#!/bin/sh
# Run by the .deb/.rpm package after install and after removal: refresh the
# desktop database and icon cache so the loomtty launcher entry and icon
# appear / disappear without a re-login. Best-effort — never fail the package
# transaction over a missing cache tool.
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -qtf /usr/share/icons/hicolor 2>/dev/null || true
fi
exit 0
