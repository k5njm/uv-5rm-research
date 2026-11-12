#!/usr/bin/env python3
"""Replay a simulator scenario on a real radio (or a dry run) and compare screens to the goldens.

  hil_run.py SCENARIO.txt --golden DIR --calibration cal.json --injector /dev/ttyACM0 --camera /dev/video0
  hil_run.py SCENARIO.txt --golden DIR --dry-run            # synthetic camera + fake injector

Keys go through the injector (never PTT). `shot` grabs a webcam frame, rectifies it with the
calibration and requires similarity >= --threshold against the golden. The scenario's `app` step
is not automated (menu layout differs per radio): open the app yourself and pass --assume-open.
Exit code 0 = all shots matched.
"""
import argparse, os, subprocess, sys, tempfile, time
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import injector as inj
import scenario as sc
import vision


def grab_webcam(device, out):
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "v4l2", "-i", device,
                    "-frames:v", "5", "-update", "1", out], check=True, timeout=30)  # skip warm-up frames
    return Image.open(out)


def run(steps, golden_dir, name, injector, get_frame, corners, threshold, out_dir, allow_tx=False, sleep=time.sleep):
    if sc.uses_tx(steps) and not allow_tx:
        raise SystemExit("scenario transmits (`allow tx`); refusing without --allow-tx")
    skipped = sc.sim_only(steps)
    results = []
    for s in steps:
        if s.op in ("key", "hold"):
            injector.press(s.args[0], 120 if s.op == "key" else 900)
        elif s.op == "wait":
            sleep(float(s.args[0]))
        elif s.op == "shot":
            frame = get_frame(s.args[0])
            lcd = vision.rectify(frame, corners)
            os.makedirs(out_dir, exist_ok=True)
            lcd.save(os.path.join(out_dir, f"{s.args[0]}.png"))
            want = Image.open(os.path.join(golden_dir, name, f"{s.args[0]}.png"))
            score = vision.similarity(lcd, want)
            results.append((s.args[0], score, score >= threshold))
    return results, skipped


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scenario")
    ap.add_argument("--golden", required=True)
    ap.add_argument("--out", default=tempfile.mkdtemp(prefix="hil-"))
    ap.add_argument("--calibration")
    ap.add_argument("--injector")
    ap.add_argument("--camera", default="/dev/video0")
    ap.add_argument("--threshold", type=float, default=0.80)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--assume-open", action="store_true", help="the app is already open on the radio")
    ap.add_argument("--allow-tx", action="store_true")
    a = ap.parse_args()
    name = os.path.splitext(os.path.basename(a.scenario))[0]
    with open(a.scenario) as fh:
        steps = sc.parse(fh.read())
    if not a.dry_run and not a.assume_open:
        raise SystemExit("open the app on the radio, then pass --assume-open (menu navigation is not automated)")
    if a.dry_run:
        injector = inj.FakeInjector()
        corners = None

        def get_frame(shot):
            nonlocal corners
            frame, corners = vision.synthetic_camera(Image.open(os.path.join(a.golden, name, f"{shot}.png")))
            return frame
        # corners are known to the synthetic camera: calibrate from its first frame
        first = next(s for s in steps if s.op == "shot")
        _, corners = vision.synthetic_camera(Image.open(os.path.join(a.golden, name, f"{first.args[0]}.png")))
    else:
        injector = inj.SerialInjector(a.injector)
        corners = vision.load_corners(a.calibration)
        tmp = os.path.join(a.out, "_cam.png")
        os.makedirs(a.out, exist_ok=True)

        def get_frame(shot):
            return grab_webcam(a.camera, tmp)
    try:
        results, skipped = run(steps, a.golden, name, injector, get_frame, corners, a.threshold, a.out, a.allow_tx,
                               sleep=(lambda s: None) if a.dry_run else time.sleep)
    finally:
        injector.close()
    for shot, score, ok in results:
        print(f"  {'ok  ' if ok else 'FAIL'} {shot}: similarity {score:.3f} (need {a.threshold})")
    for s in skipped:
        print(f"  note: line {s.line} `{s.op} {' '.join(s.args)}` is simulator-only and was not checked")
    bad = [r for r in results if not r[2]]
    print(f"{len(results) - len(bad)} matched, {len(bad)} did not; frames in {a.out}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
