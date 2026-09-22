
from PIL import Image
import os
REPO = "/home/artt/Orbis-control-implementation"
def probe(path):
    img = Image.open(path).convert("RGB"); W,H = img.size
    page_bg = img.getpixel((W-5, 400))
    border_x = None
    for x in range(W-2, 900, -1):
        hits = sum(1 for y in range(110,250,4) if sum(abs(a-b) for a,b in zip(img.getpixel((x,y)), page_bg)) > 12)
        if hits >= 30: border_x = x; break
    outside = []
    for y in range(120, 260):
        for x in range(border_x+1, W-1):
            if sum(abs(a-b) for a,b in zip(img.getpixel((x,y)), page_bg)) > 60:
                outside.append((x,y))
    return border_x, len(outside), (outside[:3] if outside else [])
for d in ["base","merged"]:
    for f in ["about-980x680-dark.png","about-980x680-light.png","about-1200x800-dark.png","about-1200x800-light.png"]:
        p = os.path.join(REPO, ".int-smoke", d, f)
        if os.path.exists(p):
            bx, n, ex = probe(p)
            print("%-6s %-26s border_x=%s px_outside_card=%d %s" % (d, f, bx, n, ex))
