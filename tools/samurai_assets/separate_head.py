"""Separate the Samurai's drawn head from its body: player heads, headgear and dyes.

The generated artwork draws the head and a long ponytail into every body frame. This
stage splits each compiled frame in two:

- the body, without head or ponytail and with the coat filled in where the hair lay. It is
  re-encoded as an indexed SPR whose palette keeps each material in a range of its own, so
  dye palettes (data\\palette\\몸\\samurai_남_N.pal) recolour the clothes, and its ACT gets a
  head anchor per frame where a player's head and headgear attach, as on official bodies;
- the hair and face, as a custom hairstyle: a head sprite for every player action
  (data\\sprite\\인간족\\머리통\\{남,여}\\43_{sex}) with hair-colour palettes
  (data\\palette\\머리\\머리43_{sex}_N.pal) that follow the official hair colours.

Steps (Python 3.12+, Pillow, NumPy), from the client repository:

  py tools/samurai_assets/separate_head.py fit <compiled dir> --reference-head <dir>
  py tools/samurai_assets/separate_head.py build <compiled dir> --output <dir>
  py tools/samurai_assets/separate_head.py install <build dir>

Run it after compile.py and compile_weapons.py (whose outputs are in the overlay): `install`
copies the body, hairstyle and palettes into overlays/samurai and pads the weapon ACTs there
to the body's frame counts.

`fit` needs an official male head (2_남.spr/.act) and its hair palettes (머리2_남_1..8.pal)
extracted from the game data into one folder; they are never committed. It places that
head on each frame's drawn head and records the head anchors, head zones and the official
hair-colour ramps in head_cut.json, which is committed and may be corrected by hand.
`build` needs only the compiled body (compile.py's output) and head_cut.json. With
--reference-head it also draws review sheets with the official head on the new body.
"""
from __future__ import annotations

import argparse
import colorsys
import json
import math
from pathlib import Path
import struct

import numpy as np
from PIL import Image, ImageDraw

from ro_formats import DIRECTIONS, read_act, read_spr, write_act, write_spr

HERE = Path(__file__).resolve().parent
CUTS = HERE / "artwork" / "head_cut.json"
HAIRSTYLE = 43
GROUPS = 13
# Dye palettes 1-7 (rAthena's default max_cloth_color is 7); hair colours 1-8 (max_hair_color 8).
DYES = 7
HAIR_COLOURS = 8


# ---------------------------------------------------------------- reading


def rgba_images(spr):
    out = []
    for image in spr["images"]:
        if image["sprite_type"]:
            pixels = image["pixels"]
        else:
            pal = spr["palette"]
            pixels = bytes(c for i in image["pixels"] for c in (pal[4 * i:4 * i + 3] + b"\xff" if i else bytes(4)))
        out.append(np.frombuffer(pixels, np.uint8).reshape(image["height"], image["width"], 4).copy())
    return out


def read_reference_act(path):
    """ACT 2.0-2.5 (official heads are 2.4; ro_formats reads only the 2.5 it writes)."""
    data = Path(path).read_bytes()
    if data[:2] != b"AC":
        raise ValueError(f"{path} is not an ACT")
    version, count = struct.unpack_from("<HH", data, 2)
    off, actions = 16, []
    for _ in range(count):
        (frames,) = struct.unpack_from("<I", data, off)
        off += 4
        out = []
        for _ in range(frames):
            off += 32
            (nl,) = struct.unpack_from("<I", data, off)
            off += 4
            layers = []
            for _ in range(nl):
                x, y, index, mirror = struct.unpack_from("<iiii", data, off)
                off += 16
                if version >= 0x200:
                    off += 4 + (8 if version >= 0x204 else 4) + 8 + (8 if version >= 0x205 else 0)
                layers.append(dict(x=x, y=y, sprite_index=index, mirror=bool(mirror)))
            if version >= 0x200:
                off += 4
            anchors = []
            if version >= 0x203:
                (na,) = struct.unpack_from("<I", data, off)
                off += 4
                for _ in range(na):
                    _, ax, ay, _ = struct.unpack_from("<Iiii", data, off)
                    off += 16
                    anchors.append(dict(x=ax, y=ay))
            out.append(dict(layers=layers, anchors=anchors))
        actions.append(dict(frames=out))
    return actions


def load_body(directory):
    directory = Path(directory)
    spr = read_spr(directory / "ember_ronin.spr")
    act = read_act(directory / "ember_ronin.act")
    return rgba_images(spr), act["actions"]


def image_uses(actions):
    """Every (action, frame, layer) that draws each body image, unmirrored first."""
    uses = {}
    for a, action in enumerate(actions):
        for f, frame in enumerate(action["frames"]):
            for layer in frame["layers"]:
                if layer["sprite_index"] >= 0:
                    uses.setdefault(layer["sprite_index"], []).append((a, f, layer))
    for v in uses.values():
        v.sort(key=lambda u: (u[2].get("mirror", False), u[0], u[1]))
    return uses


# ---------------------------------------------------------------- pixel classes


def classes(img):
    """Boolean maps: opaque, skin, red, gold, light; and a hair likelihood placeholder."""
    r, g, b, a = (img[..., i].astype(np.int32) for i in range(4))
    opaque = a >= 128
    skin = opaque & (r > 170) & (g > 110) & (r - b > 40) & (r >= g)
    red = opaque & (r > 90) & (r > g * 2) & (r > b * 2) & ~skin
    gold = opaque & (r > 140) & (g > 95) & (b < 110) & (r - b > 70) & ~skin & ~red
    mx = np.maximum(np.maximum(r, g), b)
    light = opaque & (mx > 150) & ~skin & ~red & ~gold
    return opaque, skin, red, gold, light


