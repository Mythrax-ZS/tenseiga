"""Compile authored imagegen grids and explicit placement metadata to RO assets.

This compiler only crops, samples and encodes source artwork. It never draws
artwork, guesses anatomy, re-centers individual silhouettes or invents frames.
Python 3.12+ and Pillow are required. See README.md for the input contract.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil

from PIL import Image, ImageDraw

from ro_formats import DIRECTIONS, PLAYER_ACTIONS, FormatError, read_act, read_spr, validate_pair, write_act, write_spr


def save_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def rgba_images(spr):
    result = []
    for image in spr["images"]:
        if image["sprite_type"]:
            pixels = image["pixels"]
        else:
            palette = spr["palette"]
            pixels = bytes(c for i in image["pixels"] for c in (palette[4*i:4*i+3] + b"\xff" if i else bytes(4)))
        result.append(Image.frombytes("RGBA", (image["width"], image["height"]), pixels))
    return result


def raw_image(image):
    return dict(width=image.width, height=image.height, sprite_type=1, pixels=image.tobytes())


def sampled_cell(image, rect, origin, factor, shift=(0, 0), source_scale=1, body_scale=(1, 1), drop=0):
    """Sample on the fixed cell grid, then trim; maintain the authored origin.

    Sampling before the silhouette crop keeps the same pixel phase for every
    frame. A uniform optional sheet calibration is applied in this same sample,
    and so is the package's body calibration (`body_scale` across and down,
    then `drop` pixels lower on the ground origin).
    """
    left, top, right, bottom = rect
    if not (0 <= left < right <= image.width and 0 <= top < bottom <= image.height):
        raise ValueError(f"Cell rectangle {rect} is outside {image.size}")
    cell = image.crop(rect)
    bounds = cell.getchannel("A").getbbox()
    if bounds is None:
        raise ValueError(f"Empty source cell {rect}")
    contacts = [name for name, hit in zip(("left", "top", "right", "bottom"),
                (bounds[0] == 0, bounds[1] == 0, bounds[2] == cell.width, bounds[3] == cell.height)) if hit]
    visible_bounds = cell.getchannel("A").point(lambda alpha: 255 if alpha >= 128 else 0).getbbox()
    visible_contacts = [] if visible_bounds is None else [
        name for name, hit in zip(("left", "top", "right", "bottom"),
        (visible_bounds[0] == 0, visible_bounds[1] == 0,
         visible_bounds[2] == cell.width, visible_bounds[3] == cell.height)) if hit]
    if not math.isfinite(source_scale) or source_scale <= 0:
        raise ValueError("source_scale must be finite and positive")
    rx, ry = (factor / (source_scale * k) for k in body_scale)
    size = (math.ceil(cell.width / rx), math.ceil(cell.height / ry))
    sampled = cell.transform(size, Image.Transform.AFFINE, (rx, 0, 0, 0, ry, 0),
                             Image.Resampling.NEAREST, fillcolor=(0, 0, 0, 0))
    box = sampled.getchannel("A").getbbox()
    if box is None:
        raise ValueError(f"Cell disappears at downsample={factor}: {rect}")
    trimmed = sampled.crop(box)
    even = Image.new("RGBA", (trimmed.width + trimmed.width % 2, trimmed.height + trimmed.height % 2))
    even.paste(trimmed, (0, 0))
    ox, oy = round(origin[0] / rx), round(origin[1] / ry)
    x = box[0] + even.width // 2 - ox + round(shift[0] * body_scale[0])
    y = box[1] + even.height // 2 - oy + round(shift[1] * body_scale[1]) + drop
    if any(not isinstance(v, int) for v in (x, y)):
        raise ValueError("Output offsets must be whole pixels")
    return even, x, y, {"rect": list(rect), "origin": list(origin), "sampled_origin": [ox, oy],
                        "output_shift": list(shift), "source_scale": source_scale,
                        "body_scale": list(body_scale), "ground_drop": drop,
                        "sample_divisor": factor, "source_edge_contacts": contacts,
                        "source_visible_edge_contacts": visible_contacts,
                        "source_visible_bbox": list(visible_bounds) if visible_bounds else None}


def reflect_layer(layer):
    mirrored = dict(layer)
    mirrored["x"] = -layer.get("x", 0)
    mirrored["rotation"] = (-layer.get("rotation", 0)) % 360
    mirrored["mirror"] = not layer.get("mirror", False)
    return mirrored


def weapon_layer(pose, size, source_grip):
    if pose is None:
        return None
    grip_x, grip_y = pose["grip"]
    sx, sy = pose.get("scale", [1, 1])
    rotation = pose["rotation"]
    mirrored = pose.get("mirror", False)
    dx, dy = (size[0] / 2 - source_grip[0]) * sx, (size[1] / 2 - source_grip[1]) * sy
    if mirrored:
        dx = -dx
    angle = math.radians(rotation)
    return dict(sprite_index=0, sprite_type=1, width=size[0], height=size[1],
                x=round(grip_x + dx * math.cos(angle) - dy * math.sin(angle)),
                y=round(grip_y + dx * math.sin(angle) + dy * math.cos(angle)),
                rotation=rotation, mirror=mirrored, scale_x=sx, scale_y=sy)


def weapon_frame(pose, image, source_grip, mirrored):
    layer = weapon_layer(pose, image.size, source_grip) if pose else None
    if mirrored and layer:
        layer = reflect_layer(layer)
    return dict(layers=[layer] if layer else [], anchors=[])


def sampled_weapon_png(path, spec):
    """Sample an authored rectangle once, retaining its transparent margins."""
    image = Image.open(path).convert("RGBA")
    rect = spec.get("rect", [0, 0, image.width, image.height])
    if (not isinstance(rect, (list, tuple)) or len(rect) != 4
            or any(type(value) is not int for value in rect)
            or not (0 <= rect[0] < rect[2] <= image.width and 0 <= rect[1] < rect[3] <= image.height)):
        raise ValueError(f"Weapon PNG rectangle {rect} is outside {image.size} or is not four integers")
    factor = spec.get("downsample", 1)
    if type(factor) is not int or factor < 1:
        raise ValueError("Weapon PNG downsample must be a positive integer")
    cell = image.crop(rect)
    size = tuple(math.ceil(value / factor) for value in cell.size)
    return cell.transform(size, Image.Transform.AFFINE, (factor, 0, 0, 0, factor, 0),
                          Image.Resampling.NEAREST, fillcolor=(0, 0, 0, 0))


def body_calibration(config, native=False):
    """((scale across, scale down), {direction: ground drop}): the body against official
    bodies. `ground_drop` is pixels or, per unmirrored direction, a map of them.
    `native` is the artwork's own size, where separate_head.py cuts the head."""
    if native:
        return (1, 1), {d: 0 for d in DIRECTIONS}
    scale, drop = config.get("body_scale", [1, 1]), config.get("ground_drop", 0)
    if (not isinstance(scale, list) or len(scale) != 2
            or any(not isinstance(k, (int, float)) or not math.isfinite(k) or k <= 0 for k in scale)):
        raise ValueError("body_scale must be two positive numbers")
    drops = drop if isinstance(drop, dict) else {d: drop for d in DIRECTIONS}
    if set(drops) - set(DIRECTIONS) or any(type(v) is not int for v in drops.values()):
        raise ValueError("ground_drop must be whole pixels, or a map of them by direction")
    return tuple(scale), {d: drops.get(d, 0) for d in DIRECTIONS}


