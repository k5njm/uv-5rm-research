# Baofeng UV-5RM Custom Firmware: UI/UX Audit & Agent Handoff

**Target Hardware:** Baofeng UV-5RM (`5RHNRFB-YP07`, AT32F421 MCU, ST7735 160×128 RGB565 LCD)  
**Audit Scope:** UI/UX concepts in `build-out/concept/`, shared list widget (`list.rs`), Channel Manager (`chanmgr`), Settings (`settings`), Standby/Core screens, and specialized apps.  
**Benchmark:** Transition from "budget Chinese radio UI" to "premium handheld transceiver" (Radio Foundry / Kenwood TH-D74 / Yaesu FT-5D aesthetic).

---

## 1. Executive Summary

A recent push overhauled the radio's shared list widget, introduced human-readable menu names, added a bottom key-hint strip, introduced channel groups, and drafted safety guards for settings reset and channel deletion. Concepts generated in `build-out/concept/` capture these iterations.

While the new design language (clean header, blue navigation cursor, amber editing state, bottom key hints) is a massive leap forward from the stock firmware, the concept screens reveal **critical safety vulnerabilities, visual inconsistencies, misleading navigational affordances, and text rendering glitches** that must be resolved before this UI can be considered production-ready.

### High-Priority Findings
1. **Destructive Action Guards are Inadequate:** A single accidental press of `MENU` while on `RESET ALL` or `DELETE` executes permanent data erasure. Pocket-presses or double-taps can wipe the radio or destroy channel memories.
2. **Deceptive Stepper Arrows (`^ ... v`):** Non-steppable fields (such as `RESET ALL` and `LATITUDE`) display up/down arrow glyphs (`^` and `v`), misrepresenting navigation behavior.
3. **Inconsistent State & Color Language:** Armed destructive states flip unpredictably between Amber (in `01`) and Blue (in `03` and `x-settings_system_guards-reset_armed.png`). Blue should strictly denote *cursor selection*, while Amber/Red must denote *active/armed state*.
4. **Rendering Glitch in Text Input:** In `x-chanmgr_name_stock_blank-typed_k5r.png`, the amber editing highlight is split by a black gap due to an alignment mismatch in `draw_cursor_box` in `list.rs`.
5. **No Unsaved Changes Protection:** Pressing `EXIT` while configuring a channel or setting silently discards all changes with zero confirmation.
6. **No Scroll / Position Affordance:** Lists display 7 items at a time with neither a scrollbar track nor an index counter (`1/14`), hiding off-screen menu options.

---

## 2. Screen-by-Screen Concept Audit (`build-out/concept/`)

### 2.1. `01-settings_system_guards-reset_armed.png` & `02-settings_system_guards-reset_row.png`
* **Context:** `SETTINGS > SYSTEM > RESET ALL` (Armed state vs Selected row).
* **Observed UI:**
  - `01`: Amber row `RESET ALL   ^ MENU = WIPE v`, bottom hint `MENU wipe all  EXIT cancel`.
  - `02`: Blue row `RESET ALL`, bottom hint `MENU change  EXIT back`.
* **Critique & Flaws:**
  - **Misleading Arrows (`^ ... v`):** The `^` and `v` arrows indicate that `UP`/`DOWN` will cycle options. In reality, `UP`/`DOWN` do nothing in this armed state.
  - **Redundant Hint:** The row text repeats `MENU = WIPE` while the bottom bar repeats `MENU wipe all`. 
  - **Informal Terminology:** "WIPE" is computer slang. Radios use "RESET".
  - **Unsafe Trigger:** A single `MENU` click triggers complete memory reformat (`first_boot_format`).
* **Recommendation:**
  - In `02`, change hint to `MENU reset   EXIT back`.
  - In `01`, drop `^` and `v`. Use a timed hold guard: row displays `HOLD MENU (2s) TO RESET` or opens a modal dialog box: `RESET RADIO? MENU=YES EXIT=NO`.

