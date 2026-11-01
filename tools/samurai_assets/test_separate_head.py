"""The head is cut at the artwork's own size and the cut carried to the official-size body."""
import json
from pathlib import Path
import tempfile
import unittest

import numpy as np
from PIL import Image

from compile import compile_package
from separate_head import (NECK_GAP, NECK_REACH, WALK_BOB, HairModel, cell_grids, load_body, neck_stub, regrid,
                           rescale_body, steady_walk, upper_body_from)


class RescaleTest(unittest.TestCase):
    def test_the_native_cut_carries_to_the_official_size(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            # One 8x8 cell per direction: a 4x2 "head" over a 4x4 "body", feet on the origin.
            source = Image.new("RGBA", (8, 40))
            for row in range(5):
                for x in range(2, 6):
                    for y in range(1, 3):
                        source.putpixel((x, row * 8 + y), (40, 30, 20, 255))
                    for y in range(3, 7):
                        source.putpixel((x, row * 8 + y), (90, 90, 100, 255))
            source.save(root / "body.png")
            groups = [dict(index=g, sheet="all", columns=[0], interval_ms=100) for g in range(13)]
            config = dict(schema_version=1, downsample=1, body_scale=[2, 3], ground_drop=4,
                          mirror_directions={"NE": "NW", "E": "W", "SE": "SW"},
                          sheets={"all": dict(path="body.png", cell_size=[8, 8], rows=["S", "SW", "W", "NW", "N"], origin=[4, 7])},
                          groups=groups)
            path = root / "build.json"
            path.write_text(json.dumps(config))
            compile_package(path, root / "native", native=True)
            compile_package(path, root / "sized")
            native, nacts = load_body(root / "native")
            sized, sacts = load_body(root / "sized")
            i, j = (acts[0]["frames"][0]["layers"][0]["sprite_index"] for acts in (nacts, sacts))
            gn, gs = cell_grids(root / "native")[i], cell_grids(root / "sized")[j]
            self.assertEqual((native[i].shape[:2], sized[j].shape[:2]), ((6, 4), (18, 8)))
            # A native pixel's corner and centre land on its 2x3 block at the official size.
            self.assertEqual(tuple(map(float, regrid((1, 2), gn, gs))), (2.0, 6.0))
            self.assertEqual(tuple(map(float, regrid((1.5, 2.5), gn, gs))), (3.0, 7.5))
            # The cut: the head's two rows go, one body pixel was painted over (say, under hair).
            removed = np.zeros(native[i].shape[:2], bool)
            removed[:2] = True
            painted = np.zeros_like(removed)
            painted[2, 0] = True
            body = native[i].copy()
            body[2, 0, :3] = (200, 0, 0)
            model = HairModel(np.array([[40, 30, 20, 255]]), np.array([[90, 90, 100, 255]]))
            out = rescale_body(sized[j], gs, body, gn, removed, painted, model)
            self.assertFalse(out[:6, :, 3].any())
            self.assertTrue((out[6:9, 0:2, :3] == (200, 0, 0)).all())
            self.assertTrue((out[6:, 2:, :3] == (90, 90, 100)).all() and out[6:, :, 3].all())


def opaque(h, w):
    img = np.zeros((h, w, 4), np.uint8)
    img[..., 3] = 255
    return img


class WalkTest(unittest.TestCase):
    def test_walking_holds_the_torso_on_one_line_and_bobs_it(self):
        rng = np.random.default_rng(7)
        torso = rng.integers(20, 236, (14, 20, 3), np.uint8)
        images, necks, acts = [], [], [dict(frames=[]) for _ in range(8)]
        for d in range(5):
            frames = []
            for f in range(8):
                dx, dy = (int(v) for v in rng.integers(-2, 3, 2))
                img = np.zeros((60, 40, 4), np.uint8)
                img[10 + dy:24 + dy, 10 + dx:30 + dx, :3] = torso
                img[10 + dy:24 + dy, 10 + dx:30 + dx, 3] = 255
                img[44:58, 12:28] = (60, 60, 60, 255)
                frames.append(dict(layers=[dict(sprite_index=len(images), x=0, y=-30, width=40, height=60)]))
                images.append(img)
                necks.append((20.0 + dx, 10.0 + dy))
            acts.append(dict(frames=frames))
        acts, images, necks = steady_walk(acts, images, necks)
        for d in range(5):
            spots = []
            for f, frame in enumerate(acts[8 + d]["frames"]):
                layer = frame["layers"][0]
                img = images[layer["sprite_index"]]
                h, w = img.shape[:2]
                ys, xs = np.nonzero((img[..., :3] == torso[0, 0]).all(-1) & (img[..., 3] > 0))
                top, left = layer["y"] - h // 2, layer["x"] - w // 2
                feet = np.nonzero(img[..., 3].any(1))[0].max()
                spots.append((left + xs.min(), top + ys.min() - WALK_BOB[f], top + feet))
                # The neck rides the torso: still at the middle of its top edge.
                self.assertEqual(necks[layer["sprite_index"]], (xs.min() + 10, ys.min()))
            self.assertEqual(len(set(spots)), 1, spots)
            self.assertEqual(spots[0][2], -3)

    def test_a_frame_takes_its_neighbours_upper_body_sash_on_sash(self):
        dst, src = opaque(30, 20), opaque(34, 24)
        dst[:5, ..., 3] = src[:4, ..., 3] = 0
        dst[..., :3], src[..., :3] = (70, 70, 70), (40, 40, 120)
        dst[15:18, :, :3] = src[18:21, :, :3] = (200, 20, 20)
        out, (cx, cy), neck = upper_body_from(dst, (10, 5), src, (12, 4))
        h, w = out.shape[:2]
        # Actor space, dst drawn centred on the origin: its pixel (x, y) sat at (x - 10, y - 15).
        actor = lambda x, y: (x - w / 2 + cx, y - h / 2 + cy)
        # src moves 2 left (centre on centre) and 3 up, so its upper body ends on dst's sash.
        ys, xs = np.nonzero((out[..., :3] == (40, 40, 120)).all(-1) & (out[..., 3] > 0))
        self.assertEqual(actor(xs.min(), ys.max() + 1), (0 - 2 - 10, 15 - 15))
        self.assertEqual(actor(*neck), (12 - 2 - 10, 4 - 3 - 15))
        # Below the sash it is still dst, the feet where they were.
        self.assertEqual(actor(0, np.nonzero(out[..., 3].any(1))[0].max() + 1)[1], 30 - 15)

    def test_the_neck_bridges_the_cut_only_where_the_body_is_close(self):
        colours = (np.array((230, 180, 150), np.uint8), np.array((120, 90, 80), np.uint8))
        img = np.zeros((40, 20, 4), np.uint8)
        img[20 + NECK_GAP:, :10] = img[20 + NECK_GAP + 1:, 10:] = (70, 70, 70, 255)
        out = neck_stub(img, (10.0, 20.0), colours)
        added = (out[..., 3] > 0) & (img[..., 3] == 0)
        # Left of the neck the body starts close below it: the neck reaches NECK_REACH up past it.
        self.assertTrue(added[20 - NECK_REACH:20 + NECK_GAP, 6:10].all())
        self.assertFalse(added[:20 - NECK_REACH].any())
        self.assertTrue((out[18, 7, :3] == colours[0]).all() and (out[18, 6, :3] == colours[1]).all())
        # Right of it the body starts further down (a bowed head): nothing hangs there.
        self.assertFalse(added[:, 10:].any())


if __name__ == "__main__":
    unittest.main()