def calibrated_pose(pose, scale, drop):
    """A weapon grip moves with the hand it sits in; the blade keeps its size."""
    if pose is None:
        return None
    gx, gy = pose["grip"]
    return dict(pose, grip=[gx * scale[0], gy * scale[1] + drop])


def compile_package(config_path, destination, native=False):
    config_path, destination = Path(config_path).resolve(), Path(destination).resolve()
    config = json.loads(config_path.read_text(encoding="utf-8"))
    if config.get("schema_version") != 1:
        raise ValueError("Expected schema_version 1")
    divisor = config["downsample"]
    if not isinstance(divisor, int) or divisor < 1:
        raise ValueError("downsample must be a positive integer")
    body_scale, drops = body_calibration(config, native)
    groups = {group["index"]: group for group in config["groups"]}
    if len(groups) != 13 or set(groups) != set(range(13)) or len(config["groups"]) != 13:
        raise ValueError("Provide each group 0 through 12 exactly once")
    sheets, source_hashes = {}, {}
    for name, spec in config["sheets"].items():
        path = (config_path.parent / spec["path"]).resolve()
        sheets[name] = Image.open(path).convert("RGBA")
        source_hashes[spec["path"]] = hashlib.sha256(path.read_bytes()).hexdigest()
        rows = spec.get("rows", [])
        if len(rows) != len(set(rows)) or any(row not in DIRECTIONS for row in rows):
            raise ValueError(f"Invalid direction rows in {name}")
    body_images, image_ids, source_frames, actions = [], {}, {}, []
    mirrors = config.get("mirror_directions", {})
    for direction, source in mirrors.items():
        if direction not in DIRECTIONS or source not in DIRECTIONS or source in mirrors:
            raise ValueError("Mirrors must map one known direction to an unmirrored known direction")
    # A mirrored direction is its source's art, so it stands on the source's ground line.
    drops = {d: drops[mirrors.get(d, d)] for d in DIRECTIONS}
    weapon_config = config.get("weapon")
    if weapon_config:
        source_types = [key for key in ("spr", "png") if key in weapon_config]
        if len(source_types) != 1:
            raise ValueError("Weapon must specify exactly one of spr or png")
        source_type = source_types[0]
        source_name = weapon_config[source_type]
        weapon_path = (config_path.parent / source_name).resolve()
        if source_type == "png":
            weapon_image = sampled_weapon_png(weapon_path, weapon_config)
        else:
            weapon_images = rgba_images(read_spr(weapon_path))
            weapon_image = weapon_images[weapon_config.get("image", 0)]
        source_hashes[source_name] = hashlib.sha256(weapon_path.read_bytes()).hexdigest()
    else:
        weapon_image = Image.new("RGBA", (2, 2))
    # Off-hand poses (dual wielding) make a second pair from the same weapon image.
    has_offhand = any(group.get("offhand_poses") for group in config["groups"])
    weapon_actions, offhand_actions = [], []
    diagnostics = []
    for group_index in range(13):
        group = groups[group_index]
        for direction in DIRECTIONS:
            source_direction = mirrors.get(direction, direction)
            is_mirrored = source_direction != direction
            specs = group.get("frames", {}).get(source_direction)
            if specs is None:
                sheet_name = group["sheet"]
                row = config["sheets"][sheet_name]["rows"].index(source_direction)
                specs = [dict(sheet=sheet_name, row=row, column=col) for col in group["columns"]]
            if not specs:
                raise ValueError(f"Group {group_index} direction {direction} is empty")
            body_frames, weapon_frames, offhand_frames = [], [], []
            poses = group.get("weapon_poses", {}).get(source_direction, [None] * len(specs))
            offhand_poses = group.get("offhand_poses", {}).get(source_direction, [None] * len(specs))
            if len(poses) != len(specs) or len(offhand_poses) != len(specs):
                raise ValueError(f"Weapon frame count differs in group {group_index} direction {direction}")
            if any(pose is not None for pose in poses + offhand_poses) and not weapon_config:
                raise ValueError("Weapon poses require a weapon SPR and source grip")
            drop = drops[source_direction]
            poses = [calibrated_pose(pose, body_scale, drop) for pose in poses]
            offhand_poses = [calibrated_pose(pose, body_scale, drop) for pose in offhand_poses]
            for frame_index, ref in enumerate(specs):
                sheet_name, row, column = ref["sheet"], ref["row"], ref["column"]
                key = f"{sheet_name}:{row}:{column}"
                cell_spec = dict(config.get("cell_overrides", {}).get(key, {}))
                cell_spec.update(ref)
                source_key = json.dumps(cell_spec, sort_keys=True) + (f" drop {drop}" if drop else "")
                if source_key not in source_frames:
                    sheet_spec = config["sheets"][sheet_name]
                    if "rect" in cell_spec:
                        rect = cell_spec["rect"]
                    elif "x_edges" in sheet_spec:
                        xs, ys = sheet_spec["x_edges"], sheet_spec["y_edges"]
                        rect = [xs[column], ys[row], xs[column + 1], ys[row + 1]]
                    else:
                        width, height = sheet_spec["cell_size"]
                        mx, my = sheet_spec.get("offset", [0, 0])
                        rect = [mx + column * width, my + row * height, mx + (column + 1) * width, my + (row + 1) * height]
                    origin = cell_spec.get("origin", sheet_spec["origin"])
                    image, x, y, info = sampled_cell(sheets[sheet_name], rect, origin, divisor,
                                                   cell_spec.get("shift", [0, 0]), sheet_spec.get("source_scale", 1),
                                                   body_scale, drop)
                    digest = hashlib.sha256(str(image.size).encode() + image.tobytes()).hexdigest()
                    if digest not in image_ids:
                        image_ids[digest] = len(body_images)
                        body_images.append(image)
                    index = image_ids[digest]
                    layer = dict(sprite_index=index, sprite_type=1, width=image.width, height=image.height, x=x, y=y)
                    source_frames[source_key] = (layer, info)
                layer, info = source_frames[source_key]
                layer = reflect_layer(layer) if is_mirrored else dict(layer)
                anchor_y = round(config.get("head_anchor_y", -40) * body_scale[1]) + drop
                body_frames.append(dict(layers=[layer], anchors=[dict(x=0, y=anchor_y)]))
                grip = weapon_config["source_grip"] if weapon_config else None
                weapon_frames.append(weapon_frame(poses[frame_index], weapon_image, grip, is_mirrored))
                offhand_frames.append(weapon_frame(offhand_poses[frame_index], weapon_image, grip, is_mirrored))
                diagnostics.append(dict(action=group_index * 8 + DIRECTIONS.index(direction), frame=frame_index,
                    source=key, mirror=is_mirrored, output_layer=layer, **info))
            action = dict(interval=group["interval_ms"] / 25, frames=body_frames)
            actions.append(action)
            weapon_actions.append(dict(interval=action["interval"], frames=weapon_frames))
            offhand_actions.append(dict(interval=action["interval"], frames=offhand_frames))
    destination.mkdir(parents=True, exist_ok=True)
    body_name, weapon_name = config.get("body", "ember_ronin"), config.get("weapon_name", "katana")
    for name in (body_name, weapon_name):
        if not name or Path(name).name != name:
            raise ValueError("Asset names must be plain basenames")
    write_spr(destination / f"{body_name}.spr", [raw_image(image) for image in body_images])
    write_act(destination / f"{body_name}.act", actions)
    write_spr(destination / f"{weapon_name}.spr", [raw_image(weapon_image)])
    write_act(destination / f"{weapon_name}.act", weapon_actions)
    offhand_name = f"{weapon_name}_offhand" if has_offhand else None
    if offhand_name:
        write_spr(destination / f"{offhand_name}.spr", [raw_image(weapon_image)])
        write_act(destination / f"{offhand_name}.act", offhand_actions)
    report = validate_package(destination, body_name, weapon_name, offhand_name)
    report.update(source_sha256=source_hashes, config_sha256=hashlib.sha256(config_path.read_bytes()).hexdigest(),
                  downsample=divisor, body_scale=list(body_scale), ground_drop=drops, source_cells=diagnostics,
                  visual_status="Requires human review of all action directions; structural checks do not certify animation quality.")
    save_json(destination / "validation_report.json", report)
    save_json(destination / "build_config.json", config)
    write_previews(destination, config, body_name, weapon_name, offhand_name)
    return report


