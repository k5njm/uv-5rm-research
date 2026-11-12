r"""Preview, install, or restore a custom 160x128 boot logo.

Examples (run from the ``UV-5RM Custom Firmware`` folder):

  py source-changes\set_boot_logo.py preview photo.png --preview preview.png --fit contain
  py source-changes\set_boot_logo.py upload COM3 photo.png logo-backup.rgb565 --preview preview.png
  py source-changes\set_boot_logo.py restore COM3 logo-backup.rgb565

Uploads back up the logo currently in the radio, skip an identical image, and
read all 40,960 bytes back after writing. If verification fails, the original
logo is restored and verified before the radio restarts.
"""
import argparse
import os
import sys

from PIL import Image, ImageOps


DESCRIPTION = __doc__
HERE = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(HERE, "cps_tool.py"), encoding="utf-8") as cps_file:
    CPS_SOURCE = cps_file.read()
exec(CPS_SOURCE[:CPS_SOURCE.index("def do_logo")])

WIDTH, HEIGHT = 160, 128
EXPECTED_SIZE = WIDTH * HEIGHT * 2
RESAMPLE = getattr(Image, "Resampling", Image).LANCZOS
NEAREST = getattr(Image, "Resampling", Image).NEAREST


def parse_color(value):
    value = value.removeprefix("#")
    if len(value) != 6:
        raise argparse.ArgumentTypeError("background must be RRGGBB or #RRGGBB")
    try:
        return tuple(int(value[i:i + 2], 16) for i in (0, 2, 4))
    except ValueError as exc:
        raise argparse.ArgumentTypeError("background must be RRGGBB or #RRGGBB") from exc


def flatten_image(path, background):
    with Image.open(path) as source:
        image = ImageOps.exif_transpose(source).convert("RGBA")
    base = Image.new("RGBA", image.size, (*background, 255))
    base.alpha_composite(image)
    return base.convert("RGB")