### 2.2. `03-chanmgr_delete_guard-delete_armed.png` & `04-chanmgr_delete_guard-save_row_hint.png`
* **Context:** `CHAN MGR > Channel Detail > SAVE / DELETE` rows.
* **Observed UI:**
  - `03`: Blue row `DELETE     MENU = DELETE`, bottom hint `MENU delete it  EXIT keep`.
  - `04`: Blue row `SAVE`, bottom hint `MENU save    EXIT discard`.
* **Critique & Flaws:**
  - **Color Inconsistency:** In `01`, arming an action colored the row Amber. Here in `03`, arming `DELETE` leaves the row Blue! The operator has no color cue that they have armed a destructive operation.
  - **Colloquial Wording:** `MENU delete it  EXIT keep` sounds unpolished. 
  - **Hazardous Layout Proximity:** `DELETE` is immediately adjacent to `SAVE`. Over-scrolling down past `SAVE` and double-tapping `MENU` deletes the channel instead of saving it.
* **Recommendation:**
  - Armed `DELETE` must turn **Amber** (or Alert Red).
  - Change hint strip to `MENU confirm  EXIT cancel`.
  - Require a long-press (`HOLD MENU 2s`) or distinct key sequence to delete.

### 2.3. `05-chanmgr_name_stock_blank-typed_k5r.png` & `x-chanmgr_name_stock_blank-typed_k5r.png`
* **Context:** `CHAN MGR > Channel Detail > NAME` field editing.
* **Observed UI:**
  - `05`: Blue row `NAME      K5R`, bottom hint `0-9 UP/DN MENU ok EXIT del`. No caret/cursor visible.
  - `x-`: Amber row, but the amber highlight has a visible gap/tear between `K5R` and the right side of the screen.
* **Critique & Flaws:**
  - **Rendering Bug in `list.rs`:** In `draw_value_with_cursor` and `draw_cursor_box`, the cursor coordinate `x0` is computed for right-aligned values (`SCREEN_W - RIGHT_MARGIN - width`), but during editing the selection box fill and cursor background calculation overwrite the row background incorrectly, creating a split black bar.
  - **Lack of Caret in `05`:** Without a visible blinking cursor or block caret, the operator cannot tell character position.
  - **Key Legend Overcrowding:** `0-9 UP/DN MENU ok EXIT del` is 27 characters (overflowing a 26-char 6x10 grid) and mixes verbs (`ok`, `del`).
* **Recommendation:**
  - Fix `list.rs` so editing rows draw a solid `EDIT_BG` rectangle across the full width (`0..SCREEN_W`), then draw a reverse-contrast block cursor at the exact active character slot.
  - Format hint as `0-9 CHAR  MENU OK  EXIT BACKSPACE`.

### 2.4. `06-settings_radio_items-radio_items.png` & `08-settings_system_items-system_items.png`
* **Context:** Settings sub-menus (`RADIO` and `SYSTEM`).
* **Observed UI:**
  - `06`: `SQUELCH 0`, `FREQ STEP 2.50k`, `TX TIMEOUT OFF`, etc. Hint: `MENU change  EXIT back`.
  - `08`: `TX BANDS CE HAM`, `LATITUDE 0.0000N`, `BATTERY CAL 0.00V`, `VERSION UV-5RM OPEN FW`.
* **Critique & Flaws:**
  - **No Scroll Affordance:** The list contains 12+ items, but only 7 fit on screen. There is no scrollbar, no up/down indicator arrow in the title bar, and no index (`1/14`).
  - **Ambiguous Battery Calibration:** `BATTERY CAL  0.00V` looks like a dead battery (0.00V) rather than an offset value.
  - **Unit Inconsistencies:** `2.50k` uses a lowercase `k`, whereas all other units use uppercase (`SEC`, `V`, `HIGH`, `WIDE`).
  - **Version String Behavior:** Pressing `MENU` on `VERSION` silently toggles between two hardcoded text strings in place instead of showing an About/Firmware Information screen.