def validate_package(directory, body_name, weapon_name, offhand_name=None):
    directory = Path(directory)
    body = validate_pair(directory / f"{body_name}.spr", directory / f"{body_name}.act", True)
    weapon = validate_pair(directory / f"{weapon_name}.spr", directory / f"{weapon_name}.act")
    ba = read_act(directory / f"{body_name}.act")["actions"]
    for name in filter(None, (weapon_name, offhand_name)):
        wa = read_act(directory / f"{name}.act")["actions"]
        if len(wa) != len(ba):
            raise FormatError(f"Body and {name} action counts differ")
        for index, (b, w) in enumerate(zip(ba, wa)):
            if len(b["frames"]) != len(w["frames"]) or b["interval"] != w["interval"]:
                raise FormatError(f"Body and {name} timeline mismatch in action {index}")
    images = read_spr(directory / f"{body_name}.spr")["images"]
    x = y = row_height = 1
    for image in images:
        if image["width"] + 2 > 1024:
            raise FormatError("Image exceeds the client's 1024px sprite atlas width")
        if x + image["width"] + 1 > 1024:
            x = 1
            y += row_height + 1
            row_height = 0
        x += image["width"] + 1
        row_height = max(row_height, image["height"])
    report = dict(body=body, weapon=weapon, paired_timelines=True, body_atlas_size=[1024, y + row_height + 1])
    if offhand_name:
        report["offhand"] = validate_pair(directory / f"{offhand_name}.spr", directory / f"{offhand_name}.act")
    return report


