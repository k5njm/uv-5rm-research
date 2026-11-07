// Headless-Chromium test of web/ (phase 1: planning + FAKE radio only).
//   scripts/dev.sh web-test          (builds the images and manifest first, then runs this)
// Serves a directory holding index.html, builder.js, manifest.json and the built images.
const http = require("http"), fs = require("fs"), path = require("path");
const { chromium } = require("playwright");

const root = process.argv[2];
if (!root) { console.error("usage: node run.cjs SERVED_DIR"); process.exit(2); }
const types = { ".html": "text/html", ".js": "text/javascript", ".json": "application/json" };
const server = http.createServer((req, res) => {
  const f = path.join(root, decodeURIComponent(req.url.split("?")[0]).replace(/^\/+/, "") || "index.html");
  if (!f.startsWith(root) || !fs.existsSync(f)) { res.writeHead(404); return res.end("not found"); }
  res.writeHead(200, { "content-type": types[path.extname(f)] || "application/octet-stream" });
  fs.createReadStream(f).pipe(res);
});

let passed = 0, failed = 0;
const check = (ok, what) => { ok ? passed++ : failed++; console.log(`  ${ok ? "ok  " : "FAIL"} ${what}`); };

(async () => {
  await new Promise(r => server.listen(0, "127.0.0.1", r));
  const base = `http://127.0.0.1:${server.address().port}/`;
  const browser = await chromium.launch();
  const page = await browser.newPage();
  const external = [], errors = [];
  page.on("request", r => { if (!r.url().startsWith(base)) external.push(r.url()); });
  page.on("pageerror", e => errors.push(e.message));
  page.on("console", m => { if (m.type() === "error") errors.push(m.text()); });
  await page.goto(base);
  await page.waitForFunction(() => document.querySelectorAll("#core option").length > 0);

  console.log("page and manifest");
  check((await page.title()).includes("builder"), "title");
  check(await page.locator(".banner").innerText().then(t => /cannot flash a real radio/.test(t)), "banner says it cannot flash a real radio");
  const cores = await page.$$eval("#core option", o => o.map(x => x.value));
  check(cores.includes("default") && cores.includes("bk4819"), `both core variants listed (${cores})`);
  const apps = await page.$$eval("#apps .app", a => a.map(x => x.dataset.id));
  check(apps.includes("hamtools"), `hamtools listed (${apps})`);
  const styles = await page.$$eval('#apps .app[data-id="hamtools"] option', o => o.map(x => x.textContent));
  check(["classic", "d74", "proxx", "ft5d"].every(s => styles.includes(s)), `all four styles offered (${styles})`);
  check(await page.locator("#write").isDisabled(), "write button starts disabled");
  check(typeof await page.evaluate(() => typeof navigator.serial) === "string" && !(await page.evaluate(() => document.documentElement.innerHTML.includes("requestPort"))), "page never requests a serial port");

  check(await page.$eval("#core", s => s.value) === "default", "the default variant is preselected");
  console.log("board gate and validation");
  await page.check('#apps .app[data-id="hamtools"] input');
  check(await page.locator("#write").isDisabled(), "still disabled with no board text");
  await page.fill("#board", "5RHBK");                       // default core wants 5RHNRFB-YP07
  check(await page.locator("#problems").innerText().then(t => /BOARD GATE/.test(t)), "wrong board for the variant shows BOARD GATE");
  check(await page.locator("#write").isDisabled(), "write disabled on a gate error");
  await page.selectOption("#core", "bk4819");               // now 5RHBK is right
  check(!(await page.locator("#problems").innerText().then(t => /BOARD GATE/.test(t))), "matching board and variant clears the gate");
  await page.selectOption("#core", "default");
  await page.fill("#board", "5RHNRFB-YP07");
  check(await page.locator("#write").isEnabled(), "write enabled for matching board + one app");
  check(JSON.parse(await page.locator("#plan").innerText()).length === 1, "plan lists one slot");

  console.log("writing to the fake radio");
  await page.selectOption('#apps .app[data-id="hamtools"] select', "d74");
  await page.click("#write");
  await page.waitForFunction(() => /software radio/.test(document.getElementById("result").textContent));
  const result = await page.locator("#result").innerText();
  check(/slot 0 hamtools: verified/.test(result), "app written to slot 0 and verified by read-back");
  check(/Nothing was sent to hardware/.test(result), "result states nothing went to hardware");
  const sha = await page.evaluate(async () => {
    const m = await (await fetch("manifest.json")).json();
    const want = m.apps.find(a => a.id === "hamtools").styles.d74.sha256;
    const got = await window.Builder.sha256Hex(window.__fakeRadio.readSlot(0));
    return want === got;
  });
  check(sha, "fake radio slot 0 holds exactly the d74 build from the manifest");

  console.log("logic edge cases (Builder.validate / writePlan in the browser)");
  const edge = await page.evaluate(async () => {
    const m = await (await fetch("manifest.json")).json();
    const V = window.Builder.validate, out = {};
    const one = { core: "default", board: "5RHNRFB-YP07", apps: [{ id: "hamtools", style: "classic" }] };
    out.ok = V(m, one).ok;
    out.tooManyApps = V(m, { ...one, apps: Array.from({ length: m.limits.slots + 1 }, () => ({ id: "hamtools", style: "classic" })) }).errors.join("|");
    const fat = JSON.parse(JSON.stringify(m)); fat.apps[0].styles.classic.segments[0].arena_use = m.limits.arena_bytes - 10;
    out.arena = V(fat, one).errors.join("|");
    const abi = JSON.parse(JSON.stringify(m)); abi.apps[0].styles.classic.abi_hash = "00000000";
    out.abi = V(abi, one).errors.join("|");
    const big = JSON.parse(JSON.stringify(m)); big.apps[0].styles.classic.package_bytes = m.limits.slot_bytes + 1;
    out.slot = V(big, one).errors.join("|");
    const tight = JSON.parse(JSON.stringify(m)); tight.core.find(c => c.id === "default").headroom_bytes = 10;
    out.headroom = V(tight, one).errors.join("|");
    out.noStyle = V(m, { ...one, apps: [{ id: "hamtools", style: "nope" }] }).errors.join("|");
    // a tampered download must be refused before anything is written to the slot
    const plan = V(m, one), radio = new window.Builder.FakeRadio(m.limits.slots, m.limits.slot_bytes);
    try { await window.Builder.writePlan(plan, async () => new Uint8Array([1, 2, 3]).buffer, radio); out.tamper = "WRITTEN"; }
    catch (e) { out.tamper = e.message; }
    out.tamperLog = radio.log.length;
    return out;
  });
  check(edge.ok === true, "valid selection passes");
  check(/slots/.test(edge.tooManyApps), "more apps than slots is an error");
  check(/arena spare/.test(edge.arena), "arena below the spare floor is an error");
  check(/ABI hash/.test(edge.abi), "ABI hash mismatch is an error");
  check(/exceeds/.test(edge.slot), "package larger than a slot is an error");
  check(/headroom/.test(edge.headroom), "core headroom below the floor is an error");
  check(/no build for style/.test(edge.noStyle), "missing style build is an error");
  check(/hash does not match/.test(edge.tamper) && edge.tamperLog === 0, "tampered download refused before any write");

  console.log("hygiene");
  check(external.length === 0, `no requests outside the page's own server (${external.join(",") || "none"})`);
  check(errors.length === 0, `no console or page errors (${errors.join(" | ") || "none"})`);

  await browser.close(); server.close();
  console.log(`\n${passed} passed, ${failed} failed`);
  process.exit(failed ? 1 : 0);
})().catch(e => { console.error(e); process.exit(1); });
