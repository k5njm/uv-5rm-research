"""Webcam frame -> 160x128 LCD image, and a tolerant comparison with a simulator golden.

Pillow + numpy only. The LCD is found from four corner points you calibrate once
(calibration.json: corners in the order top-left, top-right, bottom-right, bottom-left, in
webcam pixels). A webcam sees moire, glare and colour shifts, so comparison is on blurred
luminance (Pearson correlation), not exact pixels.
"""
import json
import numpy as np
from PIL import Image, ImageFilter

W, H = 160, 128


def _coeffs(dst, src):
    """Perspective coefficients mapping output (dst) pixels back to input (src) pixels."""
    A, b = [], []
    for (x, y), (u, v) in zip(dst, src):
        A.append([x, y, 1, 0, 0, 0, -u * x, -u * y]); b.append(u)
        A.append([0, 0, 0, x, y, 1, -v * x, -v * y]); b.append(v)
    return np.linalg.solve(np.array(A, float), np.array(b, float)).tolist()


def rectify(frame, corners):
    """Warp the quadrilateral `corners` of a webcam `frame` into a 160x128 image."""
    dst = [(0, 0), (W, 0), (W, H), (0, H)]
    return frame.convert("RGB").transform((W, H), Image.PERSPECTIVE, _coeffs(dst, corners), Image.BICUBIC)


def _luma(img, blur=1.2):
    g = img.convert("L").filter(ImageFilter.GaussianBlur(blur))
    a = np.asarray(g, dtype=float)
    return (a - a.mean()) / (a.std() + 1e-6)


def _chan(img, c, blur=1.2):
    a = np.asarray(img.convert("RGB").getchannel(c).filter(ImageFilter.GaussianBlur(blur)), dtype=float)
    return (a - a.mean()) / (a.std() + 1e-6)


def similarity(a, b):
    """Worst Pearson correlation over the R, G and B channels of the blurred images.

    1.0 = identical. Taking the minimum over channels (not just luminance) makes colour-only
    changes visible, e.g. a running stopwatch's green digits vs the stopped white ones.
    A channel that is flat in one image but not the other scores 0, which is correct."""
    return min(float((_chan(a, c) * _chan(b, c)).mean()) for c in "RGB")


def load_corners(path):
    with open(path) as fh:
        c = json.load(fh)["corners"]
    assert len(c) == 4, "calibration needs 4 corners"
    return [tuple(p) for p in c]


def synthetic_camera(golden, frame_size=(640, 480), corners=None, seed=0, noise=6.0, gain=0.8, blur=0.8):
    """What a webcam might see: the golden placed in a perspective quad with glare, noise, blur.
    Returns (frame, corners). Used to test the pipeline without a camera."""
    rng = np.random.default_rng(seed)
    corners = corners or [(170, 110), (470, 135), (455, 330), (160, 305)]
    fw, fh = frame_size
    bg = np.full((fh, fw, 3), 30, float)
    # forward warp = inverse of rectify
    src = [(0, 0), (W, 0), (W, H), (0, H)]
    big = Image.new("RGB", frame_size, (30, 30, 30))
    warped = golden.convert("RGB").transform(frame_size, Image.PERSPECTIVE, _coeffs(corners, src), Image.BICUBIC)
    mask = Image.new("L", (W, H), 255).transform(frame_size, Image.PERSPECTIVE, _coeffs(corners, src))
    big.paste(warped, (0, 0), mask)
    a = np.asarray(big, float) * gain + 12                      # dimmer, lifted blacks
    yy, xx = np.mgrid[0:fh, 0:fw]
    a += 25 * np.exp(-(((xx - 330) / 90) ** 2 + ((yy - 150) / 60) ** 2))[..., None]  # glare
    a += rng.normal(0, noise, a.shape)
    img = Image.fromarray(np.clip(a, 0, 255).astype(np.uint8))
    return img.filter(ImageFilter.GaussianBlur(blur)), corners
