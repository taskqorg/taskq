// Chain: Scroll position (taskq.md) -> docs/design/ai-workspace.md § Scroll position -> src/reading-anchor.js on real pages.
// Risk: a surface loses the reader's row when data or layout changes, data lands in the middle of a scroll, a reorder drops
// focus, or a drift never reaches telemetry. Invented data, no live board.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
const CAUSES = [
    "insert-above",
    "append",
    "removal",
    "late-image",
    "resize",
    "refresh",
  ],
  BOUND = 2,
  QUIET = 140,
  AFTER_STOP = 250;
const image = (h = 160) =>
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="' + h + '"/>',
  );
const display = require("./fixtures/display-model.json").display,
  cards = require("./fixtures/answer-card.json").cases[0],
  findings = require("./fixtures/finding-composer.json");
const board = "https://example.org/alpha/issues";

const page = async (browser, viewport, html) => {
  const p = await browser.newPage({ viewport });
  p.errors = [];
  p.on("pageerror", (e) => p.errors.push(e.message));
  await p.setContent(html);
  // Chromium's own scroll anchoring would hide a surface the primitive does not hold; WebKit has none.
  await p.addStyleTag({ content: "*{overflow-anchor:none!important}" });
  await p.evaluate(() => {
    window.sent = [];
    window.ipc = { postMessage: (t) => sent.push(JSON.parse(t)) };
  });
  return p;
};
const frames = (p) =>
  p.evaluate(
    () =>
      new Promise((r) =>
        requestAnimationFrame(() =>
          requestAnimationFrame(() => setTimeout(r, 20)),
        ),
      ),
  );

// DOM changes for a surface whose rows are not one data list. Both act on the row the reader holds, so they reach real layout.
const domInsert = (rowsSel, hold, height) => (p) =>
  p.evaluate(
    ({ rowsSel, hold, height }) => {
      const rows = [...document.querySelectorAll(rowsSel)],
        row = rows[hold];
      const block = document.createElement("div");
      block.style.height = height + "px";
      block.dataset.fixture = "added";
      row.parentElement.insertBefore(block, rows[Math.max(0, hold - 1)]);
    },
    { rowsSel, hold, height },
  );
const domAppend = (rowsSel, height) => (p) =>
  p.evaluate(
    ({ rowsSel, height }) => {
      const rows = [...document.querySelectorAll(rowsSel)],
        block = document.createElement("div");
      block.style.height = height + "px";
      rows.at(-1).parentElement.append(block);
    },
    { rowsSel, height },
  );
const domRemove = (rowsSel, hold) => (p) =>
  p.evaluate(
    ({ rowsSel, hold }) => {
      document.querySelectorAll(rowsSel)[hold - 1].remove();
    },
    { rowsSel, hold },
  );
// The block the removal case takes away stands above the reader, so the row itself survives.
const domPad = (rowsSel, hold, height) => (p) =>
  p.evaluate(
    ({ rowsSel, hold, height }) => {
      const row = document.querySelectorAll(rowsSel)[hold - 1];
      row.style.minHeight = height + "px";
    },
    { rowsSel, hold, height },
  );

