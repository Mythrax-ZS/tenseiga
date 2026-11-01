"""The head is cut at the artwork's own size and the cut carried to the official-size body."""
import json
from pathlib import Path
import tempfile
import unittest

import numpy as np
from PIL import Image

from compile import compile_package
from separate_head import HairModel, cell_grids, load_body, regrid, rescale_body


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


if __name__ == "__main__":
    unittest.main()
