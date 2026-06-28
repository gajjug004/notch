import math
from PIL import Image, ImageDraw, ImageFilter

ss = 4              # supersample factor for anti-aliasing
BASE = 1024
S = BASE * ss
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

YELLOW = (255, 201, 64, 255)
YELLOW_DK = (242, 178, 40, 255)
INK = (58, 47, 20, 255)
RED = (226, 72, 61, 255)
WHITE = (255, 252, 244, 255)
BAR = (140, 110, 40, 90)

m = int(S * 0.13)               # outer margin
note = [m, m, S - m, S - m]
r = int(S * 0.11)               # corner radius

# ---- drop shadow ----
sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ds = ImageDraw.Draw(sh)
off = int(S * 0.022)
ds.rounded_rectangle(
    [note[0] + off, note[1] + off, note[2] + off, note[3] + off],
    radius=r, fill=(0, 0, 0, 120),
)
sh = sh.filter(ImageFilter.GaussianBlur(int(S * 0.018)))
img = Image.alpha_composite(img, sh)

d = ImageDraw.Draw(img)

# ---- note body ----
d.rounded_rectangle(note, radius=r, fill=YELLOW)
# subtle darker header band (top, rounded corners via clip rect)
band_h = int((note[3] - note[1]) * 0.0)  # keep flat; band omitted for cleanliness

# ---- two "text" bars near the top (evokes a note) ----
nx0, ny0, nx1, ny1 = note
pad = int((nx1 - nx0) * 0.16)
bar_h = int(S * 0.028)
by = ny0 + int((ny1 - ny0) * 0.17)
d.rounded_rectangle([nx0 + pad, by, nx1 - pad, by + bar_h],
                    radius=bar_h // 2, fill=BAR)
by2 = by + int(bar_h * 2.1)
d.rounded_rectangle([nx0 + pad, by2, nx1 - int(pad * 2.2), by2 + bar_h],
                    radius=bar_h // 2, fill=BAR)

# ---- clock / timer ----
cx = S * 0.5
cy = ny0 + (ny1 - ny0) * 0.62
cr = (nx1 - nx0) * 0.255

# face
d.ellipse([cx - cr, cy - cr, cx + cr, cy + cr], fill=WHITE)
ring = int(S * 0.013)
d.ellipse([cx - cr, cy - cr, cx + cr, cy + cr], outline=INK, width=ring)

# tick marks
for i in range(12):
    a = math.radians(i * 30)
    sin, cos = math.sin(a), -math.cos(a)
    major = (i % 3 == 0)
    r1 = cr * (0.78 if major else 0.85)
    r2 = cr * 0.93
    w = int(S * (0.011 if major else 0.006))
    d.line([cx + sin * r1, cy + cos * r1, cx + sin * r2, cy + cos * r2],
           fill=INK, width=w)

# hands (minute up, hour to ~2 o'clock)
def hand(angle_deg, length, width, color):
    a = math.radians(angle_deg)
    x = cx + math.sin(a) * length
    y = cy - math.cos(a) * length
    d.line([cx, cy, x, y], fill=color, width=width)

hand(0, cr * 0.70, int(S * 0.018), INK)     # minute
hand(115, cr * 0.46, int(S * 0.024), RED)   # hour
d.ellipse([cx - S * 0.018, cy - S * 0.018, cx + S * 0.018, cy + S * 0.018],
          fill=RED)

# downscale for anti-aliasing
out = img.resize((BASE, BASE), Image.LANCZOS)
out.save("icon_src.png")
print("wrote icon_src.png", out.size)

# Regenerate the app icon set from this source:
#   cd design && uv run --with Pillow python make_icon.py
#   cd .. && npm run tauri icon design/icon_src.png