const task = (n, extra = {}) => ({
  n,
  state: "later",
  title: "Task " + n,
  revision: n,
  labels: [],
  rank: n,
  controls: [],
  ...extra,
});
const taskState = (tasks) => ({
  display,
  platform: "windows",
  font: 16,
  theme: "dark",
  host: "fixture",
  now: 1000,
  version: "fixture",
  size: [360, 420],
  size_anchor: "fixed",
  ai: { draft: "" },
  notify: { projects: {} },
  projects: [
    {
      root: "/fixture/alpha",
      name: "Alpha",
      host: "fixture",
      issues: board,
      board_kind: "github",
      alive: true,
      complete: true,
      age: 1,
      focus: ["all"],
      labels: [],
      cap_saved: { error: "Fixture" },
      tasks,
    },
  ],
});
const taskIds = Array.from({ length: 40 }, (_, i) => 100 + i);
const thread = (attempts, extra = {}) => {
  const row = (a) => ({
    n: 7,
    role: "worker",
    attempt: a,
    runtime: "claude",
    started: 1000 + a * 60,
    local_session: true,
    session: "claude:fixture-" + a + " fixture-host",
    executions: [{ attempt: a, state: "ended" }],
    ...extra,
  });
  const t = {
    n: 7,
    title: "Long running task",
    state: "review",
    role: "worker",
    attempt: attempts.at(-1),
    runtime: "claude",
    local_session: true,
    workers: attempts.map(row),
  };
  return {
    font: 16,
    theme: "dark",
    host: "fixture-host",
    projects: [
      {
        root: "fixture-alpha",
        name: "Alpha",
        issues: board,
        alive: true,
        complete: true,
        tasks: [t],
      },
    ],
    ai: { history: [] },
    conversations: { complete: true, items: [] },
    messages: { complete: true, entries: [] },
  };
};
const attempts = Array.from({ length: 40 }, (_, i) => 100 + i);

const trayHtml = () => fs.readFileSync("build/web/tray.html", "utf8"),
  workspaceHtml = () => fs.readFileSync("build/web/workspace.html", "utf8");
const history = require("./fixtures/icon-history/day-dark.json");
const historyState = (periods = display.periods) => ({
  display: { ...display, periods },
  page: "cadence",
  font: 16,
  theme: "dark",
  size: [360, 300],
  size_anchor: "fixed",
  period: "1d",
  palette: history.palette,
  now: history.captured,
  history,
  icons: { full: history.icon_svg || history.svg, icon: history.compact },
  ai: { draft: "" },
  projects: [
    {
      root: "fixture",
      name: "Alpha",
      tasks: [],
      focus: [],
      labels: [],
      alive: true,
    },
  ],
});
const periods = (extra) => [
  ...display.periods,
  ...Array.from({ length: extra }, (_, i) => ({
    id: "fixture-" + i,
    name: "Fixture period " + i,
  })),
];
const moreCards = (ns) =>
  ns.map((n) => ({
    board: "b",
    row: { ...cards.rows[0], n, url: "https://example.org/o/r/issues/" + n },
  }));
const moreProjects = (count) => [
  ...findings.projects,
  ...Array.from({ length: count }, (_, i) => ({
    ...findings.projects[0],
    id: "fixture-" + i,
    name: "Fixture " + i,
  })),
];

