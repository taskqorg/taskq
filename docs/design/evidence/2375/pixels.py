#!/usr/bin/env python3
"""Read alpha and colour at the titlebar and body of each window capture. Usage: pixels.py PNG..."""
import json, pathlib, sys
from PIL import Image

for path in sys.argv[1:]:
    image = Image.open(path).convert("RGBA")
    w, h = image.size
    # Titlebar centre, then a body point inside the page background beside the rounded corners.
    bar = image.getpixel((w // 2, 20))
    body = image.getpixel((w // 2, h - 30))
    print(json.dumps({"file": pathlib.Path(path).name, "size": [w, h], "titlebar_rgba": bar, "body_rgba": body}))
