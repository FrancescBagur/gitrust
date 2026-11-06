#!/usr/bin/env python3
"""Makes the icons and the banner out of the art in `source/`.

The art arrived as three JPEGs with a checkerboard painted into them --
the pattern a tool draws to *say* "transparent", flattened into pixels by
the time it reached here. So the transparency has to be cut rather than
read, and this is where that cutting is written down, instead of being a
thing somebody once did in an editor that nobody can repeat.

Two different cuts, because the two pictures are nothing alike:

* The icon is a rounded plaque on a light checkerboard. Its silhouette is
  a shape, so it is cut as one: a rounded rectangle measured off the
  pixels, which gives an edge with no trace of the JPEG's fringe on it.
* The mark is an eye, a word and a line of dots on a dark checkerboard.
  Its silhouette is anything but a shape, so it is cut by colour: the
  checkerboard is pure grey and every part of the drawing is not.

It needs `ffmpeg`, which reads and writes the pixels, and nothing else.
The resizing is done here rather than by ffmpeg because `scale` knows
nothing about transparency and blends the clear pixels outside the art
into the edge, which comes out as a dark hem at the small sizes.

    python3 assets/make.py
"""

import pathlib
import struct
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE / "source"

# The plaque's place in source/icon.jpg, in pixels, measured rather than
# guessed: left, top, right, bottom, and how round the corners are. Redo
# these if the art is ever redrawn.
PLAQUE = (400, 70, 1007, 698)
ROUNDING = 105 / 608

# What the installed icon is wanted at. 512 is the one the desktop scales
# from when it has nothing better; the rest are cut on purpose, because an
# icon shrunk to 16 by somebody else's filter is a smudge.
SIZES = (512, 256, 128, 64, 48, 32, 16)


def read(path):
    """A JPEG, as width, height and a run of RGB bytes."""
    size = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0",
         "-show_entries", "stream=width,height", "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True).stdout.strip()
    width, height = (int(n) for n in size.split(","))
    pixels = subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-i", str(path),
         "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True).stdout
    assert len(pixels) == width * height * 3, "ffmpeg gave back the wrong size"
    return width, height, pixels


def write(path, width, height, rgba):
    """A run of RGBA bytes, as a PNG."""
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y",
         "-f", "rawvideo", "-pix_fmt", "rgba", "-s", f"{width}x{height}",
         "-i", "-",
         # A quarter off the file for nothing: the pixels are the same ones,
         # filtered row by row before they are deflated. Bronze is a
         # photograph as far as a PNG is concerned, and photographs are what
         # the plain encoder is worst at.
         "-pred", "mixed", "-compression_level", "100", str(path)],
        input=bytes(rgba), check=True)
    print(f"  {path.relative_to(HERE.parent)}  {width}x{height}")


def rounded(x, y, half_w, half_h, radius):
    """How much of the pixel at x, y the rounded rectangle covers, 0 to 1.

    The distance to the shape is worked out and turned into coverage by
    the half-pixel either side of the edge. That is the usual way of
    drawing a rounded rectangle smoothly, and it is exact enough that
    nothing here needs to be drawn several times over and averaged.
    """
    qx = abs(x) - half_w + radius
    qy = abs(y) - half_h + radius
    outside = (max(qx, 0.0) ** 2 + max(qy, 0.0) ** 2) ** 0.5
    distance = outside + min(max(qx, qy), 0.0) - radius
    return min(max(0.5 - distance, 0.0), 1.0)


def box(source, width, height, out_w, out_h):
    """Shrinks premultiplied RGBA by averaging whole boxes of it.

    Averaging in premultiplied form is the whole point: a clear pixel
    carries no colour into the average, so the edge of the art does not
    pick up a hem of whatever was lying outside it.
    """
    out = bytearray(out_w * out_h * 4)
    for oy in range(out_h):
        y0, y1 = oy * height // out_h, max((oy + 1) * height // out_h, oy * height // out_h + 1)
        for ox in range(out_w):
            x0, x1 = ox * width // out_w, max((ox + 1) * width // out_w, ox * width // out_w + 1)
            r = g = b = a = n = 0
            for y in range(y0, y1):
                row = y * width
                for x in range(x0, x1):
                    i = (row + x) * 4
                    r += source[i]
                    g += source[i + 1]
                    b += source[i + 2]
                    a += source[i + 3]
                    n += 1
            i = (oy * out_w + ox) * 4
            out[i] = r // n
            out[i + 1] = g // n
            out[i + 2] = b // n
            out[i + 3] = a // n
    return out