const taskList = {
  rows: '[data-row-key^="task:"]',
  viewport: ".content",
  id: "data-row-key",
  open: async (b) => {
    const p = await page(b, { width: 360, height: 420 }, trayHtml());
    await p.evaluate(
      (s) => taskq.receive(s),
      taskState(taskIds.map((n) => task(n))),
    );
    await p.locator(".tqp-counts [data-state]").first().click();
    await p.locator('[data-a="all-tasks"]').first().click();
    return p;
  },
  change: {
    "insert-above": (p) =>
      p.evaluate(
        (s) => taskq.receive(s),
        taskState([
          task(99, { title: "New row above" }),
          task(98, { title: "Another new row above" }),
          task(97, { title: "Third new row above" }),
          ...taskIds.map((n) => task(n)),
        ]),
      ),
    append: (p) =>
      p.evaluate(
        (s) => taskq.receive(s),
        taskState([
          ...taskIds.map((n) => task(n)),
          task(200, { title: "New row at the end" }),
        ]),
      ),
    removal: (p) =>
      p.evaluate(
        (s) => taskq.receive(s),
        taskState(
          taskIds.filter((n) => n !== 100 && n !== 101).map((n) => task(n)),
        ),
      ),
  },
  refresh: (marker) => (p) =>
    p.evaluate(
      (s) => taskq.receive(s),
      taskState(
        taskIds.map((n) =>
          task(n, { title: n === 139 ? marker : "Task " + n }),
        ),
      ),
    ),
};
const conversation = {
  rows: ".attempt-row",
  viewport: ".conversation-reader",
  id: "data-reading-key",
  open: async (b) => {
    const p = await page(b, { width: 900, height: 520 }, workspaceHtml());
    await p.evaluate((s) => taskq.receive(s), thread(attempts));
    await p.evaluate(
      (board) =>
        taskq.receive({
          open: {
            page: "conversations",
            board,
            task: 7,
            root: "fixture-alpha",
          },
        }),
      board,
    );
    await p.waitForSelector(".attempt-row");
    return p;
  },
  change: {
    "insert-above": (p) =>
      p.evaluate((s) => taskq.receive(s), thread([97, 98, 99, ...attempts])),
    append: (p) =>
      p.evaluate((s) => taskq.receive(s), thread([...attempts, 200])),
    removal: (p) =>
      p.evaluate((s) => taskq.receive(s), thread(attempts.slice(2))),
  },
  refresh: (marker) => (p) =>
    p.evaluate((s) => taskq.receive(s), thread(attempts, { runtime: marker })),
};
// The history page is one fixed set of blocks: a longer period row is the real data that moves the blocks after it.
const historyPage = {
  rows: '[data-row-key^="history:"]',
  viewport: ".content",
  id: "data-row-key",
  hold: 2,
  open: async (b) => {
    const p = await page(b, { width: 360, height: 300 }, trayHtml());
    await p.evaluate((s) => taskq.receive(s), historyState(periods(12)));
    await p.waitForSelector('[data-row-key="history:plot"]');
    return p;
  },
  change: {
    "insert-above": (p) =>
      p.evaluate((s) => taskq.receive(s), historyState(periods(24))),
    append: domAppend('[data-row-key^="history:"]', 200),
    removal: (p) =>
      p.evaluate((s) => taskq.receive(s), historyState(periods(0))),
  },
  refresh: (marker) => (p) =>
    p.evaluate(
      (s) => taskq.receive(s),
      historyState(periods(12).map((x, i) => (i ? x : { ...x, name: marker }))),
    ),
};
// A bounded block is a scroll region inside the page; the tray keeps one found on a draw. Its children are its rows.
const bounded = {
  rows: '[data-bound="fixture"] [data-row-key]',
  viewport: '[data-bound="fixture"]',
  id: "data-row-key",
  open: async (b) => {
    const p = await page(b, { width: 360, height: 700 }, trayHtml());
    await p.evaluate(
      (s) => taskq.receive(s),
      taskState(taskIds.slice(0, 3).map((n) => task(n))),
    );
    await p.evaluate(() => {
      const block = document.createElement("div");
      block.dataset.bound = "fixture";
      block.style.cssText =
        "overflow:auto;height:180px;position:fixed;left:0;top:200px;width:300px;background:Canvas";
      const inner = document.createElement("div");
      inner.innerHTML = Array.from(
        { length: 30 },
        (_, i) =>
          '<p data-row-key="line:' +
          i +
          '" style="margin:0;min-height:24px">Line ' +
          i +
          "</p>",
      ).join("");
      block.append(inner);
      document.querySelector(".menu").append(block);
    });
    await p.evaluate(
      (s) => taskq.receive(s),
      taskState(taskIds.slice(0, 4).map((n) => task(n))),
    );
    return p;
  },
  change: {
    "insert-above": domInsert('[data-bound="fixture"] [data-row-key]', 12, 96),
    append: domAppend('[data-bound="fixture"] [data-row-key]', 96),
    removal: domRemove('[data-bound="fixture"] [data-row-key]', 12),
  },
  refresh: (marker) => (p) =>
    p.evaluate(
      (s) => taskq.receive(s),
      taskState(taskIds.slice(0, 5).map((n) => task(n, { title: marker }))),
    ),
};
const answerCss = fs.readFileSync("src/answer-card.css", "utf8");
const answerCard = {
  rows: "#c [data-row-key]",
  viewport: "#c",
  id: "data-row-key",
  open: async (b) => {
    const p = await page(
      b,
      { width: 480, height: 640 },
      '<!doctype html><meta name="color-scheme" content="light dark"><style>' +
        answerCss +
        'body{margin:0;font:16px/1.4 sans-serif}#c{height:600px;overflow:auto}</style><div id="c"></div><script>' +
        fs.readFileSync("build/web/answer-card.js", "utf8") +
        "</script>",
    );
    await p.evaluate(
      (rows) => {
        window.ui = TaskqAnswerCard.mount(document.getElementById("c"), {
          capability: () => ({ answer: true }),
          send: () => new Promise(() => {}),
        });
        ui.update(rows);
      },
      moreCards([10, 11, 12, 13, 14, 15, 16, 17, 18, 19]),
    );
    return p;
  },
  change: {
    "insert-above": (p) =>
      p.evaluate(
        (rows) => ui.update(rows),
        moreCards([8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]),
      ),
    append: (p) =>
      p.evaluate(
        (rows) => ui.update(rows),
        moreCards([10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]),
      ),
    removal: (p) =>
      p.evaluate(
        (rows) => ui.update(rows),
        moreCards([11, 12, 13, 14, 15, 16, 17, 18, 19]),
      ),
  },
  refresh: (marker) => (p) =>
    p.evaluate(
      (rows) => ui.update(rows),
      moreCards([10, 11, 12, 13, 14, 15, 16, 17, 18, 19]).map((c, i) =>
        i === 9 ? { ...c, row: { ...c.row, title: marker } } : c,
      ),
    ),
};
const composerCss = ["project-blocks", "answer-card", "finding-composer"]
  .map((n) => fs.readFileSync("src/" + n + ".css", "utf8"))
  .join("\n");
