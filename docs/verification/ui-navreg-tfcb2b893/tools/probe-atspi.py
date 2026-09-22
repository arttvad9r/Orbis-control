#!/usr/bin/env python3
"""Probe: does the live orbis-control window expose an AT-SPI tree?"""
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

desktop = Atspi.get_desktop(0)
print(f"desktop children: {desktop.get_child_count()}")
for i in range(desktop.get_child_count()):
    app = desktop.get_child_at_index(i)
    try:
        name = app.get_name()
    except Exception as e:
        name = f"<err {e}>"
    print(f"app[{i}] name={name!r}")
    try:
        n = app.get_child_count()
    except Exception as e:
        print(f"  child_count err: {e}")
        continue
    for j in range(n):
        win = app.get_child_at_index(j)
        try:
            wn = win.get_name()
            ext = win.get_extents(Atspi.CoordType.WINDOW)
            print(f"  win[{j}] name={wn!r} extents=({ext.x},{ext.y},{ext.width},{ext.height})")
        except Exception as e:
            print(f"  win[{j}] err: {e}")
