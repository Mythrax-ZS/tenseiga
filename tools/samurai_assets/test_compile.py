"""Regression fixtures for placement, transparency and paired RO timelines."""
import hashlib
import json
import math
from pathlib import Path
import tempfile
import unittest

from PIL import Image

from compile import compile_package, preview_layout, reflect_layer, sampled_cell, sampled_weapon_png, validate_package, weapon_layer
from ro_formats import FormatError, read_act, read_spr, write_act, write_spr


class CompilerTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_trimming_does_not_follow_advancing_foot(self):
        first = Image.new("RGBA", (16, 20))
        second = Image.new("RGBA", (16, 20))
        # A torso point stays at source (8, 4); the contact foot advances.
        for image, foot_x in ((first, 4), (second, 12)):
            image.putpixel((8, 4), (220, 10, 20, 128))
            image.putpixel((foot_x, 17), (200, 30, 50, 255))
        for image in (first, second):
            packed, x, y, info = sampled_cell(image, (0, 0, 16, 20), (8, 18), 1)
            points = [(px, py) for py in range(packed.height) for px in range(packed.width)
                      if packed.getpixel((px, py))[3] == 128]
            px, py = points[0]
            self.assertEqual((px + x - packed.width // 2, py + y - packed.height // 2), (0, -14))
            self.assertEqual(info["sampled_origin"], [8, 18])

    def test_fixed_sampling_phase_preserves_partial_alpha(self):
        image = Image.new("RGBA", (8, 8))
        image.putpixel((3, 3), (200, 100, 50, 97))
        packed, x, y, _ = sampled_cell(image, (0, 0, 8, 8), (4, 8), 2)
        self.assertIn(bytes((200, 100, 50, 97)), [packed.tobytes()[i:i+4] for i in range(0, len(packed.tobytes()), 4)])
        self.assertEqual((x, y), (0, -2))
        calibrated, cx, cy, _ = sampled_cell(image, (0, 0, 8, 8), (4, 8), 1, source_scale=0.5)
        self.assertEqual((calibrated.size, calibrated.tobytes(), cx, cy), (packed.size, packed.tobytes(), x, y))

    def test_weapon_reflection_preserves_authored_grip(self):
        grip, source_grip, size = (-6, -28), (3, 4), (70, 12)
        layer = weapon_layer(dict(grip=grip, rotation=35, scale=[0.6, 0.8]), size, source_grip)
        reflected = reflect_layer(layer)
        def position(layer):
            x = (source_grip[0] - size[0] / 2) * layer["scale_x"]
            y = (source_grip[1] - size[1] / 2) * layer["scale_y"]
            if layer["mirror"]:
                x = -x
            angle = math.radians(layer["rotation"])
            return (layer["x"] + x * math.cos(angle) - y * math.sin(angle),
                    layer["y"] + x * math.sin(angle) + y * math.cos(angle))
        original, mirrored = position(layer), position(reflected)
        self.assertAlmostEqual(mirrored[0], -original[0])
        self.assertAlmostEqual(mirrored[1], original[1])
        self.assertLess(abs(original[0] - grip[0]), 0.51)
        self.assertLess(abs(original[1] - grip[1]), 0.51)

    def test_preview_includes_overhead_weapon_tip(self):
        image = Image.new("RGBA", (70, 8), (200, 200, 200, 255))
        layer = dict(sprite_index=0, x=0, y=-110, rotation=270)
        layout = preview_layout({"weapon": ([image], [dict(frames=[dict(layers=[layer])])])})
        self.assertGreaterEqual(layout["origin_y"], 169)
        self.assertGreaterEqual(layout["cell_height"], layout["origin_y"] + 20)

    def config(self):
        source = Image.new("RGBA", (16, 40))
        for row in range(5):
            for column in range(2):
                source.putpixel((column * 8 + 2, row * 8 + 2), (20 + row, 100, 200, 97))
                source.putpixel((column * 8 + 4, row * 8 + 6), (20, 30 + column, 80, 255))
        source.save(self.root / "body.png")
        write_spr(self.root / "weapon.spr", [dict(width=2, height=2, sprite_type=1, pixels=b"\x01\x02\x03\x80" * 4)])
        groups = [dict(index=group, sheet="all", columns=[0, 1] if group in (1, 5) else [0], interval_ms=100, loop=True)
                  for group in range(13)]
        groups[5]["weapon_poses"] = {direction: [dict(grip=[-2, -4], rotation=20), None]
                                     for direction in ("S", "SW", "W", "NW", "N")}
        return dict(schema_version=1, downsample=1,
                    mirror_directions={"NE": "NW", "E": "W", "SE": "SW"},
                    sheets={"all": dict(path="body.png", cell_size=[8, 8], rows=["S", "SW", "W", "NW", "N"], origin=[4, 7])},
                    weapon=dict(spr="weapon.spr", source_grip=[1, 1]), groups=groups)

    def test_full_roundtrip_alpha_origins_and_paired_weapon(self):
        config = self.config()
        path = self.root / "build.json"
        path.write_text(json.dumps(config))
        output = self.root / "compiled"
        report = compile_package(path, output)
        self.assertTrue(report["paired_timelines"])
        self.assertEqual(report["body"]["actions"], 104)
        self.assertEqual(report["body"]["frames"], 120)
        body = read_spr(output / "ember_ronin.spr")
        self.assertEqual(body["indexed_count"], 0)
        self.assertIn(97, body["images"][0]["pixels"][3::4])
        preview = Image.open(output / "body/0000.png").convert("RGBA")
        self.assertEqual(preview.tobytes(), body["images"][0]["pixels"])
        ba, wa = (read_act(output / f"{name}.act")["actions"] for name in ("ember_ronin", "katana"))
        self.assertEqual(ba[6]["frames"][0]["layers"][0]["x"], -ba[2]["frames"][0]["layers"][0]["x"])
        self.assertTrue(ba[6]["frames"][0]["layers"][0]["mirror"])
        self.assertTrue(wa[40]["frames"][0]["layers"])
        self.assertFalse(wa[40]["frames"][1]["layers"])
        self.assertEqual(len(list(output.glob("contact_*.png"))), 13)
        self.assertTrue((output / "preview.html").is_file())
        self.assertNotIn("offhand", report)
        self.assertFalse((output / "katana_offhand.act").exists())
        before = (output / "ember_ronin.spr").read_bytes(), (output / "ember_ronin.act").read_bytes()
        compile_package(path, output)
        self.assertEqual(before, ((output / "ember_ronin.spr").read_bytes(), (output / "ember_ronin.act").read_bytes()))
        wa[8]["interval"] *= 2
        write_act(output / "katana.act", wa)
        with self.assertRaisesRegex(FormatError, "timeline mismatch"):
            validate_package(output, "ember_ronin", "katana")

    def test_offhand_poses_make_a_paired_dual_wield_layer(self):
        config = self.config()
        config["groups"][5]["offhand_poses"] = {direction: [None, dict(grip=[2, -4], rotation=200, mirror=True)]
                                                for direction in ("S", "SW", "W", "NW", "N")}
        path = self.root / "build.json"
        path.write_text(json.dumps(config))
        output = self.root / "compiled"
        report = compile_package(path, output)
        self.assertTrue(report["offhand"]["valid"])
        self.assertEqual(read_spr(output / "katana_offhand.spr")["images"], read_spr(output / "katana.spr")["images"])
        ba, oa = (read_act(output / f"{name}.act")["actions"] for name in ("ember_ronin", "katana_offhand"))
        for body, offhand in zip(ba, oa):
            self.assertEqual((len(body["frames"]), body["interval"]), (len(offhand["frames"]), offhand["interval"]))
        self.assertFalse(oa[40]["frames"][0]["layers"])
        west, east = oa[42]["frames"][1]["layers"][0], oa[46]["frames"][1]["layers"][0]
        self.assertEqual((west["x"], west["y"], west["rotation"], west["mirror"]), (2, -4, 200, True))
        self.assertEqual((east["x"], east["rotation"], east["mirror"]), (-2, 160, False))
        oa[40]["interval"] *= 2
        write_act(output / "katana_offhand.act", oa)
        with self.assertRaisesRegex(FormatError, "katana_offhand timeline mismatch"):
            validate_package(output, "ember_ronin", "katana", "katana_offhand")

    def test_body_calibration_scales_art_origin_and_grip_and_lowers_each_direction(self):
        config = self.config()
        config["body_scale"] = [2, 3]
        config["ground_drop"] = {"S": 5, "SW": 4, "W": 3, "NW": 2, "N": 1}
        path = self.root / "build.json"
        path.write_text(json.dumps(config))
        compile_package(path, self.root / "native", native=True)
        compile_package(path, self.root / "sized")
        native, sized = (read_act(self.root / name / "ember_ronin.act")["actions"] for name in ("native", "sized"))
        nspr, sspr = (read_spr(self.root / name / "ember_ronin.spr")["images"] for name in ("native", "sized"))
        def corner(layer, images):
            """Actor position of the opaque source pixel's top-left (it is drawn unmirrored)."""
            image = images[layer["sprite_index"]]
            i = image["pixels"][3::4].index(255)
            return (layer["x"] - image["width"] // 2 + i % image["width"], layer["y"] - image["height"] // 2 + i // image["width"])
        for action, drop in ((0, 5), (8, 5), (2, 3), (3, 2), (4, 1)):
            n, s = native[action]["frames"][0]["layers"][0], sized[action]["frames"][0]["layers"][0]
            # Twice as wide and three times as tall about the same origin, `drop` lower.
            (nx, ny), (sx, sy) = corner(n, nspr), corner(s, sspr)
            self.assertEqual((sx, sy), (2 * nx, 3 * ny + drop))
        weapon = read_act(self.root / "sized" / "katana.act")["actions"][40]["frames"][0]["layers"][0]
        plain = read_act(self.root / "native" / "katana.act")["actions"][40]["frames"][0]["layers"][0]
        # The grip (-2, -4) moves to (-4, -12 + 5); the blade keeps its size.
        self.assertEqual((weapon["x"] - plain["x"], weapon["y"] - plain["y"]), (-2, -3))
        self.assertEqual(read_spr(self.root / "sized" / "katana.spr")["images"], read_spr(self.root / "native" / "katana.spr")["images"])

    def test_empty_cells_and_missing_groups_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "Empty source cell"):
            sampled_cell(Image.new("RGBA", (8, 8)), [0, 0, 8, 8], [4, 7], 1)
        config = self.config()
        config["groups"].pop()
        path = self.root / "build.json"
        path.write_text(json.dumps(config))
        with self.assertRaisesRegex(ValueError, "each group"):
            compile_package(path, self.root / "compiled")

    def test_png_weapon_roundtrip_preserves_sampled_rgba_and_grip(self):
        config = self.config()
        image = Image.new("RGBA", (8, 8))
        image.putpixel((3, 3), (210, 120, 40, 97))
        image.putpixel((5, 3), (30, 40, 50, 255))
        image.putpixel((7, 5), (90, 80, 70, 128))
        source = self.root / "weapon.png"
        image.save(source)
        config["weapon"] = dict(png="weapon.png", rect=[2, 2, 8, 6], downsample=2, source_grip=[1, 1])
        for poses in config["groups"][5]["weapon_poses"].values():
            poses[0] = dict(grip=[-2, -4], rotation=90)
        path = self.root / "build.json"
        path.write_text(json.dumps(config))
        output = self.root / "compiled"
        report = compile_package(path, output)
        encoded = read_spr(output / "katana.spr")["images"][0]
        expected = bytes((210, 120, 40, 97, 30, 40, 50, 255, 0, 0, 0, 0,
                          0, 0, 0, 0, 0, 0, 0, 0, 90, 80, 70, 128))
        self.assertEqual((encoded["width"], encoded["height"]), (3, 2))
        self.assertEqual(encoded["pixels"], expected)
        self.assertEqual(report["source_sha256"]["weapon.png"], hashlib.sha256(source.read_bytes()).hexdigest())
        layer = read_act(output / "katana.act")["actions"][40]["frames"][0]["layers"][0]
        # The source grip is at (1,1) within a 3x2 image, not original PNG (1,1).
        self.assertEqual((layer["x"], layer["y"], layer["rotation"]), (-2, -4, 90))
        self.assertTrue(report["paired_timelines"])

    def test_png_weapon_defaults_keep_entire_image_and_invalid_inputs_fail(self):
        image = Image.new("RGBA", (6, 4))
        image.putpixel((3, 2), (3, 7, 9, 65))
        path = self.root / "weapon.png"
        image.save(path)
        sampled = sampled_weapon_png(path, {})
        self.assertEqual((sampled.size, sampled.tobytes()), (image.size, image.tobytes()))
        for rect in ([0, 0, 7, 4], [-1, 0, 6, 4], [2, 0, 2, 4], [0, 0, 6], [0, 0, 6.0, 4]):
            with self.subTest(rect=rect), self.assertRaisesRegex(ValueError, "rectangle"):
                sampled_weapon_png(path, dict(rect=rect))
        for factor in (0, -1, 1.5, True):
            with self.subTest(factor=factor), self.assertRaisesRegex(ValueError, "positive integer"):
                sampled_weapon_png(path, dict(downsample=factor))
        config = self.config()
        config["weapon"]["png"] = "weapon.png"
        build = self.root / "build.json"
        build.write_text(json.dumps(config))
        with self.assertRaisesRegex(ValueError, "exactly one"):
            compile_package(build, self.root / "compiled")


if __name__ == "__main__":
    unittest.main()
