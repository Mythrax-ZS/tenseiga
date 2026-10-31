"""Samurai skill icons: 24x24 pixel art in the manner of the Guillotine Cross icons.

The frame and every motif are drawn here pixel by pixel; nothing is copied from game
data. Divinity (Kusanagi) skills use pale blades with gold light; Cursed (Muramasa)
skills use crimson blades with wine-dark auras.

CLI: py skill_icons.py --output <overlay root> [--preview preview.png]
Writes data/texture/유저인터페이스/item/<skill>.bmp (24-bit, magenta = transparent),
the names the client looks up before falling back to a stock icon.
"""
from __future__ import annotations

import argparse
import math
from pathlib import Path

from PIL import Image

PALETTE = {
    "#": (255, 0, 255),  # transparent key
    ".": (0x2A, 0x27, 0x3B), ",": (0x2F, 0x1F, 0x4C), "x": (0x16, 0x16, 0x30),
    "1": (0x4C, 0x3A, 0x6A), "2": (0x68, 0x55, 0x87), "3": (0x81, 0x70, 0xA2), "4": (0x9B, 0x8B, 0xBA),
    "5": (0xB5, 0xA6, 0xD2), "6": (0xCB, 0xC0, 0xE9), "7": (0xED, 0xED, 0xFF),
    "w": (0xFF, 0xF3, 0xEB), "c": (0xEE, 0xDF, 0xD7), "C": (0xDC, 0xCB, 0xC3), "k": (0xCA, 0xB7, 0xAF),
    "s": (0xB8, 0xA3, 0x9B), "g": (0xA9, 0x8F, 0x8B), "h": (0x99, 0x7B, 0x7B), "G": (0x89, 0x67, 0x6B),
    "b": (0x76, 0x3A, 0x32), "B": (0x4F, 0x26, 0x1C),
    "m": (0x5C, 0x25, 0x3B), "n": (0x80, 0x41, 0x51), "N": (0x9E, 0x5C, 0x66),
    "r": (0x8F, 0x22, 0x18), "R": (0xBD, 0x64, 0x55), "o": (0xED, 0x64, 0x47),
    "A": (0xEA, 0xA1, 0x71), "a": (0xF7, 0xA5, 0x72), "f": (0xFF, 0xC0, 0x8C),
    "e": (0xFE, 0xDE, 0xAC), "y": (0xFF, 0xE5, 0xA3), "Y": (0xFF, 0xFF, 0xCB),
    # Tenseiga (Mortal Flame line): indigo field, crimson hilt, magenta runes, violet body,
    # cyan edge.
    "d": (0x1A, 0x12, 0x3A), "D": (0x2A, 0x1C, 0x55), "z": (0x7A, 0x0E, 0x26), "K": (0xE0, 0x22, 0x44),
    "p": (0xF0, 0x52, 0xB4), "v": (0x7A, 0x46, 0xE6), "V": (0xA8, 0x86, 0xFF),
    "q": (0x2C, 0xB8, 0xF0), "Q": (0xA6, 0xEE, 0xFF), "W": (0xFF, 0xFF, 0xFF),
}

