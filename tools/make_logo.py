"""Generates assets/logo.png and assets/icon.ico (original RustBlox logo).

Run from the project root:  python tools/make_logo.py   (needs Pillow)
"""
import os

from PIL import Image, ImageDraw

S = 2048  # supersampled canvas


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vgrad(size, top, bottom):
    img = Image.new("RGB", size)
    px = img.load()
    for y in range(size[1]):
        c = lerp(top, bottom, y / (size[1] - 1))
        for x in range(size[0]):
            px[x, y] = c
    return img


def dgrad(size, a, b):
    img = Image.new("RGB", size)
    px = img.load()
    n = size[0] + size[1] - 2
    for y in range(size[1]):
        for x in range(size[0]):
            px[x, y] = lerp(a, b, (x + y) / n)
    return img


bg = vgrad((S, S), (28, 32, 66), (12, 14, 30)).convert("RGBA")
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.22), fill=255)
bg.putalpha(mask)

ring_mask = Image.new("L", (S, S), 0)
d = ImageDraw.Draw(ring_mask)
cx = cy = S // 2
r_out, r_in = int(S * 0.36), int(S * 0.285)
d.ellipse([cx - r_out, cy - r_out, cx + r_out, cy + r_out], fill=255)
d.ellipse([cx - r_in, cy - r_in, cx + r_in, cy + r_in], fill=0)
# open the ring at the top so the chevrons "launch" through it
d.polygon(
    [(cx - 140, cy - r_out - 20), (cx + 140, cy - r_out - 20), (cx + 140, cy - r_in + 20), (cx - 140, cy - r_in + 20)],
    fill=0,
)
ring = dgrad((S, S), (255, 138, 61), (255, 61, 113)).convert("RGBA")
ring.putalpha(ring_mask)
bg = Image.alpha_composite(bg, ring)

ch = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(ch)


def chevron(y_tip, half_w, thick):
    pts = [
        (cx, y_tip),
        (cx + half_w, y_tip + half_w),
        (cx + half_w - thick * 0.9, y_tip + half_w + thick * 0.9),
        (cx, y_tip + thick * 1.8),
        (cx - half_w + thick * 0.9, y_tip + half_w + thick * 0.9),
        (cx - half_w, y_tip + half_w),
    ]
    d.polygon(pts, fill=(255, 255, 255, 255))


chevron(cy - 330, 300, 130)
chevron(cy - 40, 300, 130)
bg = Image.alpha_composite(bg, ch)

final = bg.resize((1024, 1024), Image.LANCZOS)
os.makedirs("assets", exist_ok=True)
final.resize((256, 256), Image.LANCZOS).save("assets/logo.png")
final.save("assets/icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("logo written")