def fit_image(path, fit, background):
    image = flatten_image(path, background)
    if fit == "stretch":
        return image.resize((WIDTH, HEIGHT), RESAMPLE)
    if fit == "cover":
        return ImageOps.fit(image, (WIDTH, HEIGHT), method=RESAMPLE)

    contained = ImageOps.contain(image, (WIDTH, HEIGHT), method=RESAMPLE)
    canvas = Image.new("RGB", (WIDTH, HEIGHT), background)
    canvas.paste(contained, ((WIDTH - contained.width) // 2, (HEIGHT - contained.height) // 2))
    return canvas


def to_rgb565(image):
    raw = bytearray(EXPECTED_SIZE)
    pixels = image.load()
    out = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            r, g, b = pixels[x, y]
            value = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
            raw[out] = value & 0xFF
            raw[out + 1] = value >> 8
            out += 2
    return bytes(raw)


def from_rgb565(raw):
    if len(raw) != EXPECTED_SIZE:
        raise ValueError(f"RGB565 image is {len(raw)} bytes; expected {EXPECTED_SIZE}")
    pixels = []
    for i in range(0, len(raw), 2):
        value = raw[i] | raw[i + 1] << 8
        r = ((value >> 11) & 0x1F) * 255 // 31
        g = ((value >> 5) & 0x3F) * 255 // 63
        b = (value & 0x1F) * 255 // 31
        pixels.append((r, g, b))
    image = Image.new("RGB", (WIDTH, HEIGHT))
    image.putdata(pixels)
    return image


def ensure_parent(path):
    parent = os.path.dirname(os.path.abspath(path))
    os.makedirs(parent, exist_ok=True)


def save_preview(raw, path, scale):
    ensure_parent(path)
    image = from_rgb565(raw)
    if scale != 1:
        image = image.resize((WIDTH * scale, HEIGHT * scale), NEAREST)
    image.save(path)
    print(f"preview: {path} ({image.width}x{image.height})")


def save_raw(raw, path, label="RGB565 image"):
    ensure_parent(path)
    with open(path, "wb") as output:
        output.write(raw)
    print(f"{label}: {path} ({len(raw)} bytes)")


def save_backup(raw, path):
    ensure_parent(path)
    try:
        with open(path, "xb") as output:
            output.write(raw)
        print(f"current logo backed up to {path}")
    except FileExistsError:
        with open(path, "rb") as existing:
            old = existing.read()
        if old != raw:
            raise SystemExit(
                f"REFUSING TO OVERWRITE {path}: it contains a different backup; "
                "choose a new file name"
            )
        print(f"backup already matches current logo: {path}")


def read_logo(cps, label="reading logo"):
    raw = bytearray()
    for off in range(0, EXPECTED_SIZE, CHUNK):
        block = cps.send(READ_LOGO, off)[2]
        if len(block) != CHUNK:
            raise SystemExit(f"SHORT LOGO READ at {off}: got {len(block)}, expected {CHUNK}")
        raw += block
        print(f"\r{label}: {len(raw)}/{EXPECTED_SIZE}", end="", flush=True)
    print()
    return bytes(raw)


def write_logo(cps, raw, label="writing logo"):
    if len(raw) != EXPECTED_SIZE:
        raise SystemExit(f"logo is {len(raw)} bytes; expected {EXPECTED_SIZE}")
    for off in range(0, EXPECTED_SIZE, CHUNK):
        cps.send(
            WRITE_LOGO,
            off,
            raw[off:off + CHUNK],
            ERASE_TIMEOUT if off == 0 else FRAME_TIMEOUT,
        )
        done = off + CHUNK
        print(f"\r{label}: {done}/{EXPECTED_SIZE}", end="", flush=True)
    print()


def finish(cps):
    cps.end()
    print("done; radio restarting")


def make_image(args):
    image = fit_image(args.image, args.fit, args.background)
    raw = to_rgb565(image)
    if args.preview:
        save_preview(raw, args.preview, args.preview_scale)
    if args.raw:
        save_raw(raw, args.raw)
    return raw


def command_preview(args):
    raw = make_image(args)
    if not args.preview and not args.raw:
        raise SystemExit("preview needs --preview, --raw, or both")
    print(f"converted {args.image}: {len(raw)} bytes")


def command_upload(args):
    wanted = make_image(args)
    cps = Cps(args.port)
    try:
        before = read_logo(cps, "reading current logo")
        save_backup(before, args.backup)
        if before == wanted:
            print("requested logo is already installed; no flash write needed")
            finish(cps)
            return

        write_logo(cps, wanted)
        after = read_logo(cps, "verifying logo")
        if after != wanted:
            bad = sum(a != b for a, b in zip(after, wanted))
            print(f"VERIFY FAILED: {bad} bytes differ; restoring the original logo")
            write_logo(cps, before, "restoring original logo")
            restored = read_logo(cps, "verifying restored logo")
            finish(cps)
            if restored != before:
                raise SystemExit(
                    f"RESTORE VERIFY FAILED; the original remains saved in {args.backup}"
                )
            raise SystemExit("upload verification failed; original logo restored")

        print(f"logo written and verified: {EXPECTED_SIZE} bytes")
        finish(cps)
    finally:
        if cps.port.is_open:
            cps.port.close()


def command_restore(args):
    with open(args.backup, "rb") as source:
        backup = source.read()
    if len(backup) != EXPECTED_SIZE:
        raise SystemExit(
            f"backup is {len(backup)} bytes; expected {EXPECTED_SIZE}; nothing was written"
        )

    cps = Cps(args.port)
    try:
        current = read_logo(cps, "reading current logo")
        if current == backup:
            print("backup is already installed; no flash write needed")
            finish(cps)
            return
        if args.current_backup:
            save_backup(current, args.current_backup)

        write_logo(cps, backup, "restoring logo")
        restored = read_logo(cps, "verifying restored logo")
        if restored != backup:
            finish(cps)
            raise SystemExit("RESTORE VERIFY FAILED")
        print(f"logo restored and verified: {EXPECTED_SIZE} bytes")
        finish(cps)
    finally:
        if cps.port.is_open:
            cps.port.close()


def add_image_options(parser, preview_required=False):
    parser.add_argument("image", help="PNG, JPEG, BMP, or another Pillow-readable image")
    parser.add_argument(
        "--fit",
        choices=("contain", "cover", "stretch"),
        default="contain",
        help="contain: letterbox (default); cover: crop; stretch: ignore aspect ratio",
    )
    parser.add_argument(
        "--background",
        type=parse_color,
        default=(0, 0, 0),
        metavar="RRGGBB",
        help="background for transparency and contain mode (default: 000000)",
    )
    parser.add_argument(
        "--preview",
        required=preview_required,
        metavar="OUT.PNG",
        help="save an exact post-RGB565 preview",
    )
    parser.add_argument(
        "--preview-scale",
        type=int,
        choices=range(1, 9),
        default=3,
        metavar="N",
        help="preview enlargement, 1-8 (default: 3)",
    )
    parser.add_argument("--raw", metavar="OUT.rgb565", help="also save the converted raw image")


def build_parser():
    parser = argparse.ArgumentParser(
        description=DESCRIPTION,
        formatter_class=argparse.RawTextHelpFormatter,
    )
    commands = parser.add_subparsers(dest="command", required=True)

    preview = commands.add_parser("preview", help="convert locally without opening a serial port")
    add_image_options(preview)
    preview.set_defaults(run=command_preview)

    upload = commands.add_parser("upload", help="back up, upload, and verify a custom logo")
    upload.add_argument("port", help="serial port, for example COM3")
    add_image_options(upload)
    upload.add_argument("backup", help="new or matching .rgb565 file for the current radio logo")
    upload.set_defaults(run=command_upload)

    restore = commands.add_parser("restore", help="restore and verify a raw logo backup")
    restore.add_argument("port", help="serial port, for example COM3")
    restore.add_argument("backup", help="40,960-byte .rgb565 backup to restore")
    restore.add_argument(
        "--current-backup",
        metavar="OUT.rgb565",
        help="save the currently installed logo before restoring",
    )
    restore.set_defaults(run=command_restore)
    return parser


def main():
    args = build_parser().parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