# Interior: 18 columns x 16 rows, placed at (3, 4). The first character of each icon's
# grid sets its field colour.
ICONS = {
    # Divinity: a katana falls from a burst of heavenly light (the critical strike).
    "sam_heavens_severance": [
        ",,,,,,,,,,,,,,o,,,",
        ",,3,,,,,,,,,,,a,,,",
        ",,,,,,,,,,,,,aya,,",
        ",,,,,,,,,,,oaywyao",
        ",,,,,,,,,,,,,aya,,",
        ",,,,,,,,,,,,w,o,,,",
        ",,,,,,,,,,,wck,,,,",
        ",,,,,,,,,,wcCk,,,,",
        ",,,,,,,,,wcCk,,,1,",
        ",,,,,,,,wcCk,,,,,,",
        ",,,,,,,wcCk,,,,,,,",
        ",,,,,,wcCk,,,,,,,,",
        ",,,,,wcCk,,,,,,,,,",
        ",,,ayea,,,,,,,,,,,",
        ",,,Rb,,,,,,,,,,,,,",
        ",,bR,,,,,,,,,,,,,,",
    ],
    # Divinity: the upright katana turns blows aside (a spark and a parry arc).
    "sam_kusanagi_guard": [
        ".........w........",
        "........wck.4.....",
        "....o...wck..5....",
        "...aya..wck..56..3",
        "..oywyo.wck...6...",
        "...aya..wck...6...",
        "....o...wck..56..3",
        "........wck..5....",
        "........wck.4.....",
        "........wck.......",
        "......AayyyaA.....",
        "........bRb.......",
        "........RbR.......",
        "........bRb.......",
        "........RbR.......",
        ".........B........",
    ],
    # Divinity: a crescent moon; a blow glances off the blade and is sent back.
    "sam_moonlit_reflection": [
        ",,,,,,,,,,,,,,,,,,",
        ",,,,5677,,,,,,,,w,",
        ",,,567,,,,,,,,,wc,",
        ",,567,,,,,,,,,wck,",
        ",,67,,,,,,,,,wck,,",
        ",567,,,,,,,,wck,,,",
        ",567,,,,,,,wck,,,,",
        ",567,,,,,,wck,,,6,",
        ",,67,,,,,wckya4567",
        ",,567,,ayea,a,,,6,",
        ",,,567Rb,,,,,4,,,,",
        ",,,,5Rb7,,,,,,3,,,",
        ",,,,Bb,,,,,,,,,2,,",
        ",,,,,,,,,,,,,,,,1,",
        ",,,,,,,,,,,,,,,,,,",
        ",,,,,,,,,,,,,,,,,,",
    ],
    # Divinity: two blades moving as one, trailing speed lines.
    "sam_blade_harmony": [
        "............w..a..",
        "...........wc.aya.",
        "..........wck..a..",
        "..11234..wck......",
        ".........wck....w.",
        ".......wck.....wc.",
        "1234..wck.....wck.",
        "....ayea.....wck..",
        "....Rb......wck...",
        "...bR......wck....",
        "..Bb......wck.....",
        ".1234....ayea.....",
        "........Rb........",
        ".......bR.........",
        ".123..Bb..........",
        "..................",
    ],
    # Cursed: a crimson blade sheds three drops of its wielder's blood.
    "sam_blood_covenant": [
        "...............o..",
        "..............oR..",
        ".............oRr..",
        "............oRr...",
        "...........oRr....",
        "..........oRr.....",
        ".........oRr....r.",
        "........oRr....rRr",
        ".......oRr..r..roR",
        ".....GssG..rRr..r.",
        ".....xr....roR....",
        "....rx..r...r.....",
        "...xr..rRr........",
        ".......roR........",
        "........r.........",
        "..................",
    ],
    # Cursed: the blade burns with a dark flame under watching red eyes.
    "sam_muramasas_curse": [
        ",,,,,,,,,,,,,,,,,,",
        ",,ro,,,,,,,,,,or,,",
        ",,,ryo,,,,,,oyr,,,",
        ",,,,,,,,,o,,,,,,,,",
        ",,,,,n,,oRr,,n,,,,",
        ",,,,,mn,oRr,nm,,,,",
        ",,,n,mr,oRr,rm,n,,",
        ",,,mnrr,oRr,rrnm,,",
        ",,,,mrr,oRr,rrm,,,",
        ",,,nmrr,oRr,rrmn,,",
        ",,,,mmr,oRr,rmm,,,",
        ",,,,,mm,oRr,mm,,,,",
        ",,,,,,xGsksGx,,,,,",
        ",,,,,,,,xrx,,,,,,,",
        ",,,,,,,,rxr,,,,,,,",
        ",,,,,,,,,x,,,,,,,,",
    ],
    # Cursed: a whirl of ash-grey cuts, flecked with blood.
    "sam_ashen_wind_slash": [
        "..................",
        "..r....GGghh...R..",
        "....ww.....ssk..r.",
        "...cwC......gkC...",
        "...ckC......hhc...",
        "..CCs...R....scc..",
        "..kh..........kw..",
        "..kg..........Cw..",
        "..s.........r.....",
        "..h...............",
        "..gg..........GG..",
        "...g..........g...",
        "...G.........hg...",
        ".r....wkkshssh....",
        "..R...wwcCCk......",
        "..................",
    ],
    # Cursed: the chains break and the crimson blade comes down in a burst.
    "sam_wrath_unbound": [
        ",,,,a,,,xrx,,,a,,,",
        ",,,o,,,,rxr,,,,o,,",
        ",,or,,,,xrx,,,,ro,",
        ",,,,o,xGsksGx,o,,,",
        ",,,r,,,,oRr,,,,r,,",
        ",,or,,,,oRr,,,,ro,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,o,,,o,,,o,,,,",
        ",,,,,,ra,y,ar,,,,,",
        ",,,ro,aeyYyea,or,,",
        ",,oafyeYYwYYeyfao,",
        ",oaeyYwwwwwwwYyeao",
    ],
}