def unpremultiply(data):
    """Back to plain RGBA, which is what a PNG holds."""
    out = bytearray(data)
    for i in range(0, len(out), 4):
        a = out[i + 3]
        if a == 0:
            out[i] = out[i + 1] = out[i + 2] = 0
        elif a < 255:
            for c in range(3):
                out[i + c] = min(255, out[i + c] * 255 // a)
    return out


def icon():
    """The plaque, cut to its own silhouette, at every size asked for."""
    width, height, rgb = read(SOURCE / "icon.jpg")
    left, top, right, bottom = PLAQUE
    cut_w, cut_h = right - left + 1, bottom - top + 1

    # The plaque is a few pixels taller than it is wide, so it is drawn
    # into a square: a stretch of three per cent, which nobody will see,
    # against an icon that is off-centre at every size, which everybody
    # would.
    side = cut_h
    half = side / 2.0
    radius = ROUNDING * side
    # Pulled in by a pixel so that no scrap of the checkerboard survives
    # along the edge, where the JPEG has blurred the two together.
    half -= 1.0

    premultiplied = bytearray(side * side * 4)
    for y in range(side):
        sy = top + min(cut_h - 1, int(y * cut_h / side))
        for x in range(side):
            sx = left + min(cut_w - 1, int(x * cut_w / side))
            coverage = rounded(x + 0.5 - side / 2.0, y + 0.5 - side / 2.0,
                               half, half, radius)
            s = (sy * width + sx) * 3
            i = (y * side + x) * 4
            alpha = int(coverage * 255 + 0.5)
            premultiplied[i] = rgb[s] * alpha // 255
            premultiplied[i + 1] = rgb[s + 1] * alpha // 255
            premultiplied[i + 2] = rgb[s + 2] * alpha // 255
            premultiplied[i + 3] = alpha

    for size in SIZES:
        small = box(premultiplied, side, side, size, size)
        name = "icon.png" if size == max(SIZES) else f"icon-{size}.png"
        write(HERE / name, size, size, unpremultiply(small))


# Below this the drawing has started; above it there is only checkerboard.
# The checkerboard is grey to the byte and the drawing never is, so the
# two are told apart by how far a pixel's channels are from each other.
CLEAR = 25
SOLID = 60


def mark():
    """The eye and the word, cut off the checkerboard by colour alone."""
    width, height, rgb = read(SOURCE / "logo.jpg")

    alpha = bytearray(width * height)
    for i in range(width * height):
        r, g, b = rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]
        spread = max(r, g, b) - min(r, g, b)
        if spread >= SOLID:
            alpha[i] = 255
        elif spread > CLEAR:
            alpha[i] = (spread - CLEAR) * 255 // (SOLID - CLEAR)

    # Trimmed to what is actually drawn, with a little air around it.
    rows = [y for y in range(height) if any(alpha[y * width:(y + 1) * width])]
    columns = [x for x in range(width)
               if any(alpha[y * width + x] for y in range(rows[0], rows[-1] + 1))]
    margin = 12
    left = max(0, columns[0] - margin)
    right = min(width - 1, columns[-1] + margin)
    top = max(0, rows[0] - margin)
    bottom = min(height - 1, rows[-1] + margin)

    cut_w, cut_h = right - left + 1, bottom - top + 1
    out = bytearray(cut_w * cut_h * 4)
    for y in range(cut_h):
        for x in range(cut_w):
            s = ((top + y) * width + left + x)
            i = (y * cut_w + x) * 4
            out[i] = rgb[s * 3]
            out[i + 1] = rgb[s * 3 + 1]
            out[i + 2] = rgb[s * 3 + 2]
            out[i + 3] = alpha[s]
    write(HERE / "logo.png", cut_w, cut_h, out)


def banner():
    """The wide one, which needs no cutting: it is a picture, edge to edge.

    Only made smaller. It is read on a page about nine hundred pixels
    across, and the file it came as is twice that.
    """
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-i", str(SOURCE / "banner.jpg"),
         "-vf", "scale=1400:-1:flags=lanczos", "-q:v", "3",
         str(HERE / "banner.jpg")], check=True)
    print(f"  assets/banner.jpg  1400 wide")


def ico():
    """The Windows one: every size in a single file.

    Nothing is drawn here and nothing is decoded. An .ico is a short
    directory followed by the images themselves, and Windows has taken PNG
    images inside one since Vista -- so the PNGs written above go in
    untouched, byte for byte, and this only writes the directory in front of
    them.

    It is one file rather than seven because that is what gets embedded in
    the .exe: without it, a window and a taskbar button come up wearing the
    blank sheet Windows gives to a program that brought no icon.
    """
    # Biggest first, which is the order Windows reads them in when it is
    # looking for the one nearest the size it wants.
    #
    # 512 is left out, and cannot be put back: the width in the directory is
    # one byte, so the largest it can say is 256 -- which is why 256 itself
    # is written as a zero. A 512 image in here would arrive claiming to be
    # 256 and be drawn at half scale.
    pngs = [(size, (HERE / f"icon-{size}.png").read_bytes())
            for size in SIZES if size <= 256]

    HEADER, ENTRY = 6, 16
    at = HEADER + ENTRY * len(pngs)
    directory = struct.pack("<HHH", 0, 1, len(pngs))
    for size, data in pngs:
        directory += struct.pack(
            "<BBBBHHII",
            # 256 and above is written as a zero: the field is one byte, and
            # 256 does not fit in one byte.
            0 if size >= 256 else size,
            0 if size >= 256 else size,
            0,          # no palette
            0,          # reserved
            1,          # one plane
            32,         # bits a pixel
            len(data),
            at,
        )
        at += len(data)

    out = HERE / "icon.ico"
    out.write_bytes(directory + b"".join(data for _, data in pngs))
    print(f"  assets/icon.ico  {len(pngs)} sizes, {out.stat().st_size // 1024}K")


if __name__ == "__main__":
    if not SOURCE.is_dir():
        sys.exit("there is no assets/source to work from")
    print("making:")
    icon()
    mark()
    banner()
    # After icon(), which writes the files this gathers up.
    ico()
