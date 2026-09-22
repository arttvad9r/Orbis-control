#!/bin/sh
# Probe available D-Bus service-authoring options for the scripted Session1 peer.
echo "== python gi =="
python3 - <<'EOF'
try:
    import gi
    gi.require_version('Gio', '2.0')
    from gi.repository import Gio, GLib
    print('gi Gio OK')
except Exception as e:
    print('gi FAIL:', e)
try:
    import dbus
    from dbus.mainloop.glib import DBusGMainLoop
    print('python-dbus OK')
except Exception as e:
    print('python-dbus FAIL:', e)
EOF
echo "== cli tools =="
command -v gdbus busctl qdbus busctl-introspect 2>/dev/null
echo done
