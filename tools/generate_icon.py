from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


SIZE = 1024
OUTER_MARGIN = 64
OUTER_RADIUS = 224
INNER_MARGIN = 176
INNER_RADIUS = 112


def rounded_mask(left: int, top: int, right: int, bottom: int, radius: int) -> Image.Image:
    mask = Image.new("L", (right - left, bottom - top), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (left, top, right, bottom),
        radius=radius,
        fill=255,
    )
    return mask


def vertical_gradient(
    size: int,
    left: int,
    top: int,
    right: int,
    bottom: int,
    top_color: tuple[int, int, int],
    bottom_color: tuple[int, int, int],
) -> Image.Image:
    gradient = Image.new("RGB", (right - left, bottom - top))
    draw = ImageDraw.Draw(gradient)
    height = bottom - top
    for y in range(height):
        ratio = y / max(1, height - 1)
        color = tuple(
            round(top_color[channel] + (bottom_color[channel] - top_color[channel]) * ratio)
            for channel in range(3)
        )
        draw.line([(0, y), (gradient.width, y)], fill=color)
    return gradient.crop((0, 0, right - left, bottom - top))


def generate_icon() -> Image.Image:
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    outer_left = OUTER_MARGIN
    outer_top = OUTER_MARGIN
    outer_right = SIZE - OUTER_MARGIN
    outer_bottom = SIZE - OUTER_MARGIN

    background = vertical_gradient(
        SIZE,
        outer_left,
        outer_top,
        outer_right,
        outer_bottom,
        (38, 51, 70),
        (15, 22, 32),
    )
    outer_mask = rounded_mask(outer_left, outer_top, outer_right, outer_bottom, OUTER_RADIUS)
    canvas.paste(background.convert("RGBA"), (outer_left, outer_top), outer_mask)

    highlight = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(highlight).rounded_rectangle(
        (outer_left + 14, outer_top + 14, outer_right - 14, outer_bottom - 14),
        radius=OUTER_RADIUS - 28,
        outline=(255, 255, 255, 22),
        width=14,
    )
    canvas.alpha_composite(highlight)

    inner_left = INNER_MARGIN
    inner_top = INNER_MARGIN
    inner_right = SIZE - INNER_MARGIN
    inner_bottom = SIZE - INNER_MARGIN
    scene = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    sky = vertical_gradient(
        SIZE,
        0,
        inner_top,
        SIZE,
        inner_bottom,
        (170, 227, 255),
        (66, 136, 255),
    ).convert("RGBA")
    scene.paste(sky, (0, 0))
    inner_mask = rounded_mask(inner_left, inner_top, inner_right, inner_bottom, INNER_RADIUS)
    inner_mask_full = Image.new("L", (SIZE, SIZE), 0)
    inner_mask_full.paste(inner_mask, (inner_left, inner_top))
    scene.putalpha(inner_mask_full)

    scene_draw = ImageDraw.Draw(scene)
    sun_center = (645, 330)
    sun_radius = 92
    glow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse(
        (
            sun_center[0] - sun_radius - 42,
            sun_center[1] - sun_radius - 42,
            sun_center[0] + sun_radius + 42,
            sun_center[1] + sun_radius + 42,
        ),
        fill=(255, 224, 130, 110),
    )
    glow = glow.filter(ImageFilter.GaussianBlur(28))
    scene.alpha_composite(glow)
    scene_draw.ellipse(
        (
            sun_center[0] - sun_radius,
            sun_center[1] - sun_radius,
            sun_center[0] + sun_radius,
            sun_center[1] + sun_radius,
        ),
        fill=(255, 209, 102),
    )

    back_mountain = [
        (inner_left, inner_bottom),
        (inner_left, 590),
        (355, 430),
        (530, 560),
        (710, 420),
        (inner_right, 570),
        (inner_right, inner_bottom),
    ]
    scene_draw.polygon(back_mountain, fill=(58, 110, 170, 255))

    front_mountain = [
        (inner_left, inner_bottom),
        (inner_left, 700),
        (270, 500),
        (505, 715),
        (735, 480),
        (inner_right, 690),
        (inner_right, inner_bottom),
    ]
    scene_draw.polygon(front_mountain, fill=(22, 122, 105, 255))

    snow_cap = [
        (270, 500),
        (315, 545),
        (345, 520),
        (380, 555),
        (420, 520),
        (450, 560),
        (485, 540),
        (505, 715),
        (270, 500),
    ]
    scene_draw.polygon(snow_cap, fill=(240, 248, 255, 255))

    canvas.alpha_composite(scene)

    frame = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(frame).rounded_rectangle(
        (inner_left - 24, inner_top - 24, inner_right + 24, inner_bottom + 24),
        radius=INNER_RADIUS + 24,
        outline=(244, 247, 251, 225),
        width=24,
    )
    canvas.alpha_composite(frame)

    return canvas


def main() -> None:
    assets = Path(__file__).resolve().parents[1] / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    icon = generate_icon()
    icon.save(assets / "icon.png", format="PNG", optimize=True)
    icon.save(
        assets / "icon.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


if __name__ == "__main__":
    main()