const composerJs = ["project-blocks", "answer-card", "finding-composer"]
  .map((n) => fs.readFileSync("build/web/" + n + ".js", "utf8"))
  .join("\n");
const composerRows = "[data-finding-form] > *";
const composer = {
  rows: composerRows,
  viewport: "#app",
  id: null,
  open: async (b) => {
    const p = await page(
      b,
      { width: 480, height: 420 },
      '<!doctype html><meta name="color-scheme" content="light dark"><style>' +
        composerCss +
        'body{margin:0;font:16px/1.4 sans-serif}#app{height:400px;overflow:auto}</style><div id="app"></div><script>' +
        composerJs +
        "</script>",
    );
    await p.evaluate((fx) => {
      window.composer = TaskqFindingComposer.mount(
        document.getElementById("app"),
        {
          capability: () => ({ create: true, start: true }),
          send: async () => ({}),
          reconcile: async () => ({}),
          saveDraft() {},
          loadDraft() {},
          navigation: { project() {}, tasks() {}, open() {} },
          answer: {
            capability: () => ({ answer: true }),
            send: async () => ({ state: "accepted" }),
          },
        },
      );
      composer.update(fx);
    }, findings);
    return p;
  },
  change: {
    "insert-above": (p) =>
      p.evaluate((fx) => composer.update(fx), {
        ...findings,
        projects: moreProjects(8),
      }),
    append: domAppend(composerRows, 200),
    removal: domPad(composerRows, 3, 0),
  },
  refresh: (marker) => (p) =>
    p.evaluate((fx) => composer.update(fx), {
      ...findings,
      projects: findings.projects.map((x, i) =>
        i ? x : { ...x, name: marker },
      ),
    }),
};
const workspaceRows = '.content-body [data-row-key^="line:"]';
const workspaceSurface = {
  rows: workspaceRows,
  viewport: ".content",
  id: "data-row-key",
  open: async (b) => {
    const p = await page(b, { width: 640, height: 260 }, workspaceHtml());
    await p.evaluate((s) => taskq.receive(s), thread(attempts));
    await p.evaluate(() =>
      taskq.receive({ open: { page: "operations", operation: "resources" } }),
    );
    await p.waitForSelector(".content-body");
    await p.evaluate(() => {
      const body = document.querySelector(".content-body");
      for (let i = 0; i < 24; i++) {
        const row = document.createElement("p");
        row.dataset.rowKey = "line:" + i;
        row.textContent = "Fixture line " + i;
        row.style.minHeight = "28px";
        body.append(row);
      }
    });
    return p;
  },
  change: {
    "insert-above": domInsert(workspaceRows, 12, 96),
    append: domAppend(workspaceRows, 96),
    removal: domRemove(workspaceRows, 12),
  },
  refresh: (marker) => (p) =>
    p.evaluate((s) => taskq.receive(s), {
      ...thread(attempts),
      ai: { history: [], error: marker },
    }),
};
const SURFACES = {
  tasks: taskList,
  history: historyPage,
  conversations: conversation,
  bounded,
  "answer-card": answerCard,
  "finding-composer": composer,
  workspace: workspaceSurface,
};

