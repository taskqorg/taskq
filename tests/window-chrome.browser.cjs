// #1660 A1–A5 (R07–R09, R21–R26): one stable header action row, Home/Back, one Conversations entry, version-only
// footer and the Settings surface; window drag, z-order, hide and raise are native and not measured here.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
const out = process.env.CHROME_EVIDENCE;
const task = (n, state) => ({
  n,
  state,
  title: "Keep the task title readable " + n,
  revision: n + 50,
  since: 900,
  created: 500,
  steps: [{ state, start: 900, end: null, complete: false }],
  controls: ["stop", "later"],
});
const long = {
  root: "/fixture/long",
  name: "Infrastructure provisioning and observability pipeline",
  host: "fixture",
  issues: "https://example.org/org/long/issues",
  board_kind: "github",
  alive: true,
  complete: true,
  age: 1,
  focus: ["all"],
  labels: [],
  tasks: [task(1, "ready"), task(2, "doing")],
  delivery: { preset: "auto-merge", revision: 0 },
};
const state = {
  platform: "windows",
  font: 16,
  theme: "dark",
  host: "fixture",
  now: 1000,
  version: "9.9.9",
  size_anchor: "auto",
  attached_edge: "top",
  top: false,
  pinned: false,
  service: { state: "running" },
  projects: [long],
  ai: { draft: "" },
  notify: { projects: {} },
  stop_modes: false,
  display: require("./fixtures/display-model.json").display,
  messages: { complete: true, count: 3 },
};
// icon::css, the palette the native menu injects
const palette = {
  dark: "--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--review:#BCA3EB",
  light:
    "--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--review:#8250DF",
};
const html = fs.readFileSync("build/web/tray.html", "utf8"),
  settingsHtml = html.replace(
    "<head>",
    "<head><script>window.taskqSurface='settings';</script>",
  );
(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  try {
    for (const theme of ["dark", "light"])
      for (const width of [320, 360, 800]) {
        const page = await browser.newPage({
            viewport: { width, height: 640 },
          }),
          errors = [];
        page.on("pageerror", (e) => errors.push(e.message));
        await page.setContent(html);
        await page.evaluate(
          (s) => {
            window.messages = [];
            window.ipc = { postMessage: (t) => messages.push(JSON.parse(t)) };
            taskq.receive(s);
          },
          { ...state, theme, font: 20, palette: palette[theme] },
        );
        assert.equal(
          await page.locator(".head h1").count(),
          0,
          "F04: Home reserves no heading",
        );
        assert.equal(
          await page.locator(".head .conversations-entry").count(),
          1,
          "F02: one entry",
        );
        assert.equal(
          await page.locator(".head .communication-count").count(),
          2,
          "F03: type counters remain distinct",
        );
        assert.equal(
          await page.locator(".head [data-a=hide]").count(),
          0,
          "F01: native Close replaces application Close",
        );
        assert.deepEqual(
          await page
            .locator(".window-controls>button")
            .evaluateAll((es) => es.map((e) => e.getAttribute("aria-label"))),
          ["Keep menu open", "Always on top"],
        );
        assert.equal(
          await page
            .locator(".menu")
            .evaluate((e) => e.scrollWidth <= e.clientWidth + 1),
          true,
          "wrapping toolbar fits",
        );
        assert.match(
          await page.locator(".status-footer").innerText(),
          /TaskQ\s+v9.9.9/,
        );
        await page.locator(".conversations-entry").click();
        assert(
          await page.evaluate(() =>
            messages.some(
              (m) => m.a === "workspace" && m.open.page === "conversations",
            ),
          ),
        );
        assert.equal(
          await page.evaluate(() =>
            messages.some((m) => /message-action|mark-read|answer/.test(m.a)),
          ),
          false,
          "entry acknowledges nothing",
        );
        await page
          .locator('[data-route="' + long.root + '"]')
          .first()
          .click();
        assert.equal(await page.locator(".head h1").textContent(), long.name);
        assert.equal(
          await page.locator(".breadcrumbs").innerText(),
          "Projects",
        );
        assert.equal(
          await page
            .locator(".head h1")
            .evaluate((e) => e.scrollWidth <= e.clientWidth + 1),
          true,
          "F05: contextual title stays complete",
        );
        const layout = await page.locator(".head").evaluate((e) => {
          const a = e.getBoundingClientRect(),
            h = e.querySelector("h1").getBoundingClientRect(),
            b = e.querySelector(".breadcrumbs").getBoundingClientRect();
          return (
            h.left >= a.left &&
            h.right <= a.right + 1 &&
            b.left >= a.left &&
            b.right <= a.right + 1
          );
        });
        assert(layout, "breadcrumb/title stay inside header");
        await page.locator(".head [data-a=home]").click();
        assert.equal(await page.locator(".head h1").count(), 0);
        await page.locator(".head [data-a=back]").click();
        assert.equal(
          await page.locator(".head h1").textContent(),
          long.name,
          "Back retains project context",
        );
        await page.close();
        const settings = await browser.newPage({
          viewport: { width, height: 640 },
        });
        await settings.setContent(settingsHtml);
        await settings.evaluate(
          (s) => {
            window.messages = [];
            window.ipc = { postMessage: (t) => messages.push(JSON.parse(t)) };
            taskq.receive(s);
          },
          { ...state, theme },
        );
        assert.equal(
          await settings.locator(".head h1").textContent(),
          "Settings",
        );
        assert.equal(await settings.locator(".head [data-a=hide]").count(), 0);
        await settings.evaluate(() =>
          taskq.receive({ settingsOpen: { page: "appearance" } }),
        );
        assert.equal(
          await settings.locator(".head h1").textContent(),
          "Appearance",
        );
        for (const a of ["settings-top", "settings-enlarge"]) {
          await settings.locator("[data-a=" + a + "]").click();
          assert(
            await settings.evaluate((a) => messages.some((m) => m.a === a), a),
          );
        }
        assert.equal(
          await settings.evaluate(() =>
            messages.some(
              (m) => m.a === "size" || (m.a === "set" && m.navigation),
            ),
          ),
          false,
          "Settings keeps independent geometry/navigation",
        );
        await settings.close();
        assert.deepEqual(errors, []);
      }
    console.log(
      "PASS F01-F05 chrome: Home without heading, one Conversations entry, type counters, contextual breadcrumbs, native Close content boundary, independent Settings. Native OS drag/Close unverified.",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
