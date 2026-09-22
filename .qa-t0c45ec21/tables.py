#!/usr/bin/env python3
import sqlite3
db = sqlite3.connect("file:/home/artt/.hermes/kanban/boards/orbis-v01/kanban.db?mode=ro", uri=True)
for r in db.execute("select name from sqlite_master where type='table'"):
    print(r[0])
