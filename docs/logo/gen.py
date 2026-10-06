# The kit's mark, traced from Cole's reference (ref2.png, 1536x1024), without the motion lines:
# two Inertia-style chevrons (violet -> indigo, the lower half a shade deeper) running into a
# ten-tooth gear (peach -> burnt orange) whose inner edge is a crescent around them, its two
# ends fading out to the left. Geometry is in the reference's pixels:
#   chevrons: top y=294, bottom y=762, apex y=530; arm width ~210; back chevron's left edge
#     x=384 at the top, front one x=656; apexes x=803 and x=1047.
#   gear: centre (868,530), root r=350, tip r=418, ten teeth every 36° (one at 0°), each
#     ~18° wide at the tip; the ring spans -112°..118°.
#   crescent inner edge: circle centred (800,524), r=277.
# Writes mark.svg (colour) and mark-mono.svg (currentColor, with --mono) into the working
# directory. With --icon it writes the favicon pair instead: icon.svg, the colour mark bare on a
# square canvas (it reads larger at 16 px than on a tile), and icon-tile.svg, the same on an
# opaque white square for icon.png (the apple-touch icon, where iOS fills transparency with black):
#   cd public && python3 ../docs/logo/gen.py --icon && rsvg-convert -w 512 -h 512 icon-tile.svg -o icon.png && rm icon-tile.svg
import math, sys

def f(p): return f"{p[0]:.1f},{p[1]:.1f}"

# --- chevrons -------------------------------------------------------------------------------
TOP, BOT, MID = 294, 762, 530
ARM = 206
RUN = 0.9 * (MID - TOP)   # the reference's arms run 0.9 px across per px down
CORNER = 10       # small radius on the outer corners, like the reference

def chevron(x0):
    # left edge x0 at the top; apex = x0 + ARM + RUN
    p = [(x0, TOP), (x0 + ARM, TOP), (x0 + ARM + RUN, MID), (x0 + ARM, BOT), (x0, BOT), (x0 + RUN, MID)]
    return p

def rounded(points, r):
    # polygon with each corner cut by a small quadratic curve of radius ~r
    n = len(points); d = ""
    for i in range(n):
        p0 = points[i - 1]; p1 = points[i]; p2 = points[(i + 1) % n]
        def toward(a, b, dist):
            L = math.hypot(b[0] - a[0], b[1] - a[1]); t = min(dist / L, 0.5)
            return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
        a = toward(p1, p0, r); b = toward(p1, p2, r)
        d += (f"M{f(a)}" if i == 0 else f" L{f(a)}") + f" Q{f(p1)} {f(b)}"
    return d + " Z"

chev_back = chevron(382)
chev_front = chevron(646)
chev_d = rounded(chev_back, CORNER) + " " + rounded(chev_front, CORNER)

# --- gear -----------------------------------------------------------------------------------
GX, GY = 868, 530
R_ROOT, R_TIP = 350, 418
TEETH = 10
TOOTH_TIP_DEG, TOOTH_ROOT_DEG = 15.0, 23.0
A_FROM, A_TO = -112, 118       # where the ring exists (degrees, 0 = right, y down)
IX, IY, IR = 800, 524, 277     # crescent inner circle

def pol(r, deg, cx=GX, cy=GY):
    a = math.radians(deg); return (cx + r * math.cos(a), cy + r * math.sin(a))

def gear_outline():
    # Outer boundary from A_FROM to A_TO: root circle with trapezoid teeth.
    pts = [pol(R_ROOT, A_FROM)]
    for i in range(TEETH):
        c = i * 36 + 12
        for k in (c - 360, c):
            if True:
                pts += [pol(R_ROOT, k - TOOTH_ROOT_DEG / 2), pol(R_TIP, k - TOOTH_TIP_DEG / 2),
                        pol(R_TIP, k + TOOTH_TIP_DEG / 2), pol(R_ROOT, k + TOOTH_ROOT_DEG / 2)]
    pts = pts[1:]
    pts.sort(key=lambda p: (math.degrees(math.atan2(p[1] - GY, p[0] - GX))))
    # dedupe teeth listed twice (k-360 and k)
    seen = []; out = []
    for p in pts:
        key = (round(p[0], 1), round(p[1], 1))
        if key not in seen: seen.append(key); out.append(p)
    pts = out
    d = "M" + f(pts[0]) + "".join(f" L{f(p)}" for p in pts[1:])
    # back along the crescent's inner circle (counter-clockwise) to the start
    return d + " Z"

def inner_point(deg):
    # where the ray from the gear centre at `deg` meets the inner circle (the far intersection)
    a = math.radians(deg); dx, dy = math.cos(a), math.sin(a)
    ox, oy = GX - IX, GY - IY
    b = ox * dx + oy * dy; c = ox * ox + oy * oy - IR * IR
    t = -b + math.sqrt(b * b - c)
    return (GX + t * dx, GY + t * dy)