const sel = ({ viewport, rows, id, hold }) => ({ viewport, rows, id, hold });
// Reads the visible rows by identity. The first is the row the anchor holds.
const place = (p, s) =>
  p.evaluate(({ viewport, rows, id }) => {
    const v = document.querySelector(viewport),
      box = v.getBoundingClientRect(),
      seen = [];
    for (const [i, row] of [...document.querySelectorAll(rows)].entries()) {
      const r = row.getBoundingClientRect();
      if (r.height <= 0 || r.bottom <= box.top || r.top >= box.bottom) continue;
      seen.push({
        key: id ? row.getAttribute(id) : String(i),
        y: r.top - box.top,
      });
    }
    return seen;
  }, sel(s));
// Only input moves a reader's anchor, so a real wheel parks the reader, and the gate closes before the case starts.
const park = async (p, s) => {
  const { delta, x, y } = await p.evaluate(({ viewport, rows, hold }) => {
    const v = document.querySelector(viewport),
      all = [...document.querySelectorAll(rows)],
      row = all[hold ?? Math.floor(all.length / 2)],
      box = v.getBoundingClientRect();
    if (v.scrollHeight <= v.clientHeight)
      throw Error(
        "surface does not scroll: " +
          viewport +
          " " +
          v.scrollHeight +
          "/" +
          v.clientHeight,
      );
    return {
      delta: row.getBoundingClientRect().top - box.top - 8,
      x: box.left + box.width / 2,
      y: box.top + Math.min(box.height / 2, 120),
    };
  }, sel(s));
  await p.mouse.move(x, y);
  await p.mouse.wheel(0, delta);
  await p.waitForTimeout(260);
  assert(
    await p.evaluate(
      (v) => document.querySelector(v).scrollTop > 0,
      s.viewport,
    ),
    "the wheel did not move " + s.viewport,
  );
};
const worst = (before, after, only) => {
  let shift = 0,
    seen = 0;
  const at = new Map(after.map((r) => [r.key, r.y]));
  for (const r of only ? before.slice(0, 1) : before)
    if (at.has(r.key)) {
      seen++;
      shift = Math.max(shift, Math.abs(at.get(r.key) - r.y));
    }
  return { shift, seen };
};
const drifts = (p) =>
  p.evaluate(() =>
    sent
      .filter(
        (m) =>
          m.a === "web-observation" &&
          /^js\.scroll\.drift\./.test(m.row?.operation || ""),
      )
      .map((m) => m.row.operation),
  );

const lateImage = (p, s, target) =>
  p.evaluate(
    ({ viewport, rows, target, src }) => {
      const v = document.querySelector(viewport),
        box = v.getBoundingClientRect(),
        all = [...document.querySelectorAll(rows)];
      const held = all.findIndex(
        (r) => r.getBoundingClientRect().bottom > box.top,
      );
      const row = all[target === "held" ? held : Math.max(0, held - 1)],
        img = document.createElement("img");
      img.style.display = "block";
      if (target !== "held") row.append(img);
      else
        for (let parent = row; ;) {
          const first = [...parent.children].find((c) => {
            const r = c.getBoundingClientRect();
            return r.height > 0 && r.bottom > box.top;
          });
          if (!first) {
            parent.append(img);
            break;
          }
          if (
            first.children.length &&
            first.getBoundingClientRect().top < box.top
          ) {
            parent = first;
            continue;
          }
          const kids = [...parent.children],
            open = kids
              .slice(kids.indexOf(first))
              .find(
                (c) =>
                  !c.matches("input,select,textarea,button,img,svg,hr,br") &&
                  c.getBoundingClientRect().height > 0,
              );
          (open || parent).append(img);
          break;
        }
      return new Promise((done) => {
        img.onload = () => done();
        setTimeout(() => {
          img.src = src;
        }, 30);
      });
    },
    { ...sel(s), target, src: image() },
  );