def render_layers(canvas, images, layers, origin):
    for layer in layers:
        if layer["sprite_index"] < 0:
            continue
        image = images[layer["sprite_index"]]
        if layer.get("mirror"):
            image = image.transpose(Image.Transpose.FLIP_LEFT_RIGHT)
        sx, sy = layer.get("scale_x", 1), layer.get("scale_y", 1)
        if sx != 1 or sy != 1:
            image = image.resize((max(1, round(image.width * sx)), max(1, round(image.height * sy))), Image.Resampling.NEAREST)
        if layer.get("rotation", 0):
            image = image.rotate(-layer["rotation"], Image.Resampling.NEAREST, expand=True)
        canvas.alpha_composite(image, (round(origin[0] + layer["x"] - image.width / 2),
                                       round(origin[1] + layer["y"] - image.height / 2)))


def preview_layout(parsed):
    """Include every transformed layer corner, including raised weapon tips."""
    corners = [(0, 0)]
    for images, actions in parsed.values():
        for action in actions:
            for frame in action["frames"]:
                for layer in frame["layers"]:
                    if layer["sprite_index"] < 0:
                        continue
                    image = images[layer["sprite_index"]]
                    angle = math.radians(layer.get("rotation", 0))
                    for dx in (-image.width / 2, image.width / 2):
                        for dy in (-image.height / 2, image.height / 2):
                            x = dx * layer.get("scale_x", 1) * (-1 if layer.get("mirror") else 1)
                            y = dy * layer.get("scale_y", 1)
                            corners.append((layer["x"] + x * math.cos(angle) - y * math.sin(angle),
                                            layer["y"] + x * math.sin(angle) + y * math.cos(angle)))
    half_width = max(96, math.ceil(max(abs(x) for x, _ in corners)) + 16)
    origin_y = max(160, math.ceil(-min(y for _, y in corners)) + 24)
    return dict(cell_width=half_width * 2, cell_height=max(192, origin_y + math.ceil(max(y for _, y in corners)) + 20),
                origin_x=half_width, origin_y=origin_y)


