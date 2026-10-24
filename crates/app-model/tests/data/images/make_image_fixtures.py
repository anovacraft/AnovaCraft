"""Fixtures and reference values for image tables (app_model::image_table).

Writes small images in every format and pixel kind AnovaCraft opens, then
decodes each with Pillow and stores the decoded pixels in
image_reference.json. tests/image_tables.rs opens the same files and checks
the table against Pillow's pixels: x and y of every pixel, its channels, and
the Rec. 709 luminance 0.2126 R + 0.7152 G + 0.0722 B.

Run from this folder:  python3 make_image_fixtures.py   (Pillow, NumPy)
"""

import json

import numpy as np
from PIL import Image

W, H = 13, 9  # not square, so a transposed reading fails
yy, xx = np.mgrid[0:H, 0:W]


def ramp(scale, phase):
    return ((xx * 19 + yy * 7 + phase) * 37 % 251) * scale / 250.0


gray8 = ramp(255, 0).round().astype(np.uint8)
rgb8 = np.stack([ramp(255, p).round().astype(np.uint8) for p in (0, 50, 120)], axis=-1)
gray16 = ramp(65535, 3).round().astype(np.uint16)

files = {}


def save(name, image, **kw):
    image.save(name, **kw)
    files[name] = None


save("gray8.png", Image.fromarray(gray8, "L"))
save("rgb8.png", Image.fromarray(rgb8, "RGB"))
rgba = np.concatenate([rgb8, (ramp(255, 9)).round().astype(np.uint8)[..., None]], axis=-1)
save("rgba8.png", Image.fromarray(rgba, "RGBA"))
save("palette.png", Image.fromarray(rgb8, "RGB").quantize(colors=16))
save("gray16.png", Image.fromarray(gray16.astype(np.int32), "I").convert("I;16"))
save("gray16.tif", Image.fromarray(gray16.astype(np.int32), "I").convert("I;16"))
save("rgb8.tif", Image.fromarray(rgb8, "RGB"))
save("rgb8.bmp", Image.fromarray(rgb8, "RGB"))
save("gray8.jpg", Image.fromarray(gray8, "L"), quality=95)
save("rgb8.jpg", Image.fromarray(rgb8, "RGB"), quality=95)

reference = {}
for name in files:
    im = Image.open(name)
    mode = im.mode
    if mode == "P" or mode == "RGBA":
        a = np.asarray(im.convert("RGB")).astype(float)
    elif mode in ("I;16", "I;16B", "I"):
        a = np.asarray(im).astype(float)
    else:
        a = np.asarray(im).astype(float)
    if a.ndim == 2:
        channels = {"intensity": a}
    else:
        r, g, b = a[..., 0], a[..., 1], a[..., 2]
        channels = {"red": r, "green": g, "blue": b, "luminance": 0.2126 * r + 0.7152 * g + 0.0722 * b}
    reference[name] = {
        "mode": mode,
        "width": im.width,
        "height": im.height,
        "lossy": name.endswith(".jpg"),
        "channels": {k: v.reshape(-1).tolist() for k, v in channels.items()},
    }

with open("image_reference.json", "w") as f:
    json.dump(reference, f, indent=0)