const run = async (browser, name, s, cause) => {
  const p = await s.open(browser);
  try {
    await frames(p);
    await park(p, s);
    await frames(p);
    const before = await place(p, s);
    assert(before.length > 0, name + " " + cause + ": no visible rows");
    const at = name + " " + cause;
    if (cause === "late-image") await lateImage(p, s, "above");
    else if (cause === "resize") {
      const size = p.viewportSize();
      await p.setViewportSize({ width: size.width - 40, height: size.height });
    } else if (cause === "refresh") await refreshDuringScrolling(p, s, at);
    else await s.change[cause](p);
    await frames(p);
    await frames(p);
    if (cause !== "refresh") {
      const { shift, seen } = worst(
        before,
        await place(p, s),
        cause === "resize",
      );
      assert(seen > 0, at + ": the held rows left the viewport");
      assert(
        shift <= BOUND,
        at + ": a visible row shifted " + shift.toFixed(1) + " CSS px",
      );
      assert.deepEqual(
        await drifts(p),
        [],
        at + ": a change inside the bound reported drift",
      );
    }
    assert.deepEqual(p.errors, [], at + ": page errors");
  } finally {
    await p.close();
  }
};

// Data arriving while the wheel moves waits, then lands within the stated bound after the last scroll, with the rows held.
const refreshDuringScrolling = async (p, s, at) => {
  const marker = "Fixture marker " + Math.random().toString(36).slice(2);
  await p.evaluate(
    ({ viewport, rows, id, marker }) => {
      const proof = (window.gateProof = {
        last: 0,
        applied: null,
        place: null,
      });
      const read = () => {
        const v = document.querySelector(viewport),
          box = v.getBoundingClientRect();
        return [...document.querySelectorAll(rows)].flatMap((row, i) => {
          const r = row.getBoundingClientRect();
          return r.height <= 0 || r.bottom <= box.top || r.top >= box.bottom
            ? []
            : [
                {
                  key: id ? row.getAttribute(id) : String(i),
                  y: r.top - box.top,
                },
              ];
        });
      };
      const note = () => {
        proof.last = performance.now();
        proof.place = read();
      };
      document.addEventListener("wheel", note, {
        capture: true,
        passive: true,
      });
      document.addEventListener("scroll", note, {
        capture: true,
        passive: true,
      });
      new MutationObserver(() => {
        if (!proof.applied && document.body.textContent.includes(marker))
          proof.applied = {
            at: performance.now(),
            last: proof.last,
            place: proof.place,
          };
      }).observe(document.body, {
        subtree: true,
        childList: true,
        characterData: true,
        attributes: true,
      });
    },
    { ...sel(s), marker },
  );
  const box = await p.locator(s.viewport).first().boundingBox();
  await p.mouse.move(
    box.x + box.width / 2,
    box.y + Math.min(box.height / 2, 120),
  );
  await p.mouse.wheel(0, 24);
  await p.waitForTimeout(30);
  await s.refresh(marker)(p);
  for (let i = 0; i < 3; i++) {
    await p.mouse.wheel(0, i % 2 ? 24 : -24);
    await p.waitForTimeout(40);
  }
  await p.waitForFunction(() => gateProof.applied, null, { timeout: 3000 });
  await frames(p);
  const proof = await p.evaluate(() => gateProof),
    late = proof.applied.at - proof.applied.last;
  assert(
    late >= QUIET,
    at +
      ": data applied " +
      late.toFixed(0) +
      " ms after scroll input, during the scroll",
  );
  assert(
    late <= AFTER_STOP,
    at + ": data applied " + late.toFixed(0) + " ms after the scroll stopped",
  );
  const { shift, seen } = worst(proof.applied.place, await place(p, s));
  assert(seen > 0, at + ": the held rows left the viewport");
  assert(
    shift <= BOUND,
    at + ": a visible row shifted " + shift.toFixed(1) + " CSS px",
  );
  assert.deepEqual(
    await drifts(p),
    [],
    at + ": a refresh inside the bound reported drift",
  );
};

