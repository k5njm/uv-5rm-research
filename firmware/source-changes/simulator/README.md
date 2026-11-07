# Host simulator and scenario tests

Runs the real app code (`logic.rs`) against a simulated radio, no hardware needed.

| Command | What it does |
|---|---|
| `cargo run --release -- OUTDIR ../../skull_logo.rgb565` | the original built-in test suite (324 checks) |
| `cargo test --release` | unit tests inside app `logic.rs` files (e.g. `hamtools`) |
| `./run_scenarios.sh` | every `tests/scenarios/*.txt` against `tests/golden/` frames |
| `STYLE=d74 ./run_scenarios.sh` | same scenarios for another visual style (`d74`, `proxx`, `ft5d`) against `tests/golden-<style>` |
| `./run_scenarios.sh --update` | rewrite goldens after an intentional UI change; **look at the PNGs first** |

A scenario is a text file of steps (`app`, `key`, `hold`, `wait`, `shot`, `expect`, `coords`; see the
header of `scenario.rs`). Each `shot` is saved as PNG and compared pixel-for-pixel with its golden.
Every scenario also fails if the app keyed PTT or called the keying tone function, unless it says
`allow tx`. Scenarios are written to be replayable on the real radio through `hil/`.

Adding an app: add its `logic.rs` as a `#[path]` module in `main.rs`, register it in `scenario.rs::lookup`,
write scenarios, review the frames, then `--update`.
