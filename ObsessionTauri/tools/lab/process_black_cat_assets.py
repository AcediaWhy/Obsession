from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageSequence


PROJECT_ROOT = Path(__file__).resolve().parents[2]
DOWNLOADS = Path.home() / "Downloads"
OUTPUT_ROOT = PROJECT_ROOT / "public" / "lab-assets" / "black-cat-states"

STATE_SOURCES = {
    "idle": DOWNLOADS / "Cute_seamless_loopin...-956995755-0.gif",
    "engaging": DOWNLOADS / "A_cozy_cute_cat_yawn...-1007727643-0.gif",
    "scanning": DOWNLOADS / "Seamless_looping_2D_...-576076388-0.gif",
    "focused": DOWNLOADS / "Cozy_cute_cat_smooth...-254265045-0.gif",
    "fault": DOWNLOADS / "Cute_black_kitten_in...-1184925030-0.gif",
}

BASE_SIZE = 256
ART_CROP = (7, 8, 231, 232)
PAW_PATCH = (70, 188, 155, 224)
DISPLAY_SIZES = (128, 256)
DEFAULT_THRESHOLD = 52


def remove_outer_matte(frame: Image.Image, threshold: int) -> Image.Image:
    """Separate the cat by contrast, then retain its enclosed face details."""
    frame = frame.convert("RGBA").resize((BASE_SIZE, BASE_SIZE), Image.Resampling.NEAREST)
    pixels = np.asarray(frame).copy()
    rgb = pixels[:, :, :3]
    alpha = pixels[:, :, 3]
    corner = rgb[0, 0]
    exact_background = (alpha == 0) | np.all(rgb == corner, axis=2)

    distance_from_background = np.max(
        np.abs(rgb.astype(np.int16) - corner.astype(np.int16)), axis=2
    )
    foreground = (alpha > 0) & (distance_from_background >= threshold)

    flood_map = Image.fromarray(np.where(foreground, 0, 255).astype(np.uint8), mode="L").copy()
    ImageDraw.floodfill(flood_map, (0, 0), 128, thresh=0)
    outside = np.asarray(flood_map) == 128

    keep = foreground | (~outside)
    face_region = np.zeros_like(keep)
    face_region[82:159, 60:201] = True
    keep &= (~exact_background) | face_region
    pale_non_face_detail = keep & (~face_region) & (np.min(rgb, axis=2) > 190)
    pixels[pale_non_face_detail, :3] = (184, 158, 133)
    channel_spread = np.max(rgb, axis=2) - np.min(rgb, axis=2)
    neutral_light_fringe = (
        keep
        & (~face_region)
        & (np.max(rgb, axis=2) > 100)
        & (channel_spread <= 8)
    )
    pixels[neutral_light_fringe, :3] = (46, 47, 46)
    transparent = ~keep
    outer_edge = np.zeros_like(keep)
    outer_edge[1:] |= transparent[:-1]
    outer_edge[:-1] |= transparent[1:]
    outer_edge[:, 1:] |= transparent[:, :-1]
    outer_edge[:, :-1] |= transparent[:, 1:]
    pixels[keep & outer_edge, :3] = (10, 9, 9)
    pixels[:, :, 3] = np.where(keep, 255, 0).astype(np.uint8)
    return Image.fromarray(pixels, mode="RGBA")


def load_clean_frames(source: Path, threshold: int) -> tuple[list[Image.Image], list[int]]:
    if not source.exists():
        raise FileNotFoundError(source)

    image = Image.open(source)
    frames: list[Image.Image] = []
    durations: list[int] = []
    for frame in ImageSequence.Iterator(image):
        frames.append(remove_outer_matte(frame, threshold).crop(ART_CROP))
        durations.append(int(frame.info.get("duration", image.info.get("duration", 150))))

    paw_left, paw_top, paw_right, paw_bottom = PAW_PATCH
    paw_anchor = np.asarray(frames[0])[paw_top:paw_bottom, paw_left:paw_right].copy()
    for index, frame in enumerate(frames):
        pixels = np.asarray(frame).copy()
        pixels[paw_top:paw_bottom, paw_left:paw_right] = paw_anchor
        frames[index] = Image.fromarray(pixels, mode="RGBA")
    return frames, durations