class HairModel:
    """Hair vs. clothing by colour, from smoothed histograms of known samples."""

    BIN = 8

    def __init__(self, hair_pixels, cloth_pixels):
        self.h = self._hist(hair_pixels)
        self.c = self._hist(cloth_pixels)

    def _hist(self, pixels):
        n = 256 // self.BIN
        hist = np.zeros((n, n, n), np.float64)
        q = (np.asarray(pixels)[:, :3] // self.BIN).astype(np.int64)
        np.add.at(hist, (q[:, 0], q[:, 1], q[:, 2]), 1)
        # Smooth over neighbouring bins so near colours share evidence.
        out = hist.copy()
        for axis in range(3):
            out = out + 0.4 * (np.roll(out, 1, axis) + np.roll(out, -1, axis))
        return (out + 0.05) / (out.sum() + 1e-9)

    def likelihood(self, img):
        q = (img[..., :3] // self.BIN).astype(np.int64)
        h = self.h[q[..., 0], q[..., 1], q[..., 2]]
        c = self.c[q[..., 0], q[..., 1], q[..., 2]]
        return h / (h + c)


def train_hair(images, uses, cuts=None):
    """Hair: the top of the idle frames (or, once fitted, every head zone); clothing: the
    south-facing frames below the collar, where the ponytail hangs out of sight."""
    hair, cloth = [], []
    for index, img in enumerate(images):
        opaque, skin, red, gold, light = classes(img)
        plain = opaque & ~skin & ~red & ~gold & ~light
        a, _, layer = uses[index][0]
        if cuts and index < len(cuts["images"]):
            x0, y0, x1, y1 = cuts["images"][index]["zone"]
            ax, ay = cuts["images"][index]["anchor"]
            ys, xs = np.mgrid[0:img.shape[0], 0:img.shape[1]]
            inside = (xs >= x0) & (xs < x1) & (ys >= y0) & (ys < ay - 2)
            hair.append(img[plain & inside])
            if a % 8 == 0:
                cloth.append(img[plain & (ys > ay + 6)])
        else:
            top = opaque.argmax(axis=0)
            if a < 8:
                hair.append(img[:22][plain[:22]])
            if a % 8 == 0:
                cloth.append(img[40:][plain[40:]])
    return HairModel(np.concatenate(hair), np.concatenate(cloth))


# ---------------------------------------------------------------- the reference head


class ReferenceHead:
    def __init__(self, folder):
        folder = Path(folder)
        spr = read_spr(folder / "2_남.spr")
        self.images = rgba_images(spr)
        self.actions = read_reference_act(folder / "2_남.act")
        self.palettes = {}
        for c in range(1, HAIR_COLOURS + 1):
            path = folder / f"머리2_남_{c}.pal"
            if path.exists():
                self.palettes[c] = path.read_bytes()[:1024]
        self.spr = spr

    def frame(self, action, frame):
        """(RGBA image, top-left offset from the anchor) for a head frame."""
        fr = self.actions[action]["frames"]
        fr = fr[frame % len(fr)]
        layer = next(l for l in fr["layers"] if l["sprite_index"] >= 0)
        img = self.images[layer["sprite_index"]]
        if layer["mirror"]:
            img = img[:, ::-1]
        anchor = fr["anchors"][0]
        h, w = img.shape[:2]
        return img, (layer["x"] - w / 2 - anchor["x"], layer["y"] - h / 2 - anchor["y"])

    def hair_ramps(self):
        """Per hair colour, the official hair shades from dark to light: the palette entries
        that differ between colours (skin and outline stay the same in every palette)."""
        pals = [np.frombuffer(self.palettes[c], np.uint8).reshape(256, 4)[:, :3].astype(int) for c in sorted(self.palettes)]
        varying = [i for i in range(1, 256) if any((p[i] != pals[0][i]).any() for p in pals[1:])]
        used = set(int(v) for img in self.spr["images"] if not img["sprite_type"] for v in img["pixels"])
        varying = [i for i in varying if i in used]
        ramps = {}
        for c, p in zip(sorted(self.palettes), pals):
            shades = sorted((tuple(int(v) for v in p[i]) for i in varying), key=luma)
            ramps[c] = shades
        return ramps


def luma(c):
    return 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]


# ---------------------------------------------------------------- fitting


def fit_anchor(img, model, head_img, offset, window=None):
    """The anchor that best lays the reference head over the drawn one. `window` (x0, y0,
    x1, y1), from a hand correction, bounds where the head's centre may go."""
    opaque, skin, red, gold, light = classes(img)
    hairness = model.likelihood(img)
    h, w = opaque.shape
    weight = np.where(opaque, 2.2 * hairness - 1.2, -0.5)
    weight = np.where(skin, 1.0, weight)
    weight = np.where(red | gold | light, -0.8, weight)
    hmask = head_img[..., 3] >= 128
    hskin = classes(head_img)[1]
    hh, hw = hmask.shape
    pad = 24
    wpad = np.full((h + 2 * pad, w + 2 * pad), -0.5)
    wpad[pad:pad + h, pad:pad + w] = weight
    spad = np.zeros_like(wpad)
    spad[pad:pad + h, pad:pad + w] = skin
    # Ribbon in the hair: red near the top of the head is not a penalty.
    rpad = np.zeros_like(wpad)
    rpad[pad:pad + h, pad:pad + w] = red
    ox, oy = offset
    best = None
    for ay in range(-pad // 2, h + pad // 2):
        for ax in range(-pad // 2, w + pad // 2):
            x0 = int(round(ax + ox)) + pad
            y0 = int(round(ay + oy)) + pad
            if x0 < 0 or y0 < 0 or x0 + hw > wpad.shape[1] or y0 + hh > wpad.shape[0]:
                continue
            if window:
                cx, cy = ax + ox + hw / 2, ay + oy + hh / 2
                if not (window[0] <= cx <= window[2] and window[1] <= cy <= window[3]):
                    continue
            win = wpad[y0:y0 + hh, x0:x0 + hw]
            score = float((win * hmask).sum())
            score += float((spad[y0:y0 + hh, x0:x0 + hw] * hskin).sum())
            top = rpad[y0:y0 + hh // 2, x0:x0 + hw] * hmask[:hh // 2]
            score += 1.0 * float(top.sum())
            if best is None or score > best[0]:
                best = (score, ax, ay)
    _, ax, ay = best
    zone = [int(math.floor(ax + ox)), int(math.floor(ay + oy)), int(math.floor(ax + ox)) + hw, int(math.floor(ay + oy)) + hh]
    return [ax, ay], zone, best[0]


def fit(compiled, reference, cuts_path):
    images, actions = load_body(compiled)
    uses = image_uses(actions)
    head = ReferenceHead(reference)
    previous = json.loads(cuts_path.read_text(encoding="utf-8")) if cuts_path.exists() else {}
    old = {i: e for i, e in enumerate(previous.get("images", []))}
    cuts = dict(schema_version=1, reference_head=r"data\sprite\인간족\머리통\남\2_남",
                hairstyle=HAIRSTYLE, images=[], hair_ramps={})
    model = train_hair(images, uses)
    for pass_ in range(2):
        cuts["images"] = []
        for index, img in enumerate(images):
            a, f, layer = uses[index][0]
            himg, offset = head.frame(a, f)
            kept = old.get(index, {})
            anchor, zone, score = fit_anchor(img, model, himg, offset, kept.get("centre_window"))
            entry = dict(anchor=anchor, zone=zone, source=f"action {a} frame {f}", score=round(score, 1), view=a % 8)
            # The fit matches the drawn head, which is bigger than a player's: lowered onto
            # the shoulders, a player's head sits as it does on official bodies.
            _, u, _ = neck_frame(entry, img[..., 3] >= 128)
            if abs(u[1]) >= abs(u[0]):
                k = FRONT_DROP if a % 8 in (0, 1, 2) else BACK_DROP
                entry["drop"] = [int(round(u[0] * k)), int(round(u[1] * k))]
            # Hand corrections survive refits.
            for key in ("centre_window", "anchor_override", "cut", "keep", "note"):
                if key in kept:
                    entry[key] = kept[key]
            cuts["images"].append(entry)
        if pass_ == 0:
            model = train_hair(images, uses, cuts)
    cuts["hair_ramps"] = {str(c): ramp for c, ramp in head.hair_ramps().items()}
    # Official idle and sit actions have three frames: straight, and the head turned right and
    # left. The body's anchor moves with the turned head's, so the head stays centred.
    cuts["turn_offsets"] = {}
    for a in list(range(0, 8)) + list(range(16, 24)):
        fr = head.actions[a]["frames"]
        base = fr[0]["anchors"][0]
        cuts["turn_offsets"][str(a)] = [[f["anchors"][0]["x"] - base["x"], f["anchors"][0]["y"] - base["y"]] for f in fr[1:3]]
    cuts_path.write_text(json.dumps(cuts, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return cuts


# How far a player's head is lowered from the drawn head's place, facing us and facing away
# (matched by eye against official bodies wearing the same head).
FRONT_DROP = 5
BACK_DROP = 4


def anchor_of(entry):
    """Where a player's head attaches: the hand correction, else the fit lowered by its drop."""
    if "anchor_override" in entry:
        return entry["anchor_override"]
    dx, dy = entry.get("drop", (0, 0))
    return [entry["anchor"][0] + dx, entry["anchor"][1] + dy]


# ---------------------------------------------------------------- cutting


def neck_frame(entry, opaque):
    """(zone centre, unit vector toward the body, the zone's half extent along it). The head's
    side of the neck is everything nearer the zone centre than that extent; this holds for
    lying poses too, where the body is beside the head rather than below it."""
    x0, y0, x1, y1 = entry["zone"]
    c = np.array([(x0 + x1) / 2, (y0 + y1) / 2])
    ys, xs = np.nonzero(opaque)
    pts = np.stack([xs, ys], 1).astype(float)
    far = np.hypot(pts[:, 0] - c[0], pts[:, 1] - c[1]) > max(x1 - x0, y1 - y0) * 0.75
    b = pts[far].mean(0) if far.any() else c + [0, 1]
    u = (b - c) / max(np.hypot(*(b - c)), 1e-6)
    extent = (abs(u[0]) * (x1 - x0) + abs(u[1]) * (y1 - y0)) / 2
    return c, u, extent


def cut_frame(img, entry, model):
    """(head mask, fill mask, mirrored mask, mirrored colours) for one frame: the drawn head
    and hair; the part of it that covered the coat (to fill) rather than hung free; and, facing
    us or away, the shoulder it covered, taken from the other shoulder."""
    opaque, skin, red, gold, light = classes(img)
    h, w = opaque.shape
    ys, xs = np.mgrid[0:h, 0:w]
    hairness = model.likelihood(img)
    c, u, extent = neck_frame(entry, opaque)
    s = (xs - c[0]) * u[0] + (ys - c[1]) * u[1]
    x0, y0, x1, y1 = entry["zone"]
    rx, ry = (x1 - x0) / 2 + 3, (y1 - y0) / 2 + 3
    ellipse = ((xs + 0.5 - c[0]) / rx) ** 2 + ((ys + 0.5 - c[1]) / ry) ** 2 <= 1
    # Averaged over each pixel's neighbourhood, so the dark strokes between strands don't
    # cut the ponytail off from the head.
    plain = opaque & ~skin & ~gold & ~light & ~red
    weight = sum(shift(plain, dy, dx).astype(np.float64) for dy in (-1, 0, 1) for dx in (-1, 0, 1))
    total = sum(shift(np.where(plain, hairness, 0), dy, dx) for dy in (-1, 0, 1) for dx in (-1, 0, 1))
    smooth = total / np.maximum(weight, 1)
    hairlike = plain & ((hairness > HAIR_T) | (smooth > HAIR_T))
    head_side = s < extent - 1
    # The ribbon tied in the hair is part of the hairstyle: red on the head's side, near the top.
    ribbon = red & (s < -extent * 0.2)
    head = opaque & ellipse & (head_side | hairlike) | (ellipse & ribbon)
    # Grow down the ponytail through hair-coloured pixels, never past the sash.
    reach = np.hypot(xs - c[0], ys - c[1]) < 44
    grow = (hairlike | ribbon) & reach & (s < extent + 34)
    frontier = head.copy()
    while True:
        new = (neighbours(frontier) > 0) & grow & ~head
        if not new.any():
            break
        head |= new
        frontier = new
    # Specks of coat left inside the hair go with it.
    for _ in range(2):
        head |= opaque & ~skin & ~red & (neighbours(head) >= 6)
    # Red below the chin is the collar, not the ribbon: it stays with the body (the head
    # drawn turned onto a neighbouring direction would otherwise carry it onto the chest).
    ax, ay = entry["anchor"]
    s_chin = (ax - c[0]) * u[0] + (ay - c[1]) * u[1]
    head &= ~(red & (s >= s_chin - 1))
    # The hair's dark outline (too dark to tell from the coat's) is left as thin strands and
    # islands once the hair is gone: strip strands touching the cut, then drop small pieces.
    for _ in range(8):
        body = opaque & ~head
        thin = body & (neighbours(head) > 0) & (neighbours(body) <= 3)
        if not thin.any():
            break
        head |= thin
    # Lines along where the hair lay (its edge strokes) go too: anything up to two pixels
    # wide beside the cut, which a 3x3 opening of the body removes.
    near = head.copy()
    for _ in range(2):
        near |= neighbours(near) > 0
    for _ in range(3):
        body = opaque & ~head
        eroded = body & (neighbours(body) == 8)
        opened = eroded | (body & (neighbours(eroded) > 0))
        strokes = body & ~opened & near & ~red & ~gold & ~skin
        if not strokes.any():
            break
        head |= strokes
    for _ in range(24):
        body = opaque & ~head
        line = body & near & ~red & ~gold & ~skin & (neighbours(body) <= 2)
        if not line.any():
            break
        head |= line
    # Strands and ribbon tails left standing up from the shoulders: on the head's side of the
    # neck, any run across of four pixels or fewer (not a hand) goes.
    above = s < extent + 2
    for _ in range(12):
        body = opaque & ~head
        runs = run_widths(body if abs(u[1]) >= abs(u[0]) else body.T)
        if abs(u[1]) < abs(u[0]):
            runs = runs.T
        thin = body & above & ((~skin & (runs <= 4)) | (skin & (runs <= 2)))
        if not thin.any():
            break
        head |= thin
    body = opaque & ~head
    parts = components(body)
    if parts:
        main = max(parts, key=len)
        near_cut = neighbours(head) > 0
        for part in parts:
            if part is main:
                continue
            ys_, xs_ = np.array(part).T
            # Specks the hair's outline left anywhere (not a hand).
            if len(part) < 10 and skin[ys_, xs_].mean() < 0.5:
                head[ys_, xs_] = True
                continue
            if not any(near_cut[y, x] for y, x in part):
                continue
            # Small pieces, and larger ones that are hair-coloured rather than a hand.
            if len(part) < 40 or (hairness[ys_, xs_].mean() > 0.5 and skin[ys_, xs_].mean() < 0.15):
                head[ys_, xs_] = True
    for rect in entry.get("cut", []):
        rx0, ry0, rx1, ry1 = rect
        head[max(ry0, 0):ry1, max(rx0, 0):rx1] |= opaque[max(ry0, 0):ry1, max(rx0, 0):rx1]
    for rect in entry.get("keep", []):
        rx0, ry0, rx1, ry1 = rect
        head[max(ry0, 0):ry1, max(rx0, 0):rx1] = False
    # Fill what lay over the coat: on the body's side of the neck, where coat shows on both
    # sides across the body (rows when upright, columns when lying) within a short gap. Hair
    # that hung free of the body has background on one side and goes.
    body = opaque & ~head
    fill = np.zeros_like(head)
    upright = abs(u[1]) >= abs(u[0])
    spans = body if upright else body.T
    target = (head & (s >= extent - 1)) if upright else (head & (s >= extent - 1)).T
    for i in range(spans.shape[0]):
        cols = np.nonzero(spans[i])[0]
        for j in np.nonzero(target[i])[0]:
            left = cols[cols < j]
            right = cols[cols > j]
            if len(left) and len(right) and right.min() - left.max() <= GAP:
                if upright:
                    fill[i, j] = True
                else:
                    fill[j, i] = True
    top = int(round(ay)) - 3
    if upright:
        # The shoulders' outline: the convex hull of the body left around them. Fills near the
        # shoulders stay inside it, so the back slopes down from the neck as a back does.
        band = body & (ys >= top) & (ys <= top + 24)
        shoulders = inside_hull(convex_hull(np.stack(np.nonzero(band)[::-1], 1)), xs, ys) if band.sum() >= 3 else np.ones_like(head)
        shoulders |= ys > top + 24
        # Hair down the back with the coat straight below it lay over the coat: fill that
        # column down to the coat (long hair can hide the back's edges entirely). Only under
        # the head: hair hanging beside it, behind a shoulder, covered nothing.
        down = 1 if u[1] > 0 else -1
        under = (xs >= x0 - 2) & (xs < x1 + 2) & shoulders
        for y, x in zip(*np.nonzero(head & (s >= extent - 1) & ~fill & under)):
            for k in range(1, 25):
                yy = y + down * k
                if not 0 <= yy < h or not opaque[yy, x]:
                    break
                if body[yy, x]:
                    fill[y, x] = True
                    break
    mirrored = np.zeros_like(head)
    mirror_rgb = np.zeros(img.shape[:2] + (3,), np.uint8)
    if upright:
        view = entry.get("view", 0)
        band = body & (ys >= top) & (ys <= top + 24)
        if view in (0, 4) and band.any():
            # Facing us or away the body is near enough symmetric: a shoulder the hair covered
            # is the other one mirrored, about the middle of the shoulders.
            mids = [(xs_r.min() + xs_r.max()) / 2 for xs_r in (np.nonzero(band[y])[0] for y in range(h)) if len(xs_r)]
            axis = float(np.median(mids))
            for y, x in zip(*np.nonzero(head & opaque & (ys >= top) & ~fill)):
                mx = int(round(2 * axis - x))
                if 0 <= mx < w and body[y, mx]:
                    mirrored[y, x] = True
                    mirror_rgb[y, x] = img[y, mx, :3]
        elif band.sum() >= 3:
            # From the side, the shoulders under the hair are the outline the rest of them
            # makes: hair inside the shoulders' convex hull covered coat.
            pts = np.stack(np.nonzero(band)[::-1], 1)
            hull = convex_hull(pts)
            cand = head & opaque & (ys >= top) & (ys <= top + 24) & ~fill
            fill |= cand & inside_hull(hull, xs, ys)
        # Filled straight across, the back under the hair would square off the shoulders. A
        # back slopes from the neck to the shoulders: keep fills inside that slope.
        neck = y1 - 2
        reach = NECK_HALF + SHOULDER_SLOPE * np.maximum(ys - neck, 0)
        fill &= (ys >= neck) & (np.abs(xs + 0.5 - c[0]) <= reach)
    return head, fill, mirrored, mirror_rgb


def convex_hull(points):
    """Monotone chain: the hull's corners anticlockwise (x right, y down), from (x, y) points."""
    pts = sorted(set(map(tuple, points.tolist())))
    if len(pts) < 3:
        return pts
    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def inside_hull(hull, xs, ys):
    """Pixels inside (or on) a convex polygon."""
    if len(hull) < 3:
        return np.zeros(xs.shape, bool)
    inside = np.ones(xs.shape, bool)
    for (ax, ay), (bx, by) in zip(hull, hull[1:] + hull[:1]):
        inside &= (bx - ax) * (ys - ay) - (by - ay) * (xs - ax) >= -0.5
    return inside


# The widest hole across the coat that a fill bridges: the back under the ponytail.
GAP = 28
# The neck's half width and how fast the shoulders widen below it (px per row), for fills on
# half-turned views.
NECK_HALF = 5
SHOULDER_SLOPE = 1.5
# Hair likelihood above which a pixel joins the hair as it grows from the head.
HAIR_T = 0.55


def shift(mask, dy, dx):
    out = np.zeros_like(mask)
    h, w = mask.shape
    out[max(dy, 0):h + min(dy, 0), max(dx, 0):w + min(dx, 0)] = mask[max(-dy, 0):h + min(-dy, 0), max(-dx, 0):w + min(-dx, 0)]
    return out


def run_widths(mask):
    """For each set pixel, the length of the horizontal run it belongs to (0 elsewhere)."""
    out = np.zeros(mask.shape, np.int32)
    for y in range(mask.shape[0]):
        row = mask[y]
        x = 0
        while x < len(row):
            if row[x]:
                end = x
                while end < len(row) and row[end]:
                    end += 1
                out[y, x:end] = end - x
                x = end
            else:
                x += 1
    return out


def neighbours(mask):
    return sum(shift(mask, dy, dx).astype(np.int32) for dy in (-1, 0, 1) for dx in (-1, 0, 1) if dy or dx)


def components(mask):
    """8-connected components of a mask, as lists of (y, x)."""
    seen = np.zeros_like(mask)
    h, w = mask.shape
    parts = []
    for y0, x0 in zip(*np.nonzero(mask)):
        if seen[y0, x0]:
            continue
        stack, part = [(y0, x0)], []
        seen[y0, x0] = True
        while stack:
            y, x = stack.pop()
            part.append((y, x))
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    ny, nx = y + dy, x + dx
                    if 0 <= ny < h and 0 <= nx < w and mask[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        stack.append((ny, nx))
        parts.append(part)
    return parts


def diffuse(img, known, todo):
    """Fill `todo` pixels smoothly from the plain coat around them: a first guess spread in
    from the edge, then relaxed (each pixel the average of its neighbours) so it settles into
    gentle gradients. Piping, knots, gold and skin give it no colour, so they don't streak
    across the patch."""
    out = img[..., :3].astype(np.float64)
    opaque, skin, red, gold, light = classes(img)
    source = known & ~skin & ~red & ~gold & ~light
    if not (source & (neighbours(todo) > 0)).any():
        source = known
    have = source.copy()
    left = todo.copy()
    steps = ((1, 0), (-1, 0), (0, 1), (0, -1))
    while left.any():
        acc = np.zeros(out.shape, np.float64)
        cnt = np.zeros(out.shape[:2], np.float64)
        for dy, dx in steps:
            m = shift(have, dy, dx)
            acc += np.stack([shift(out[..., c], dy, dx) for c in range(3)], -1) * m[..., None]
            cnt += m
        ready = left & (cnt > 0)
        if not ready.any():
            break
        out[ready] = acc[ready] / cnt[ready][:, None]
        have |= ready
        left &= ~ready
    region = todo & have
    for _ in range(80):
        acc = np.zeros(out.shape, np.float64)
        cnt = np.zeros(out.shape[:2], np.float64)
        for dy, dx in steps:
            m = shift(have, dy, dx)
            acc += np.stack([shift(out[..., c], dy, dx) for c in range(3)], -1) * m[..., None]
            cnt += m
        upd = region & (cnt > 0)
        out[upd] = acc[upd] / cnt[upd][:, None]
    result = img.copy()
    result[todo, :3] = out[todo].round().clip(0, 255).astype(np.uint8)
    result[todo, 3] = 255
    return result


def split(images, cuts, model):
    """(bodies, heads, head boxes): each frame without its head, and the head on its own."""
    bodies, heads, boxes = [], [], []
    for index, img in enumerate(images):
        entry = cuts["images"][index]
        head, fill, mirrored, mirror_rgb = cut_frame(img, entry, model)
        opaque = img[..., 3] >= 128
        body = img.copy()
        body[~opaque] = 0
        body[head] = 0
        body[mirrored, :3] = mirror_rgb[mirrored]
        body[mirrored, 3] = 255
        body = diffuse(body, (opaque & ~head) | mirrored, fill & ~mirrored)
        body[..., 3] = np.where(body[..., 3] >= 128, 255, 0)
        heads_img = np.zeros_like(img)
        heads_img[head & opaque] = img[head & opaque]
        heads_img[..., 3] = np.where(head & opaque, 255, 0)
        ys, xs = np.nonzero(head & opaque)
        if len(xs):
            hx0, hy0, hx1, hy1 = xs.min(), ys.min(), xs.max() + 1, ys.max() + 1
        else:
            hx0, hy0, hx1, hy1 = 0, 0, 2, 2
        # Even sizes keep ACT centre offsets whole.
        hx1 += (hx1 - hx0) % 2
        hy1 += (hy1 - hy0) % 2
        crop = np.zeros((hy1 - hy0, hx1 - hx0, 4), np.uint8)
        src = heads_img[hy0:min(hy1, img.shape[0]), hx0:min(hx1, img.shape[1])]
        crop[:src.shape[0], :src.shape[1]] = src
        bodies.append(body)
        heads.append(crop)
        boxes.append((int(hx0), int(hy0)))
    return bodies, heads, boxes


# ---------------------------------------------------------------- palettes


def kmeans(colors, weights, k, iters=12):
    colors = np.asarray(colors, np.float64)
    weights = np.asarray(weights, np.float64)
    if len(colors) <= k:
        return colors
    order = np.argsort(colors @ [0.299, 0.587, 0.114])
    cum = np.cumsum(weights[order]) / weights.sum()
    centres = colors[order[np.searchsorted(cum, (np.arange(k) + 0.5) / k)]]
    for _ in range(iters):
        d = ((colors[:, None, :] - centres[None]) ** 2).sum(-1)
        label = d.argmin(1)
        for j in range(k):
            m = label == j
            if m.any():
                centres[j] = (colors[m] * weights[m, None]).sum(0) / weights[m].sum()
    return centres


def build_palette(images, material_of, layout):
    """A shared 256-colour palette: index 0 transparent, then one range per material
    (dark to light), each fitted to that material's colours across every image."""
    palette = np.zeros((256, 3), np.float64)
    palette[0] = (255, 0, 255)
    for name, (start, count) in layout.items():
        colors, weights = {}, {}
        for img in images:
            mats = material_of(img)
            sel = (img[..., 3] >= 128) & (mats == name)
            for c in map(tuple, img[sel][:, :3]):
                weights[c] = weights.get(c, 0) + 1
        if not weights:
            palette[start:start + count] = (128, 128, 128)
            continue
        keys = list(weights)
        centres = kmeans(keys, [weights[k] for k in keys], count)
        centres = sorted(map(tuple, centres), key=luma)
        for j in range(count):
            palette[start + j] = centres[min(j, len(centres) - 1)]
    return palette.round().clip(0, 255).astype(np.uint8)


def index_image(img, palette, material_of, layout):
    mats = material_of(img)
    out = np.zeros(img.shape[:2], np.uint8)
    for name, (start, count) in layout.items():
        sel = (img[..., 3] >= 128) & (mats == name)
        if not sel.any():
            continue
        pal = palette[start:start + count].astype(np.int32)
        px = img[sel][:, :3].astype(np.int32)
        d = ((px[:, None, :] - pal[None]) ** 2).sum(-1)
        out[sel] = start + d.argmin(1)
    return out


BODY_LAYOUT = {"skin": (1, 20), "red": (21, 72), "charcoal": (93, 80), "warm": (173, 56), "gold": (229, 16), "light": (245, 11)}
HEAD_LAYOUT = {"skin": (1, 24), "hair": (25, 152), "red": (177, 40), "light": (217, 24), "gold": (241, 15)}


def body_material(img):
    opaque, skin, red, gold, light = classes(img)
    r, g, b = (img[..., i].astype(np.int32) for i in range(3))
    warm = (r - b > 6) & (r >= g)
    mats = np.full(img.shape[:2], "", dtype=object)
    mats[opaque] = np.where(warm[opaque], "warm", "charcoal")
    mats[light] = "light"
    mats[gold] = "gold"
    mats[red] = "red"
    mats[skin] = "skin"
    return mats


def head_material(img):
    opaque, skin, red, gold, light = classes(img)
    mats = np.full(img.shape[:2], "", dtype=object)
    mats[opaque] = "hair"
    mats[light] = "light"
    mats[gold] = "gold"
    mats[red] = "red"
    mats[skin] = "skin"
    return mats


def recolour(palette, layout, rules):
    """A dye: per material range, a function of (h, l, s) -> (h, l, s)."""
    out = palette.copy()
    for name, fn in rules.items():
        start, count = layout[name]
        for i in range(start, start + count):
            r, g, b = (palette[i] / 255).tolist()
            h, l, s = colorsys.rgb_to_hls(r, g, b)
            h, l, s = fn(h, l, s)
            out[i] = np.array(colorsys.hls_to_rgb(h % 1, min(max(l, 0), 1), min(max(s, 0), 1))) * 255
    return out.round().clip(0, 255).astype(np.uint8)


# Clothes dyes 1-7: the red coat, piping and sash take a new colour; two also change the
# charcoal kimono. Skin, leather, gold and highlights keep theirs.
DYE_RULES = [
    {"red": lambda h, l, s: (0.58, l, s)},                                   # 1 azure
    {"red": lambda h, l, s: (0.40, l, s * 0.85)},                            # 2 jade
    {"red": lambda h, l, s: (0.76, l, s)},                                   # 3 violet
    {"red": lambda h, l, s: (0.12, min(l * 1.1, 0.9), s * 0.9)},             # 4 gold
    {"charcoal": lambda h, l, s: (h, 0.5 + 0.45 * l, s * 0.3)},              # 5 white kimono
    {"red": lambda h, l, s: (0.62, l * 0.7, s * 0.35),
     "charcoal": lambda h, l, s: (0.62, l * 0.8, s)},                        # 6 midnight
    {"red": lambda h, l, s: (0.93, min(l * 0.9 + 0.12, 0.92), s * 0.8)},     # 7 sakura
]


def hair_palette(palette, ramp):
    """Hair colour: the hairstyle's hair shades, dark to light, take the official ramp's
    colours at the same rank. Near-black outline shades stay."""
    out = palette.copy()
    start, count = HEAD_LAYOUT["hair"]
    ramp = np.array(ramp, np.float64)
    order = sorted(range(start, start + count), key=lambda i: luma(palette[i]))
    shades = [i for i in order if luma(palette[i]) >= 20]
    for rank, i in enumerate(shades):
        t = rank / max(len(shades) - 1, 1) * (len(ramp) - 1)
        lo = int(math.floor(t))
        hi = min(lo + 1, len(ramp) - 1)
        out[i] = ramp[lo] + (ramp[hi] - ramp[lo]) * (t - lo)
    return out.round().clip(0, 255).astype(np.uint8)


def pal_bytes(palette):
    return bytes(b for c in palette for b in (int(c[0]), int(c[1]), int(c[2]), 0))


# ---------------------------------------------------------------- writing


def body_anchor(layer, size, anchor):
    """An image-space point in actor space, through a body layer (mirrored or not)."""
    w, h = size
    ax, ay = anchor
    dx = ax - w / 2
    x = layer["x"] - dx if layer.get("mirror") else layer["x"] + dx
    return int(round(x)), int(round(layer["y"] + ay - h / 2))


def build(compiled, out, reference=None):
    compiled, out = Path(compiled), Path(out)
    cuts = json.loads(CUTS.read_text(encoding="utf-8"))
    images, actions = load_body(compiled)
    uses = image_uses(actions)
    model = train_hair(images, uses, cuts)
    bodies, heads, boxes = split(images, cuts, model)

    body_pal = build_palette(bodies, body_material, BODY_LAYOUT)
    head_pal = build_palette(heads, head_material, HEAD_LAYOUT)
    body_idx = [index_image(b, body_pal, body_material, BODY_LAYOUT) for b in bodies]
    head_idx = [index_image(hd, head_pal, head_material, HEAD_LAYOUT) for hd in heads]

    def indexed(arrs):
        return [dict(width=a.shape[1], height=a.shape[0], sprite_type=0, pixels=a.tobytes()) for a in arrs]

    # Body ACT: indexed layers and a head anchor per frame.
    body_actions = []
    anchors = []
    for action in actions:
        frames = []
        frame_anchors = []
        for frame in action["frames"]:
            layers = [dict(l, sprite_type=0) for l in frame["layers"]]
            i = frame["layers"][0]["sprite_index"]
            size = (images[i].shape[1], images[i].shape[0])
            anchor = body_anchor(frame["layers"][0], size, anchor_of(cuts["images"][i]))
            frames.append(dict(layers=layers, anchors=[dict(x=anchor[0], y=anchor[1])]))
            frame_anchors.append(anchor)
        turns = cuts.get("turn_offsets", {}).get(str(len(body_actions)))
        if turns and len(frames) == 1:
            # Idle and sitting: the same pose again with the head turned right and left.
            for dx, dy in turns:
                ax, ay = frame_anchors[0]
                frames.append(dict(layers=frames[0]["layers"], anchors=[dict(x=ax + dx, y=ay + dy)]))
                frame_anchors.append((ax + dx, ay + dy))
        body_actions.append(dict(interval=action["interval"], frames=frames))
        anchors.append(frame_anchors)

    def head_layer(i, mirror, base_layer, at_anchor):
        """The head image of body image i, placed so its anchor lands on `at_anchor`."""
        img = images[i]
        hx, hy = boxes[i]
        hh, hw = heads[i].shape[:2]
        centre = (hx + hw / 2, hy + hh / 2)
        own = body_anchor(base_layer, (img.shape[1], img.shape[0]), anchor_of(cuts["images"][i]))
        at = body_anchor(base_layer, (img.shape[1], img.shape[0]), centre)
        return dict(sprite_index=i, sprite_type=0, width=hw, height=hh, mirror=mirror,
                    x=at[0] - own[0] + at_anchor[0], y=at[1] - own[1] + at_anchor[1])

    head_actions = []
    for a, action in enumerate(actions):
        group, d = divmod(a, 8)
        frames = []
        for f, frame in enumerate(action["frames"]):
            layer = frame["layers"][0]
            anchor = anchors[a][f]
            frames.append(dict(layers=[head_layer(layer["sprite_index"], layer.get("mirror", False), layer, anchor)],
                               anchors=[dict(x=anchor[0], y=anchor[1])]))
        if group in (0, 2):
            # Idle and sitting: frames 1 and 2 turn the head right and left, as on official
            # heads: the head drawn for the next direction round either way, on the neck
            # (frame 0's anchor), its frame anchored with the body's turned frame.
            for k, turn in enumerate(((d + 1) % 8, (d - 1) % 8), 1):
                nb = actions[group * 8 + turn]["frames"][0]["layers"][0]
                neck = anchors[a][0]
                at = anchors[a][k] if k < len(anchors[a]) else neck
                frames.append(dict(layers=[head_layer(nb["sprite_index"], nb.get("mirror", False), nb, neck)],
                                   anchors=[dict(x=at[0], y=at[1])]))
        head_actions.append(dict(interval=action["interval"], frames=frames))

    out.mkdir(parents=True, exist_ok=True)
    write_spr(out / "ember_ronin.spr", indexed(body_idx), pal_bytes(body_pal))
    write_act(out / "ember_ronin.act", body_actions)
    for sex in ("남", "여"):
        write_spr(out / f"{HAIRSTYLE}_{sex}.spr", indexed(head_idx), pal_bytes(head_pal))
        write_act(out / f"{HAIRSTYLE}_{sex}.act", head_actions)
    (out / "palette" / "몸").mkdir(parents=True, exist_ok=True)
    (out / "palette" / "머리").mkdir(parents=True, exist_ok=True)
    for n, rules in enumerate(DYE_RULES, 1):
        (out / "palette" / "몸" / f"samurai_남_{n}.pal").write_bytes(pal_bytes(recolour(body_pal, BODY_LAYOUT, rules)))
    ramps = cuts.get("hair_ramps", {})
    for sex in ("남", "여"):
        (out / "palette" / "머리" / f"머리{HAIRSTYLE}_{sex}_0.pal").write_bytes(pal_bytes(head_pal))
        for c in range(1, HAIR_COLOURS + 1):
            ramp = ramps.get(str(c))
            pal = hair_palette(head_pal, ramp) if ramp else head_pal
            (out / "palette" / "머리" / f"머리{HAIRSTYLE}_{sex}_{c}.pal").write_bytes(pal_bytes(pal))
    review(out, images, bodies, heads, boxes, cuts, uses, body_pal, head_pal, reference)
    return dict(body_images=len(body_idx), head_images=len(head_idx))


def review(out, images, bodies, heads, boxes, cuts, uses, body_pal, head_pal, reference):
    """Per frame: the original; the new body alone; the body with the hairstyle (which
    should match the original); the body with an official head, when one is given."""
    ref = ReferenceHead(reference) if reference else None
    cols, cw, ch = 4 if ref else 3, 120, 128
    per_row = 6
    rows = math.ceil(len(images) / per_row)
    sheet = Image.new("RGBA", (per_row * cols * cw // 1, rows * ch), (38, 48, 60, 255))
    draw = ImageDraw.Draw(sheet)
    for index, img in enumerate(images):
        x = (index % per_row) * cols * cw
        y = (index // per_row) * ch
        bx, by = (cw - img.shape[1]) // 2, 14
        sheet.alpha_composite(Image.fromarray(img), (x + bx, y + by))
        sheet.alpha_composite(Image.fromarray(bodies[index]), (x + cw + bx, y + by))
        sheet.alpha_composite(Image.fromarray(bodies[index]), (x + 2 * cw + bx, y + by))
        hx, hy = boxes[index]
        sheet.alpha_composite(Image.fromarray(heads[index]), (x + 2 * cw + bx + hx, y + by + hy))
        if ref:
            ax, ay = anchor_of(cuts["images"][index])
            a, f, _ = uses[index][0]
            himg, (ox, oy) = ref.frame(a, f)
            sheet.alpha_composite(Image.fromarray(bodies[index]), (x + 3 * cw + bx, y + by))
            sheet.alpha_composite(Image.fromarray(himg), (x + 3 * cw + bx + int(round(ax + ox)), y + by + int(round(ay + oy))))
        draw.text((x + 2, y + 1), str(index), fill=(230, 230, 230, 255))
    sheet.save(out / "review_frames.png")


# ---------------------------------------------------------------- review


def fit_review(compiled, reference, cuts, path, scale=3):
    images, actions = load_body(compiled)
    uses = image_uses(actions)
    head = ReferenceHead(reference)
    cols = 12
    cw, ch = 120, 130
    rows = math.ceil(len(images) / cols)
    sheet = Image.new("RGBA", (cols * cw, rows * ch), (38, 48, 60, 255))
    draw = ImageDraw.Draw(sheet)
    for index, img in enumerate(images):
        entry = cuts["images"][index]
        ax, ay = anchor_of(entry)
        a, f, _ = uses[index][0]
        himg, (ox, oy) = head.frame(a, f)
        x, y = (index % cols) * cw, (index // cols) * ch
        bx, by = x + (cw - img.shape[1]) // 2, y + 14
        sheet.alpha_composite(Image.fromarray(img), (bx, by))
        ghost = himg.copy()
        ghost[..., 3] = (ghost[..., 3] * 0.55).astype(np.uint8)
        sheet.alpha_composite(Image.fromarray(ghost), (bx + int(round(ax + ox)), by + int(round(ay + oy))))
        draw.point((bx + ax, by + ay), fill=(0, 255, 0, 255))
        draw.text((x + 2, y + 1), f"{index} {DIRECTIONS[a % 8]} g{a // 8}", fill=(230, 230, 230, 255))
    sheet = sheet.resize((sheet.width * scale // 2, sheet.height * scale // 2), Image.Resampling.NEAREST)
    sheet.save(path)


# ---------------------------------------------------------------- installing


def install(built, overlay):
    """Copies a build into the overlay (data\\ paths as the client reads them) and pads every
    weapon ACT there to the body's frame counts: idle and sit now have three frames (the head
    turns), and each weapon timeline must pair with the body's."""
    built, overlay = Path(built), Path(overlay)
    samurai = overlay / "data" / "sprite" / "custom" / "samurai"
    copies = {built / "ember_ronin.spr": samurai / "ember_ronin.spr", built / "ember_ronin.act": samurai / "ember_ronin.act"}
    for sex in ("남", "여"):
        for ext in ("spr", "act"):
            copies[built / f"{HAIRSTYLE}_{sex}.{ext}"] = overlay / "data" / "sprite" / "인간족" / "머리통" / sex / f"{HAIRSTYLE}_{sex}.{ext}"
    for pal in sorted((built / "palette").rglob("*.pal")):
        copies[pal] = overlay / "data" / pal.relative_to(built)
    for source, target in copies.items():
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(source.read_bytes())
    body = read_act(samurai / "ember_ronin.act")["actions"]
    padded = []
    for path in sorted(samurai.glob("*.act")):
        if path.stem == "ember_ronin":
            continue
        act = read_act(path)
        changed = False
        for action, want in zip(act["actions"], body):
            while len(action["frames"]) < len(want["frames"]):
                action["frames"].append(dict(action["frames"][-1]))
                changed = True
        if changed:
            write_act(path, act["actions"], act["sounds"])
            padded.append(path.name)
    return dict(copied=len(copies), padded=padded)


# ---------------------------------------------------------------- CLI


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    f = sub.add_parser("fit")
    f.add_argument("compiled", type=Path)
    f.add_argument("--reference-head", type=Path, required=True)
    f.add_argument("--review", type=Path)
    b = sub.add_parser("build")
    b.add_argument("compiled", type=Path)
    b.add_argument("--output", type=Path, required=True)
    b.add_argument("--reference-head", type=Path)
    i = sub.add_parser("install")
    i.add_argument("built", type=Path)
    i.add_argument("--overlay", type=Path, default=HERE.parents[1] / "overlays" / "samurai")
    args = parser.parse_args()
    if args.command == "fit":
        cuts = fit(args.compiled, args.reference_head, CUTS)
        if args.review:
            fit_review(args.compiled, args.reference_head, cuts, args.review)
        print(f"fitted {len(cuts['images'])} frames into {CUTS}")
    elif args.command == "build":
        print(json.dumps(build(args.compiled, args.output, args.reference_head)))
    else:
        print(json.dumps(install(args.built, args.overlay), ensure_ascii=False))


if __name__ == "__main__":
    main()