class Canvas:
    """An 18x16 interior on an indigo field, drawn with shapes rather than typed out: the
    Mortal Flame (Tenseiga) icons."""

    def __init__(self, field="d"):
        self.px = [[field] * 18 for _ in range(16)]

    def put(self, x, y, c):
        x, y = round(x), round(y)
        if 0 <= x < 18 and 0 <= y < 16:
            self.px[y][x] = c

    def line(self, x0, y0, x1, y1, c):
        n = max(abs(x1 - x0), abs(y1 - y0), 1) * 2
        for i in range(int(n) + 1):
            t = i / n
            self.put(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, c)

    def where(self, test, c):
        for y in range(16):
            for x in range(18):
                if test(x, y):
                    self.px[y][x] = c

    def rows(self):
        return ["".join(row) for row in self.px]


def dist_to_line(x, y, x0, y0, x1, y1):
    dx, dy = x1 - x0, y1 - y0
    t = max(0.0, min(1.0, ((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy)))
    return math.hypot(x - (x0 + dx * t), y - (y0 + dy * t))


def mortal_flame():
    """A flame of mortal will: crimson outside, magenta and violet within, a cyan-white core
    (Tenseiga's colours, hilt to edge)."""
    c = Canvas()
    for y in range(16):
        for x in range(18):
            v = (15.5 - y) / 15  # 0 at the foot, 1 at the tip
            half = 7.2 * math.sin(math.pi * min(1.0, v * 0.9 + 0.08)) ** 0.8 * (1 - 0.55 * v)
            # Tongues licking upward.
            half += 1.2 * math.sin(x * 1.3 + v * 9) * v
            r = abs(x - 8.5) / max(half, 0.01)
            if r > 1:
                continue
            core = r + v * 0.9
            c.put(x, y, "W" if core < 0.45 else "Q" if core < 0.7 else "v" if core < 0.95 else "p" if core < 1.25 else "K" if r < 0.85 else "z")
    return c.rows()


def sever_heavens_lies():
    """A golden halo, the goddess's lie, cut through by a cyan slash and falling apart."""
    c = Canvas()
    slash = (16.5, 0.5, 1.5, 15.0)
    for y in range(16):
        for x in range(18):
            ring = math.hypot((x - 8.5) / 1.0, (y - 7.0) / 0.62)
            if 4.4 <= ring <= 5.9 and dist_to_line(x, y, *slash) > 1.6:
                # The far side of the halo sits a pixel lower: the cut has shifted it.
                c.put(x, y + (1 if x > 9 else 0), "Y" if y < 6 else "y" if y < 9 else "e")
    c.where(lambda x, y: dist_to_line(x, y, *slash) < 0.75, "Q")
    c.where(lambda x, y: 0.75 <= dist_to_line(x, y, *slash) < 1.2 and (x + y) % 2 == 0, "q")
    for x, y in ((14, 3), (3, 12), (12, 1), (5, 14)):
        c.put(x, y, "W")
    return c.rows()


def light_and_shadow():
    """Two crossing cuts: one of light (white and gold), one of shadow (violet), meeting in a
    magenta spark."""
    c = Canvas()
    light, shadow = (2, 1, 15.5, 14.5), (15.5, 1, 2, 14.5)
    c.where(lambda x, y: dist_to_line(x, y, *shadow) < 1.3, "D")
    c.where(lambda x, y: dist_to_line(x, y, *shadow) < 0.8, "v")
    c.where(lambda x, y: dist_to_line(x, y, *shadow) < 0.35, "V")
    c.where(lambda x, y: dist_to_line(x, y, *light) < 1.2 and (x + y) % 2 == 0, "e")
    c.where(lambda x, y: dist_to_line(x, y, *light) < 0.75, "Y")
    c.where(lambda x, y: dist_to_line(x, y, *light) < 0.35, "W")
    c.where(lambda x, y: math.hypot(x - 8.7, y - 7.8) < 1.6, "p")
    c.where(lambda x, y: math.hypot(x - 8.7, y - 7.8) < 0.8, "W")
    return c.rows()


def unchained_will():
    """A chain snapped apart, the broken links flying off around a burst of cyan."""
    c = Canvas()
    for cx, cy, tilt in ((4.5, 10.0, -0.5), (13.5, 5.0, -0.5)):
        def ring(x, y, cx=cx, cy=cy, tilt=tilt):
            dx, dy = x - cx, y - cy
            u, w = dx * math.cos(tilt) - dy * math.sin(tilt), dx * math.sin(tilt) + dy * math.cos(tilt)
            return math.hypot(u / 3.6, w / 2.2)
        c.where(lambda x, y, ring=ring: 0.62 <= ring(x, y) <= 1.05, "4")
        c.where(lambda x, y, ring=ring: 0.62 <= ring(x, y) <= 0.8 and y < 9, "6")
    # The broken ends, and the burst between them.
    c.where(lambda x, y: math.hypot(x - 9, y - 7.5) < 2.2, "D")
    for a in range(8):
        ang = a * math.pi / 4 + 0.3
        c.line(9 + math.cos(ang) * 1.2, 7.5 + math.sin(ang) * 1.2, 9 + math.cos(ang) * 3.2, 7.5 + math.sin(ang) * 3.2, "q")
    c.where(lambda x, y: math.hypot(x - 9, y - 7.5) < 1.3, "Q")
    c.put(9, 7, "W")
    c.put(9, 8, "W")
    return c.rows()


def forge_of_ten_souls():
    """An anvil at the First Forge, a flame on it and ten souls circling above."""
    c = Canvas()
    # Anvil: face, waist and foot.
    c.where(lambda x, y: 11 <= y <= 12 and 3 <= x <= 15, "4")
    c.where(lambda x, y: y == 11 and 3 <= x <= 15, "6")
    c.where(lambda x, y: y == 11 and 1 <= x <= 2, "4")
    c.where(lambda x, y: y == 13 and 6 <= x <= 12, "3")
    c.where(lambda x, y: y == 14 and 5 <= x <= 13, "2")
    c.where(lambda x, y: y == 15 and 4 <= x <= 14, "1")
    # The flame on the anvil.
    for y in range(6, 11):
        for x in range(6, 13):
            v = (10.5 - y) / 5
            r = abs(x - 9) / max(2.6 * (1 - v) + 0.3, 0.1)
            if r <= 1:
                c.put(x, y, "W" if r < 0.3 and v < 0.6 else "p" if r < 0.6 else "K")
    # Ten souls in an arc, in the blade's colours.
    colours = "KpvqQKpvqQ"
    for i in range(10):
        ang = math.pi * (1.05 + 0.9 * i / 9)
        c.put(9 + math.cos(ang) * 7.6, 7.2 + math.sin(ang) * 6.0, colours[i])
    return c.rows()


def destinys_severance():
    """The sun-moon disc of the Weaver split in two by one stroke, crimson at the hilt end
    and cyan at the tip."""
    c = Canvas()
    for y in range(16):
        for x in range(18):
            r = math.hypot(x - 8.5, y - 7.5)
            if r <= 6.2:
                left = x < 8.5 + (y - 7.5) * 0.25
                # The halves drift apart.
                shift = -1 if left else 1
                c.put(x + shift, y, ("y" if r < 4.5 else "e") if left else ("V" if r < 4.5 else "v"))
    for y in range(16):
        x = 8.5 + (y - 7.5) * 0.25
        colour = "K" if y > 10 else "p" if y > 5 else "Q"
        c.put(x, y, "W" if 4 <= y <= 11 else colour)
        c.put(x - 1, y, colour)
    return c.rows()


def crescent_gale():
    """A crescent of mortal flame flying right: cyan-white at its leading edge, crimson
    within, magenta and violet streaks trailing it."""
    c = Canvas()
    for y, x0, x1, col in ((4, 1, 9, "v"), (6, 0, 10, "p"), (8, 2, 10, "v"), (10, 0, 9, "p"), (12, 3, 8, "v")):
        c.line(x0, y, x1, y, col)
    for y in range(16):
        for x in range(18):
            outer, inner = math.hypot(x - 7.5, y - 7.5), math.hypot(x - 2.8, y - 7.5)
            if outer > 7.6 or inner < 7.4:
                continue
            edge = 7.6 - outer
            c.put(x, y, "W" if edge < 0.7 else "Q" if edge < 1.3 else "q" if edge < 1.8 else "K" if inner > 8.0 else "z")
    return c.rows()


ICONS.update({
    "sam_mortal_flame": mortal_flame(),
    "sam_sever_heavens_lies": sever_heavens_lies(),
    "sam_light_and_shadow": light_and_shadow(),
    "sam_unchained_will": unchained_will(),
    "sam_forge_of_ten_souls": forge_of_ten_souls(),
    "sam_destinys_severance": destinys_severance(),
    "sam_crescent_gale": crescent_gale(),
})


def frame():
    """An original border in the Guillotine Cross manner: wine side rails, rose cord bands
    and fringe teeth at top and bottom, with magenta corners and side notches."""
    rows = [list("." * 24) for _ in range(24)]
    for y in range(24):
        for x in range(24):
            edge = x in (0, 23)
            if (x, y) in ((0, 0), (23, 0), (0, 23), (23, 23)) or (edge and 8 <= y <= 15):
                rows[y][x] = "#"
    for y, x_of in ((1, 0), (22, 0)):
        for x in range(1, 23):
            rows[y][x] = "G" if x % 5 == 0 else ("r" if x % 5 == 3 else "h")
    for y in (2, 21):
        for x in range(1, 23):
            rows[y][x] = "s" if x % 2 else "g"
    for y in (3, 20):
        for x in range(1, 23):
            rows[y][x] = "k" if x % 3 == 1 else ("G" if x % 3 == 2 else ".")
    for y in range(4, 20):
        rows[y][1], rows[y][22] = "m", "m"
        rows[y][2], rows[y][21] = ("N" if y in (4, 19) else "n"), ("N" if y in (4, 19) else "n")
    for x, y in ((1, 3), (22, 3), (1, 20), (22, 20)):
        rows[y][x] = "c"
    return rows


def render(name):
    grid = ICONS[name]
    if len(grid) != 16 or any(len(row) != 18 for row in grid):
        raise ValueError(f"{name}: the interior must be 16 rows of 18 pixels")
    rows = frame()
    for y, row in enumerate(grid):
        for x, ch in enumerate(row):
            if ch not in PALETTE:
                raise ValueError(f"{name}: unknown colour {ch!r}")
            rows[4 + y][3 + x] = ch
    image = Image.new("RGB", (24, 24))
    image.putdata([PALETTE[ch] for row in rows for ch in row])
    return image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="overlay root (contains data/)")
    parser.add_argument("--preview", type=Path, help="write an 8x contact sheet here")
    args = parser.parse_args()
    folder = args.output / "data/texture/유저인터페이스/item"
    folder.mkdir(parents=True, exist_ok=True)
    images = {name: render(name) for name in ICONS}
    for name, image in images.items():
        image.save(folder / f"{name}.bmp", format="BMP")
    if args.preview:
        scale, gap = 8, 8
        sheet = Image.new("RGB", (len(images) * (24 * scale + gap) + gap, 24 * scale + 2 * gap), (128, 128, 128))
        for i, image in enumerate(images.values()):
            sheet.paste(image.resize((24 * scale, 24 * scale), Image.Resampling.NEAREST), (gap + i * (24 * scale + gap), gap))
        sheet.save(args.preview)
    print("\n".join(sorted(f"{name}.bmp" for name in images)))


if __name__ == "__main__":
    main()