def darken_outer_edge(frame: Image.Image) -> Image.Image:
    pixels = np.asarray(frame.convert("RGBA")).copy()
    opaque = pixels[:, :, 3] > 0
    outlined = opaque.copy()
    outlined[1:] |= opaque[:-1]
    outlined[:-1] |= opaque[1:]
    outlined[:, 1:] |= opaque[:, :-1]
    outlined[:, :-1] |= opaque[:, 1:]
    outlined[1:, 1:] |= opaque[:-1, :-1]
    outlined[1:, :-1] |= opaque[:-1, 1:]
    outlined[:-1, 1:] |= opaque[1:, :-1]
    outlined[:-1, :-1] |= opaque[1:, 1:]
    new_outline = outlined & (~opaque)
    pixels[new_outline, :3] = (10, 9, 9)
    pixels[new_outline, 3] = 255
    opaque = outlined
    near_transparency = ~opaque
    fringe = np.zeros_like(opaque)
    for _ in range(max(3, frame.width // 64 + 1)):
        expanded = near_transparency.copy()
        expanded[1:] |= near_transparency[:-1]
        expanded[:-1] |= near_transparency[1:]
        expanded[:, 1:] |= near_transparency[:, :-1]
        expanded[:, :-1] |= near_transparency[:, 1:]
        fringe |= expanded & opaque
        near_transparency = expanded
    light_fringe = fringe & (np.max(pixels[:, :, :3], axis=2) > 130)
    pixels[light_fringe, :3] = (10, 9, 9)
    return Image.fromarray(pixels, mode="RGBA")


def save_webp(
    path: Path,
    frames: list[Image.Image],
    durations: list[int],
    *,
    loop: int,
    size: int,
) -> None:
    resized = [
        darken_outer_edge(frame.resize((size, size), Image.Resampling.NEAREST))
        for frame in frames
    ]
    resized[0].save(
        path,
        format="WEBP",
        save_all=True,
        append_images=resized[1:],
        duration=durations,
        loop=loop,
        lossless=True,
        method=6,
    )


def create_preview(thresholds: tuple[int, ...], output: Path) -> None:
    source = STATE_SOURCES["idle"]
    image = Image.open(source)
    frame = next(ImageSequence.Iterator(image)).convert("RGBA")
    tile_size = 256
    preview = Image.new("RGB", (tile_size * len(thresholds), tile_size + 28), (31, 45, 39))
    draw = ImageDraw.Draw(preview)
    for index, threshold in enumerate(thresholds):
        cleaned = remove_outer_matte(frame, threshold)
        tile = Image.new("RGBA", (tile_size, tile_size), (31, 45, 39, 255))
        tile.alpha_composite(cleaned)
        x = index * tile_size
        preview.paste(tile.convert("RGB"), (x, 0))
        draw.text((x + 8, tile_size + 7), f"threshold {threshold}", fill=(244, 232, 201))
    preview.save(output)


def create_contact_sheet(threshold: int, output: Path) -> None:
    tile_size = 128
    row_height = tile_size + 22
    max_frames = max(Image.open(source).n_frames for source in STATE_SOURCES.values())
    preview = Image.new("RGB", (tile_size * max_frames, row_height * len(STATE_SOURCES)), (31, 45, 39))
    draw = ImageDraw.Draw(preview)
    for row, (state, source) in enumerate(STATE_SOURCES.items()):
        frames, _ = load_clean_frames(source, threshold)
        y = row * row_height
        for column, frame in enumerate(frames):
            tile = Image.new("RGBA", (tile_size, tile_size), (31, 45, 39, 255))
            tile.alpha_composite(frame.resize((tile_size, tile_size), Image.Resampling.NEAREST))
            preview.paste(tile.convert("RGB"), (column * tile_size, y))
        draw.text((6, y + tile_size + 4), state, fill=(244, 232, 201))
    preview.save(output)


def generate(threshold: int) -> None:
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    for state, source in STATE_SOURCES.items():
        frames, durations = load_clean_frames(source, threshold)
        for size in DISPLAY_SIZES:
            save_webp(
                OUTPUT_ROOT / f"{state}-{size}.webp",
                frames,
                durations,
                loop=0,
                size=size,
            )
            darken_outer_edge(frames[0].resize((size, size), Image.Resampling.NEAREST)).save(
                OUTPUT_ROOT / f"{state}-{size}-still.webp",
                format="WEBP",
                lossless=True,
                method=6,
            )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--threshold", type=int, default=DEFAULT_THRESHOLD)
    parser.add_argument("--preview", type=Path)
    parser.add_argument("--contact", type=Path)
    args = parser.parse_args()

    if args.preview:
        create_preview((36, 44, 52, 60, 68), args.preview)
        return
    if args.contact:
        create_contact_sheet(args.threshold, args.contact)
        return
    generate(args.threshold)


if __name__ == "__main__":
    main()