* **Recommendation:**
  - Add a 2-pixel scrollbar thumb on the right edge or display item indices (e.g., `01/14`) in the title bar.
  - Format `BATTERY CAL` as `OFFSET +0.00V` or `CAL: 0.00V`.
  - Use uppercase `2.50K` or `2.5 kHz`.

### 2.5. `07-settings_system_items-lat_editing.png` & `x-lat.png`
* **Context:** `SETTINGS > SYSTEM > LATITUDE` editing mode.
* **Observed UI:**
  - `07`: Amber row `LATITUDE  ^ --.----N v`, hint `0-9  * N/S E/W  MENU ok`.
  - `x-lat.png`: Amber row without `^` and `v` arrows.
* **Critique & Flaws:**
  - **False Navigation Cues in `07`:** The arrows `^` and `v` falsely implied `UP`/`DOWN` stepped the coordinate. `x-lat.png` correctly removed them.
  - **Geographically Nonsensical Hint:** For `LATITUDE`, the hint says `* N/S E/W`. Latitude can only be North or South; East/West applies only to Longitude.
  - **Format Expectation:** Amateur radio operators almost universally use Maidenhead Grid Squares (e.g., `EM12tr`) or APRS standard (`DD MM.MM`). Entering 6 decimal digits is tedious on a radio keypad.
* **Recommendation:**
  - Contextualize hint: `0-9 DIGIT  * N/S  MENU OK` for Latitude; `0-9 DIGIT  * E/W  MENU OK` for Longitude.
  - Future improvement: Support direct Maidenhead Grid Locator entry (translating to coordinates automatically).

---

## 3. Systematic UI/UX Comparison & Design Rules

To achieve a cohesive "Radio Foundry / Premium Transceiver" experience, adopt the following strict rules:

| Element | Current Inconsistent Behavior | Standardized Rule |
|---|---|---|
| **Selection Bar** | Blue for normal selection; sometimes used for armed delete | **`#1E64C8` (Blue)** strictly indicates **cursor navigation** |
| **Editing / Armed Bar** | Amber in `01` & `07`, Blue in `03` and `x-reset` | **`#FFAA00` (Amber)** strictly indicates **active value change or armed action** |
| **Alert / Warning** | Red toast on standby; red header on TEXT and BANDSWEEP | **`#C82020` (Red)** reserved strictly for **transmitting alerts, prohibited TX, and destructive confirmations** |
| **Channel Naming** | `M001` (Standby) vs `234` (ChanMgr List) vs `CH002` (Editor) | Unified format: **`CH 001`** across all screens, editor titles, and toasts |
| **Key Hint Verbs** | Mixed: `open`, `edit`, `change`, `save`, `ok`, `del`, `wipe`, `keep`, `discard` | Standardized verb set:<br>• `MENU SELECT` / `EXIT BACK` (Browsing)<br>• `MENU EDIT` / `EXIT BACK` (List items)<br>• `MENU SAVE` / `EXIT CANCEL` (Editing value)<br>• `MENU CONFIRM` / `EXIT CANCEL` (Guarded actions) |
| **Scroll Indicator** | Completely absent | Title bar or right rail must show scroll progress (e.g., `(01/14)` or 2px scroll thumb) |
| **Unsaved Edits** | Pressing `EXIT` silently discards edits | If edited fields are dirty, `EXIT` prompts: `DISCARD? MENU=YES EXIT=NO` |

---

## 4. Prioritized Engineering Recommendations

### Priority 1: Prevent Accidental Data Loss & Radio Wiping (Safety Critical)
- [ ] **Modal Confirmation for `RESET ALL`:**
  - Remove instant execution on `MENU`.
  - Require holding `MENU` for 2 seconds with an on-screen countdown: `HOLD MENU (2s) TO RESET`, or display an explicit two-key confirmation dialog: `FACTORY RESET? [MENU] YES  [EXIT] NO`.