gear_d = gear_outline()
_w = R_TIP + 60
def _wedge(a_to, a_from):
    # the region from the centre sweeping from a_to (lower-left) round through 180° to a_from
    # (upper-left), sampled so no corner wraps the wrong way
    pts = [pol(_w, a) for a in [a_to + (a_from + 360 - a_to) * k / 24 for k in range(25)]]
    return f"M{f((GX, GY))} " + " ".join(f"L{f(p)}" for p in pts) + " Z"
wedge_d = _wedge(A_TO, A_FROM)
mono_wedge_d = _wedge(A_TO - 14, A_FROM + 14)
inner_d = f"M{IX - IR},{IY} a{IR},{IR} 0 1,0 {2 * IR},0 a{IR},{IR} 0 1,0 {-2 * IR},0 Z"

# --- canvas ---------------------------------------------------------------------------------
PAD = 24
minx, maxx = 382 - PAD, GX + R_TIP + PAD
miny, maxy = GY - R_TIP - PAD, GY + R_TIP + PAD
vb = f"{minx} {miny} {maxx - minx} {maxy - miny}"
import os
if os.environ.get("FULL"): vb = "0 0 1536 1024"

mono = "--mono" in sys.argv
if mono:
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" fill="currentColor">
  <mask id="cut" maskUnits="userSpaceOnUse" x="{minx}" y="{miny}" width="{maxx - minx}" height="{maxy - miny}">
    <rect x="{minx}" y="{miny}" width="{maxx - minx}" height="{maxy - miny}" fill="#fff"/>
    <path d="{inner_d}" fill="#000"/>
    <path d="{mono_wedge_d}" fill="#000"/>
  </mask>
  <path d="{gear_d}" mask="url(#cut)"/>
  <path d="{chev_d}"/>
</svg>
'''
else:
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}">
  <defs>
    <linearGradient id="gear" gradientUnits="userSpaceOnUse" x1="{GX - 120}" y1="{GY - R_TIP}" x2="{GX + 160}" y2="{GY + R_TIP}">
      <stop offset="0" stop-color="#FF9636"/>
      <stop offset="0.55" stop-color="#F0621F"/>
      <stop offset="1" stop-color="#C23414"/>
    </linearGradient>
    <linearGradient id="fade" gradientUnits="userSpaceOnUse" x1="{IX - 150}" y1="0" x2="{IX + 10}" y2="0">
      <stop offset="0" stop-color="#fff" stop-opacity="0"/>
      <stop offset="1" stop-color="#fff" stop-opacity="1"/>
    </linearGradient>
    <mask id="ends" maskUnits="userSpaceOnUse" x="{minx}" y="{miny}" width="{maxx - minx}" height="{maxy - miny}">
      <rect x="{minx}" y="{miny}" width="{maxx - minx}" height="{maxy - miny}" fill="url(#fade)"/>
      <path d="{inner_d}" fill="#000"/>
      <path d="{wedge_d}" fill="#000"/>
    </mask>
    <linearGradient id="chevTop" gradientUnits="userSpaceOnUse" x1="0" y1="{TOP}" x2="0" y2="{MID}">
      <stop offset="0" stop-color="#9A5CFB"/>
      <stop offset="1" stop-color="#6E47F5"/>
    </linearGradient>
    <linearGradient id="chevBot" gradientUnits="userSpaceOnUse" x1="0" y1="{MID}" x2="0" y2="{BOT}">
      <stop offset="0" stop-color="#5A36E0"/>
      <stop offset="1" stop-color="#3F22B8"/>
    </linearGradient>
    <clipPath id="upper"><rect x="{minx}" y="{miny}" width="{maxx - minx}" height="{MID - miny}"/></clipPath>
    <clipPath id="lower"><rect x="{minx}" y="{MID}" width="{maxx - minx}" height="{maxy - MID}"/></clipPath>
  </defs>
  <path d="{gear_d}" fill="url(#gear)" mask="url(#ends)"/>
  <path d="{chev_d}" fill="url(#chevTop)" clip-path="url(#upper)"/>
  <path d="{chev_d}" fill="url(#chevBot)" clip-path="url(#lower)"/>
</svg>
'''
if "--icon" in sys.argv:
    body = svg[svg.index(">") + 1:svg.rindex("</svg>")]
    cx, cy = (382 + GX + R_TIP) / 2, GY
    def square(side, background=""):
        x, y = cx - side / 2, cy - side / 2
        rect = f'\n  <rect x="{x:g}" y="{y:g}" width="{side}" height="{side}" fill="{background}"/>' if background else ""
        return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x:g} {y:g} {side} {side}" width="512" height="512">{rect}{body}</svg>\n'
    open("icon.svg", "w").write(square(R_TIP * 2 + 88))
    open("icon-tile.svg", "w").write(square(1200, "#fff"))
else:
    open("mark-mono.svg" if mono else "mark.svg", "w").write(svg)
print(vb)
