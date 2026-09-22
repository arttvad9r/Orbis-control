import sys
try:
    from PIL import Image
    print("PIL OK", sys.executable)
except Exception as e:
    print("PIL FAIL:", e, sys.executable)