def write_previews(directory, config, body_name, weapon_name, offhand_name=None):
    """All preview images are decoded from the completed SPR/ACT outputs."""
    manifest = dict(name=config.get("name", "Samurai"), directions=list(DIRECTIONS), groups=[], sheets={})
    parsed = {}
    kinds = [("body", body_name), ("weapon", weapon_name)] + ([("offhand", offhand_name)] if offhand_name else [])
    for kind, name in kinds:
        sprite, action = read_spr(directory / f"{name}.spr"), read_act(directory / f"{name}.act")
        images = rgba_images(sprite)
        paths = []
        folder = directory / kind
        folder.mkdir(exist_ok=True)
        for index, image in enumerate(images):
            path = f"{kind}/{index:04d}.png"
            image.save(directory / path)
            paths.append(path)
        parsed[kind] = (images, action["actions"])
        manifest["sheets"][kind] = dict(images=paths, actions=action["actions"])
    layout = preview_layout(parsed)
    manifest["layout"] = layout
    cw, ch = layout["cell_width"], layout["cell_height"]
    config_groups = {group["index"]: group for group in config["groups"]}
    for group_index in range(13):
        group = config_groups[group_index]
        manifest["groups"].append(dict(index=group_index, name=PLAYER_ACTIONS[group_index], loop=group.get("loop", True)))
        longest = max(len(parsed["body"][1][group_index * 8 + d]["frames"]) for d in range(8))
        sheet = Image.new("RGBA", (longest * cw, 8 * ch), "#26303c")
        draw = ImageDraw.Draw(sheet)
        for direction in range(8):
            index = group_index * 8 + direction
            action = parsed["body"][1][index]
            draw.text((4, direction * ch + 2), DIRECTIONS[direction], fill="#e6e8ed")
            for fi in range(len(action["frames"])):
                origin = (fi * cw + layout["origin_x"], direction * ch + layout["origin_y"])
                draw.line((origin[0] - 50, origin[1], origin[0] + 50, origin[1]), fill="#526272")
                # The off-hand blade is drawn just before the main one, at the same depth.
                weapons = [kind for kind in ("offhand", "weapon") if kind in parsed]
                order = weapons + ["body"] if direction in (3, 4, 5) else ["body"] + weapons
                for kind in order:
                    images, actions = parsed[kind]
                    render_layers(sheet, images, actions[index]["frames"][fi]["layers"], origin)
        sheet.convert("RGB").save(directory / f"contact_{group_index:02d}_{PLAYER_ACTIONS[group_index]}.png")
    save_json(directory / "preview_manifest.json", manifest)
    (directory / "preview_manifest.js").write_text("window.SAMURAI_PREVIEW=" + json.dumps(manifest) + ";\n", encoding="utf-8")
    shutil.copy2(Path(__file__).with_name("preview.html"), directory / "preview.html")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--native", action="store_true",
                        help="the artwork's own size, ignoring body_scale and ground_drop (for separate_head.py)")
    args = parser.parse_args()
    report = compile_package(args.config, args.output, args.native)
    print(json.dumps({key: value for key, value in report.items() if key != "source_cells"}, indent=2))


if __name__ == "__main__":
    main()