- [ ] **Guarded Channel Deletion:**
  - Set row background to Amber/Red when `DELETE` is selected.
  - Separate `DELETE` from `SAVE` (e.g., require pressing `*` or `SIDE2` to access Delete, or place Delete on a separate page).
- [ ] **Unsaved Changes Trap:**
  - In `chanmgr`, track a `dirty: bool` flag. If the user presses `EXIT` from an editor with unsaved changes, show `UNSAVED EDITS - DISCARD? MENU=YES EXIT=NO`.

### Priority 2: Fix List Widget Rendering & Cursor Bugs
- [ ] **Fix Broken Amber Highlight in `firmware/firmware-source/src/ui/list.rs`:**
  - In `draw_row`, ensure `PrimitiveStyle::with_fill(EDIT_BG)` always draws across the full width `[0..SCREEN_W]`.
  - Correct `draw_cursor_box` so that when `editing == true`, the cursor character is drawn as an inverted black block with white text without clearing surrounding background pixels to black.
- [ ] **Add List Position / Scrollbar Indicator:**
  - In `draw_list` (`list.rs`), render either a 1-pixel vertical scroll track on the right margin (`x = SCREEN_W - 1`) or append `(i/N)` to the right side of the title bar when `total > VISIBLE_ROWS`.

### Priority 3: Harmonize Vocabulary and Wording
- [ ] **Unify Channel Identifiers:**
  - Standardize on `CH 001` across Standby screen, `chanmgr` list, `chanmgr` detail header, and toast messages.
- [ ] **Standardize Key Hints in `common.rs` and `chanmgr/src/main.rs`:**
  - Replace `MENU wipe all` / `EXIT keep` / `EXIT del` with standard verbs (`CONFIRM`, `CANCEL`, `BACKSPACE`).
  - Tailor `LATITUDE` / `LONGITUDE` hints to only show valid hemisphere toggles (`* N/S` or `* E/W`).

### Priority 4: App Visual Style Alignment
- [ ] **Standardize Header Bars across Overlay Apps:**
  - Bring `TEXT`, `SPECTRUM`, and `BANDSWEEP` into visual alignment with the main list widget (light grey header with dark text, consistent bottom key-hint strip).
  - Eliminate jarring dark-red title bars unless the radio is actively transmitting.

---

## 5. Artifact Reference Map

| File Path | Description | Current State |
|---|---|---|
| `build-out/concept/01-settings_system_guards-reset_armed.png` | Reset armed concept with stepper arrows | Superseded by `x-settings_system_guards-reset_armed.png` |
| `build-out/concept/02-settings_system_guards-reset_row.png` | Reset row selected (unarmed) | Needs hint update (`MENU reset`) |
| `build-out/concept/03-chanmgr_delete_guard-delete_armed.png` | Channel delete armed concept | Needs Amber highlight and formal wording |
| `build-out/concept/04-chanmgr_delete_guard-save_row_hint.png` | Save row selected | Valid; needs dirty-state confirmation |
| `build-out/concept/05-chanmgr_name_stock_blank-typed_k5r.png` | Name field with text typed | Missing cursor caret |
| `build-out/concept/x-chanmgr_name_stock_blank-typed_k5r.png` | Amber name editing concept | Contains background split rendering glitch |
| `build-out/concept/06-settings_radio_items-radio_items.png` | Radio settings list | Needs scrollbar / item counter |
| `build-out/concept/07-settings_system_items-lat_editing.png` | Latitude editing with arrows | Superseded by `x-lat.png` |
| `build-out/concept/x-lat.png` | Latitude editing without arrows | Valid; hint needs `* N/S` only |
| `build-out/concept/08-settings_system_items-system_items.png` | System settings overview | Needs `BATTERY CAL` formatting fix |
