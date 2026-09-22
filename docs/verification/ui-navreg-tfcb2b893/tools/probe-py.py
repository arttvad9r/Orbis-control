import sys
print(sys.version)
print(sys.path)
try:
    import gi
    print("gi OK", gi.__file__)
except Exception as e:
    print("gi FAIL:", e)
