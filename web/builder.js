// Web builder, phase 1: choose a core variant and apps, validate the budget, "write" to a FAKE radio.
// UNVERIFIED: written without a browser test (needs Node/Playwright approval). There is deliberately no
// real serial access anywhere in this file; test_web_static.py pins that.
(function (root) {
  "use strict";

  // ---- pure logic (no DOM), so it can be unit-tested under Node later --------------------------------

  /** selection = { core: "default", board: "5RHNRFB-YP07", apps: [{ id, style }] } */
  function validate(manifest, selection) {
    const L = manifest.limits, errors = [], warnings = [], slots = [];
    const core = manifest.core.find(c => c.id === selection.core);
    if (!core) errors.push("Pick a core variant.");
    else {
      if (!core.vectors_ok) errors.push("Core image failed the vector-table check.");
      if (core.headroom_bytes < L.min_core_headroom)
        errors.push(`Core headroom ${core.headroom_bytes} B is below the ${L.min_core_headroom} B floor.`);
      if (selection.board !== undefined && selection.board !== "" && !core.boards.includes(selection.board))
        errors.push(`BOARD GATE: variant "${core.id}" supports ${core.boards.join(", ")}, not "${selection.board}".`);
      if (!selection.board) warnings.push("Type the text your radio shows when you hold 8 while powering on.");
    }
    if (selection.apps.length > L.slots)
      errors.push(`${selection.apps.length} apps selected but the radio has ${L.slots} slots.`);
    const seen = new Set();
    selection.apps.forEach((sel, i) => {
      const app = manifest.apps.find(a => a.id === sel.id);
      const build = app && app.styles[sel.style];
      if (!build) { errors.push(`${sel.id}: no build for style "${sel.style}".`); return; }
      if (seen.has(sel.id)) errors.push(`${sel.id} selected twice.`);
      seen.add(sel.id);
      if (build.abi_hash !== manifest.abi_hash) errors.push(`${sel.id}: ABI hash ${build.abi_hash} does not match the firmware (${manifest.abi_hash}).`);
      if (build.package_bytes > L.slot_bytes) errors.push(`${sel.id}: package ${build.package_bytes} B exceeds the ${L.slot_bytes} B slot.`);
      build.segments.forEach((s, k) => {
        if (L.arena_bytes - s.arena_use < L.min_arena_spare)
          errors.push(`${sel.id} segment ${k}: arena spare ${L.arena_bytes - s.arena_use} B is below the ${L.min_arena_spare} B floor.`);
      });
      slots.push({ slot: i, app: sel.id, style: sel.style, file: build.file, sha256: build.sha256, bytes: build.package_bytes });
    });
    return { ok: errors.length === 0, errors, warnings, slots };
  }

  /** A software radio with the app slots only. It is the ONLY write target in phase 1. */
  class FakeRadio {
    constructor(slotCount, slotBytes) {
      this.slotBytes = slotBytes;
      this.slots = Array.from({ length: slotCount }, () => new Uint8Array(0));
      this.log = [];
    }
    eraseSlot(n) { this.slots[n] = new Uint8Array(0); this.log.push(`erase ${n}`); }
    writeSlot(n, bytes) {
      if (bytes.length > this.slotBytes) throw new Error(`slot ${n}: ${bytes.length} B > ${this.slotBytes} B`);
      this.slots[n] = Uint8Array.from(bytes); this.log.push(`write ${n} ${bytes.length}`);
    }
    readSlot(n) { return this.slots[n]; }
  }

  async function sha256Hex(bytes) {
    const d = await crypto.subtle.digest("SHA-256", bytes);
    return Array.from(new Uint8Array(d)).map(b => b.toString(16).padStart(2, "0")).join("");
  }

  /** Writes the plan to the fake radio and verifies each slot by reading it back and hashing it. */
  async function writePlan(plan, fetchBytes, radio) {
    const report = [];
    for (const s of plan.slots) {
      const bytes = new Uint8Array(await fetchBytes(s.file));
      if (await sha256Hex(bytes) !== s.sha256) throw new Error(`${s.file}: hash does not match the manifest; nothing written for it`);
      radio.eraseSlot(s.slot);
      radio.writeSlot(s.slot, bytes);
      const back = await sha256Hex(radio.readSlot(s.slot));
      report.push({ slot: s.slot, app: s.app, verified: back === s.sha256 });
      if (back !== s.sha256) throw new Error(`slot ${s.slot}: read-back differs`);
    }
    return report;
  }

  root.Builder = { validate, FakeRadio, writePlan, sha256Hex };

  // ---- UI -------------------------------------------------------------------------------------------
  if (typeof document === "undefined") return;
  const $ = id => document.getElementById(id);
  let manifest = null;
  const radio = () => (root.__fakeRadio = root.__fakeRadio || new FakeRadio(manifest.limits.slots, manifest.limits.slot_bytes));

  function selection() {
    return {
      core: $("core").value,
      board: $("board").value.trim(),
      apps: [...document.querySelectorAll("#apps .app")].filter(r => r.querySelector("input").checked)
        .map(r => ({ id: r.dataset.id, style: r.querySelector("select").value })),
    };
  }

  function render() {
    const v = validate(manifest, selection());
    $("problems").innerHTML = v.errors.map(e => `<li class="err">${e}</li>`).concat(v.warnings.map(w => `<li class="warn">${w}</li>`)).join("");
    $("plan").textContent = JSON.stringify(v.slots, null, 2);
    $("write").disabled = !(v.ok && v.slots.length && selection().board);
    return v;
  }

  function load(m) {
    manifest = m;
    $("core").innerHTML = m.core.map(c => `<option value="${c.id}">${c.id} (${c.rf_chip}; board ${c.boards.join(" / ")})</option>`).join("");
    $("apps").innerHTML = m.apps.map(a => `<div class="app" data-id="${a.id}"><label><input type="checkbox"> ${a.id}</label>
      <select>${Object.keys(a.styles).map(s => `<option>${s}</option>`).join("")}</select></div>`).join("");
    document.querySelectorAll("#apps input, #apps select, #core, #board").forEach(e => e.addEventListener("input", render));
    render();
  }

  async function onWrite() {
    const v = render();
    if (!v.ok) return;
    $("result").textContent = "Writing to the FAKE radio...";
    try {
      const rep = await writePlan(v, f => fetch(f).then(r => r.arrayBuffer()), radio());
      $("result").textContent = rep.map(r => `slot ${r.slot} ${r.app}: ${r.verified ? "verified" : "FAILED"}`).join("\n")
        + "\n\nThis was a software radio. Nothing was sent to hardware.";
    } catch (e) { $("result").textContent = "Stopped: " + e.message; }
  }

  document.addEventListener("DOMContentLoaded", () => {
    $("write").addEventListener("click", onWrite);
    $("manifestFile").addEventListener("change", async e => load(JSON.parse(await e.target.files[0].text())));
    fetch("manifest.json").then(r => r.json()).then(load).catch(() => { $("problems").innerHTML = '<li class="warn">No manifest.json next to this page: load one with the file picker (scripts/dev.sh manifest writes build-out/manifest.json).</li>'; });
  });
})(typeof window !== "undefined" ? window : globalThis);