// A row that grows in view moves the rows after it: that is drift, and telemetry names the surface and the cause.
const drifted = async (browser, name, s) => {
  const p = await s.open(browser);
  try {
    await frames(p);
    await park(p, s);
    await frames(p);
    await lateImage(p, s, "held");
    await frames(p);
    await frames(p);
    const seen = await drifts(p);
    assert(
      seen.includes("js.scroll.drift." + name + ".late-image"),
      name +
        " forced drift reports the surface and cause, saw " +
        JSON.stringify(seen),
    );
    const row = await p.evaluate(
      () =>
        sent.find((m) => /^js\.scroll\.drift\./.test(m.row?.operation || ""))
          .row,
    );
    assert.deepEqual([row.outcome, row.error], ["failed", "scroll-drift"]);
  } finally {
    await p.close();
  }
};
const grown = (n, extra = []) =>
  taskState([
    ...extra,
    ...taskIds.map((id) =>
      task(id, {
        title:
          id === n
            ? "A much longer title that wraps over several lines in a narrow window so the rows below move away from where they were"
            : "Task " + id,
      }),
    ),
  ]);
const forcedCauses = async (browser) => {
  const heldId = async (p) =>
    Number((await place(p, taskList))[0].key.match(/#?(\d+)$/)[1]);
  const cases = {
    refresh: async (p, n) => p.evaluate((s) => taskq.receive(s), grown(n)),
    "insert-above": async (p, n) =>
      p.evaluate(
        (s) => taskq.receive(s),
        grown(n, [
          task(99, { title: "New row above" }),
          task(98, { title: "Another new row above" }),
        ]),
      ),
    append: async (p, n) =>
      p.evaluate(
        (s) => taskq.receive(s),
        taskState([
          ...taskIds.map((id) => task(id)),
          task(200, { rank: n + 0.5 }),
        ]),
      ),
    removal: async (p, n) =>
      p.evaluate((s) => taskq.receive(s), {
        ...grown(n + 1),
        projects: [
          {
            ...grown(n + 1).projects[0],
            tasks: grown(n + 1).projects[0].tasks.filter((t) => t.n !== n),
          },
        ],
      }),
    resize: async (p) =>
      p.evaluate(({ rows }) => {
        const v = document.querySelector(".content"),
          box = v.getBoundingClientRect();
        const row = [...document.querySelectorAll(rows)].find(
          (r) => r.getBoundingClientRect().bottom > box.top,
        );
        const block = document.createElement("div");
        block.style.height = "120px";
        row.append(block);
      }, sel(taskList)),
  };
  for (const [cause, change] of Object.entries(cases)) {
    const p = await taskList.open(browser);
    try {
      await frames(p);
      await park(p, taskList);
      await frames(p);
      await change(p, await heldId(p));
      await frames(p);
      await frames(p);
      const seen = await drifts(p);
      assert(
        seen.includes("js.scroll.drift.tasks." + cause),
        "forced " + cause + " reports its cause, saw " + JSON.stringify(seen),
      );
    } finally {
      await p.close();
    }
  }
};

// A reorder keeps focus on the same row's control; Back returns to the same row; a held selection holds its row.
const focusAndBack = async (browser) => {
  const p = await taskList.open(browser);
  try {
    await frames(p);
    await park(p, taskList);
    await frames(p);
    const before = await place(p, taskList),
      n = await p.evaluate(({ rows }) => {
        const v = document.querySelector(".content"),
          box = v.getBoundingClientRect(),
          row = [...document.querySelectorAll(rows)].find(
            (r) => r.getBoundingClientRect().top >= box.top,
          );
        const target = row.querySelector(".task-target");
        target.focus();
        return Number(target.dataset.task);
      }, sel(taskList));
    const reordered = taskState(
      taskIds.map((id) => task(id, { rank: 200 - id })),
    );
    await p.evaluate((s) => taskq.receive(s), reordered);
    await frames(p);
    assert.equal(
      await p.evaluate(() => Number(document.activeElement?.dataset?.task)),
      n,
      "a reorder keeps focus on the same row",
    );
    assert(before.length > 0);
    await p.evaluate(
      (s) => taskq.receive(s),
      taskState(taskIds.map((id) => task(id))),
    );
    await frames(p);
    await park(p, taskList);
    await frames(p);
    const list = await place(p, taskList);
    await p.evaluate(() =>
      document.querySelector('[data-route="settings"]').click(),
    );
    await frames(p);
    await p.evaluate(() => document.querySelector('[data-a="back"]').click());
    await frames(p);
    await frames(p);
    const { shift, seen } = worst(list, await place(p, taskList));
    assert(
      seen > 0 && shift <= BOUND,
      "Back returns to the same row, shifted " +
        shift.toFixed(1) +
        " CSS px with " +
        seen +
        " rows found",
    );
  } finally {
    await p.close();
  }
  const c = await conversation.open(browser);
  try {
    await frames(c);
    await park(c, conversation);
    await frames(c);
    const key = await c.evaluate(({ rows }) => {
      const v = document.querySelector(".conversation-reader"),
        box = v.getBoundingClientRect(),
        row = [...document.querySelectorAll(rows)].filter(
          (r) => r.getBoundingClientRect().top >= box.top,
        )[1];
      getSelection().selectAllChildren(row);
      return row.dataset.readingKey;
    }, sel(conversation));
    const y = () =>
      c.evaluate((key) => {
        const v = document.querySelector(".conversation-reader");
        return (
          document
            .querySelector('[data-reading-key="' + key + '"]')
            .getBoundingClientRect().top - v.getBoundingClientRect().top
        );
      }, key);
    const was = await y();
    await conversation.change["insert-above"](c);
    await frames(c);
    await frames(c);
    assert(
      Math.abs((await y()) - was) <= BOUND,
      "a selected row keeps its place when rows land above it",
    );
  } finally {
    await c.close();
  }
};

(async () => {
  const browser = await chromium.launch(require("./browser.cjs")()),
    failures = [],
    only = process.argv[2];
  try {
    for (const [name, s] of Object.entries(SURFACES)) {
      if (only && only !== name) continue;
      for (const cause of CAUSES)
        try {
          await run(browser, name, s, cause);
          console.log("PASS " + name + " " + cause);
        } catch (e) {
          const line = e.message.split("\n")[0].slice(0, 200);
          failures.push(name + " " + cause + ": " + line);
          console.error("FAIL " + name + " " + cause + ": " + line);
        }
      try {
        await drifted(browser, name, s);
        console.log("PASS " + name + " forced drift");
      } catch (e) {
        const line = e.message.split("\n")[0].slice(0, 200);
        failures.push(name + " forced drift: " + line);
        console.error("FAIL " + name + " forced drift: " + line);
      }
    }
    if (!only) {
      for (const [what, check] of [
        ["forced causes", forcedCauses],
        ["focus, Back and selection", focusAndBack],
      ])
        try {
          await check(browser);
          console.log("PASS " + what);
        } catch (e) {
          const line = e.message.split("\n")[0].slice(0, 200);
          failures.push(what + ": " + line);
          console.error("FAIL " + what + ": " + line);
        }
    }
    assert.equal(
      failures.length,
      0,
      failures.length + " scroll table failures",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e.message.slice(0, 4000));
  process.exit(1);
});
