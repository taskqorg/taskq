// Chain: Product / M3 / M14 / F01-F18. Invented source receipts exercise the real bundles;
// native chrome, OS clipboard and macOS/Windows interaction acceptance remain separate (R12).
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
const board = "https://example.org/alpha/issues",
  root = "fixture-alpha",
  thread = "task:" + board + "/7";
const older = {
  attempt: 71,
  role: "worker",
  runtime: "codex",
  host: "fixture-host",
  local_session: true,
  session: "codex:old fixture-host",
};
const newer = {
  attempt: 75,
  role: "worker",
  runtime: "claude",
  host: "fixture-host",
  local_session: true,
  session: "claude:new fixture-host",
};
const task = {
  n: 7,
  title: "Owner review preserves exact source identities",
  state: "ask",
  revision: 76,
  since: 600,
  age: "5m",
  created: 50,
  role: "worker",
  attempt: 75,
  session: newer.session,
  runtime: "claude",
  host: "fixture-host",
  local_session: true,
  worker: newer,
  workers: [older, newer],
  controls: ["later"],
  model: "DO_NOT_INFER_THIS_MODEL",
  brief: "Goal: keep every source.",
  question: {
    text: "Current question at its original position",
    choices: [],
    evidence: [],
  },
  history: [
    {
      question: {
        text: "Earlier question survives the answer",
        choices: [],
        evidence: [],
      },
      answered: {
        by: "owner",
        text: "Keep the original question",
        at: 400,
        source: {
          id: 74,
          question: 72,
          asked_at: 200,
          attempt: 71,
          session: older.session,
        },
      },
    },
  ],
};
const project = {
  root,
  name: "Alpha",
  board_kind: "github",
  issues: board,
  alive: true,
  complete: true,
  tasks_live: true,
  host: "fixture-host",
  tasks: [
    task,
    { n: 8, title: "Held draft", state: "held", revision: 8 },
    ...Array.from({ length: 30 }, (_, i) => ({
      n: i + 20,
      title: "Long task " + i + " with enough content for scroll observation",
      state: "ready",
      revision: i + 20,
      age: "2m",
    })),
  ],
  hosts: [{ name: "fixture-host", live: true, age: 2, age_text: "2s" }],
  events: [],
  reports: { complete: true, reports: [], history: [] },
};
const dashboard = {
  schema: 1,
  selected: "running",
  views: [
    { id: "all", name: "All tasks", saved: {} },
    { id: "completed", name: "Completed", saved: {} },
    { id: "running", name: "Running", saved: { states: ["doing", "review"] } },
  ],
  pins: [],
  collapsed: [],
};
const state = {
  platform: "windows",
  font: 16,
  theme: "dark",
  host: "fixture-host",
  now: 1000,
  version: "fixture",
  top: true,
  stop_modes: true,
  display: require("./fixtures/display-model.json").display,
  projects: [project],
  navigation: { schema: 1, current: { page: "home" }, dashboard },
  conversations: { complete: true, items: [] },
  messages: { complete: true, entries: [] },
  ai: { history: [] },
};
(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  try {
    const home = await browser.newPage({
        viewport: { width: 420, height: 760 },
      }),
      workspace = await browser.newPage({
        viewport: { width: 1000, height: 760 },
      }),
      errors = [];
    for (const page of [home, workspace])
      page.on("pageerror", (e) => errors.push(e.message));
    await home.setContent(fs.readFileSync("build/web/tray.html", "utf8"));
    await home.evaluate((s) => {
      window.sent = [];
      window.ipc = { postMessage: (text) => sent.push(JSON.parse(text)) };
      taskq.receive(s);
    }, state);
    assert.equal(
      await home.locator(".head h1").count(),
      0,
      "F04: no Home heading or reserved title",
    );
    assert.equal(await home.locator(".conversations-entry").count(), 1);
    assert.equal(
      await home.locator('.head [aria-label="Close menu"]').count(),
      0,
    );
    assert.equal(
      await home.locator(".top-task-counters .communication-count").count(),
      2,
      "F03: independent New/Needs me counters",
    );
    assert.equal(
      await home
        .locator(
          ".host-runtime-totals .host-runtime[data-runtime=Antigravity] svg",
        )
        .count(),
      1,
    );
    assert(
      !(await home
        .locator(".host-runtime-totals")
        .innerText()
        .then((t) => /Antigravity|Partial/.test(t))),
    );
    assert.equal(
      await home.locator(".viewbar>[data-a=view-group]").count(),
      1,
      "A3: leading grouping replaces the filter glyph",
    );
    assert.equal(
      await home.locator('[title^="Previous section:"]').count(),
      0,
      "F11: no invented previous section",
    );
    const choose = async (name) => {
      await home.locator(".view-dropdown>summary").click();
      await home
        .locator(".view-options")
        .getByRole("button", { name, exact: true })
        .click();
    };
    await choose("All tasks");
    assert.equal(
      await home.locator('[title="Previous section: Running"]').count(),
      1,
    );
    await choose("All tasks");
    assert.equal(
      await home.locator('[title="Previous section: Running"]').count(),
      1,
      "choosing current preserves previous",
    );
    await home.locator('[title="Previous section: Running"]').click();
    await home.locator('[title="Previous section: All tasks"]').click();
    assert.match(
      await home.locator(".view-dropdown>summary").innerText(),
      /All tasks/,
      "repeated previous swaps the pair",
    );
    const group = home.locator(".viewbar [data-a=view-group]"),
      before = await home
        .locator(".view-dropdown>summary")
        .getAttribute("aria-label");
    await group.click();
    assert.equal(await group.getAttribute("aria-pressed"), "true");
    assert.equal(
      await home.locator(".view-dropdown>summary").getAttribute("aria-label"),
      before,
    );
    const total = home.locator(".filter-header .tqp-counts>button").first();
    assert.match(await total.innerText(), /^Σ 32$/);
    assert.match(await total.getAttribute("aria-label"), /All tasks: 32 open/);
    const strip = home.locator(".dashboard-status>summary"),
      top = await strip.boundingBox();
    await home.locator(".content").evaluate((e) => (e.scrollTop = 3));
    const moved = await strip.boundingBox();
    assert(
      Math.abs(top.y - moved.y) < 0.5,
      "F08: first scroll does not move the strip",
    );
    await home.locator(".content").evaluate((e) => (e.scrollTop = 0));
    const mainRow = home.locator('.task-target[data-task="7"]');
    assert.equal(
      await mainRow.locator("button,a,[tabindex]").count(),
      1,
      "A7: only the independent Copy action accompanies row navigation",
    );
    const original = await mainRow.evaluate((e) => e.outerHTML);
    await mainRow.click();
    assert(
      await home.evaluate(
        ({ board, root }) =>
          sent.some(
            (m) =>
              m.a === "workspace" &&
              m.open.board === board &&
              m.open.root === root &&
              m.open.task === 7 &&
              m.open.target.revision === 76,
          ),
        { board, root },
      ),
    );
    await workspace.setContent(
      fs
        .readFileSync("build/web/workspace.html", "utf8")
        .replace(
          "<head>",
          "<head><script>window.taskqNativeFrame=true;</script>",
        ),
    );
    await workspace.evaluate(
      ({ state, board, root }) => {
        window.sent = [];
        document.execCommand = (kind) => {
          if (kind !== "copy") return false;
          window.copied = document.activeElement.value;
          return true;
        };
        window.ipc = {
          postMessage: (text) => {
            const m = JSON.parse(text);
            sent.push(m);
            if (m.a === "conversation-dialogue" && m.action === "transcript")
              queueMicrotask(() =>
                taskq.receive({
                  dialogueReceipt: {
                    ...m,
                    state: "accepted",
                    value: {
                      generation: "source-" + m.attempt,
                      start: 0,
                      end: 100,
                      size: 100,
                      following: false,
                      items:
                        m.attempt === 71
                          ? [
                              {
                                at: 0,
                                i: 0,
                                role: "assistant",
                                updated: 100000,
                                text: "First model message",
                                model: "invented-model-one",
                                session: "codex:old",
                                attempt: 71,
                              },
                              {
                                at: 10,
                                i: 0,
                                role: "assistant",
                                updated: 300000,
                                text: "Another historical model",
                                model: "invented-model-two",
                                session: "codex:old",
                                attempt: 71,
                              },
                            ]
                          : [
                              {
                                at: 0,
                                i: 0,
                                role: "assistant",
                                updated: 500000,
                                text: "New worker model",
                                model: "invented-model-three",
                                session: "claude:new",
                                attempt: 75,
                              },
                              {
                                at: 10,
                                i: 0,
                                role: "assistant",
                                updated: 550000,
                                text: "Unattributed native block",
                                session: "claude:new",
                                attempt: 75,
                              },
                            ],
                    },
                  },
                }),
              );
          },
        };
        taskq.receive(state);
        taskq.receive({
          open: {
            page: "conversations",
            root,
            board,
            task: 7,
            target: { kind: "question", revision: 76 },
          },
        });
      },
      { state, board, root },
    );
    await workspace.getByText("New worker model", { exact: true }).waitFor();
    const shared = workspace.locator('.task-target[data-task="7"]');
    assert.equal(
      (await shared.evaluate((e) => e.outerHTML)).replace(
        ' aria-current="page"',
        "",
      ),
      original,
      "F14: one task template supplies both surfaces",
    );
    assert.equal(
      await workspace
        .getByRole("button", { name: "Work sessions", exact: true })
        .count(),
      0,
    );
    assert(await workspace.locator(".sticky-object-head").isVisible());
    assert(
      await workspace
        .locator(".conversation-reader .object-metadata")
        .isVisible(),
      "F15: full metadata flows beneath the fixed header",
    );
    assert.equal(
      await workspace.locator('.head [aria-label="Close workspace"]').count(),
      0,
      "F01: native workspace has no duplicate Close",
    );
    assert(
      await workspace.locator(".sticky-object-head").evaluate((e) => {
        const title = e.querySelector(".thread-title").getBoundingClientRect();
        return [
          ...e.querySelectorAll(":scope>button,:scope>a,:scope>.object-status"),
        ].every(
          (n) =>
            n.getBoundingClientRect().top >= title.bottom - 1 &&
            n.scrollWidth <= n.clientWidth + 1,
        );
      }),
      "F15: header actions wrap below the title without breaking labels",
    );
    assert.match(
      await workspace.locator(".current-question").innerText(),
      /Current question at its original position/,
    );
    assert.match(
      await workspace
        .locator(".dialogue-item")
        .filter({ hasText: "Unattributed native block" })
        .innerText(),
      /Model unknown/,
    );
    assert(
      !(await workspace
        .locator(".conversation-reader")
        .innerText()
        .then((t) => t.includes("DO_NOT_INFER_THIS_MODEL"))),
    );
    await workspace
      .locator(".attempt-row")
      .first()
      .getByRole("button", { name: "Open full log", exact: true })
      .click();
    await workspace.getByText("First model message", { exact: true }).waitFor();
    const chronological = await workspace
      .locator(".dialogue-transcript>.dialogue-item")
      .evaluateAll((es) => es.map((e) => e.dataset.time));
    assert.deepEqual(chronological, [
      "100000",
      "200000",
      "300000",
      "400000",
      "500000",
      "550000",
      "600000",
    ]);
    for (const model of ["invented-model-one", "invented-model-two"])
      assert((await workspace.getByText(model, { exact: false }).count()) > 0);
    const question = workspace.locator('[data-question="72"]');
    assert.equal(
      await question.count(),
      1,
      "F16: historical question appears once after its answer",
    );
    assert.match(await question.innerText(), /Earlier question survives/);
    await workspace
      .locator(".dialogue-item")
      .filter({ hasText: "First model message" })
      .getByRole("button", { name: "Copy block" })
      .click();
    assert.equal(await workspace.evaluate(() => copied), "First model message");
    await workspace.evaluate(
      ({ root, board }) =>
        taskq.receive({
          open: {
            page: "conversations",
            root,
            board,
            task: 7,
            target: { kind: "question", revision: 72 },
          },
        }),
      { root, board },
    );
    await question.waitFor();
    await workspace.waitForFunction(() =>
      sent.some(
        (m) =>
          m.a === "conversation-navigation" &&
          m.navigation.threads.positions[
            "task:https://example.org/alpha/issues/7"
          ]?.target?.revision === 72,
      ),
    );
    await workspace.evaluate(
      ({ root, board }) =>
        taskq.receive({
          open: {
            page: "conversations",
            root,
            board,
            task: 7,
            target: { kind: "question", revision: 999 },
          },
        }),
      { root, board },
    );
    assert.equal(await workspace.locator('[data-question="999"]').count(), 0);
    assert.match(
      await workspace.locator(".history-target").innerText(),
      /another source is never substituted/,
    );
    assert.equal(
      await workspace.evaluate(() =>
        sent.some(
          (m) =>
            ["answer", "reply", "send", "start"].includes(m.a) ||
            ["answer", "reply", "send", "start"].includes(m.action),
        ),
      ),
      false,
      "reading/copying/navigation never submits work",
    );
    // F17: native notification line identity must reach its exact event, never the current question.
    const eventId = JSON.stringify([board, "event", 7, [150, "gone", "71"]]);
    await home.evaluate(
      (root) =>
        taskq.receive({ open: { root, n: 7, event: "150 #7 gone 71" } }),
      root,
    );
    assert.equal(
      await home.evaluate(
        () => sent.filter((m) => m.a === "workspace").at(-1).open.event,
      ),
      "150 #7 gone 71",
      "native click packet survives the Menu handoff",
    );
    await workspace.evaluate(
      ({ state, root, board }) => {
        state.projects[0].events = [
          [150, "gone", "71", 7],
          [600, "ask", "75", 7],
        ];
        taskq.receive(state);
        taskq.receive({
          open: {
            page: "conversations",
            root,
            board,
            task: 7,
            event: "150 #7 gone 71",
          },
        });
      },
      { state: structuredClone(state), root, board },
    );
    await workspace.waitForFunction(
      (eventId) =>
        sent.some(
          (m) =>
            m.a === "conversation-navigation" &&
            Object.values(m.navigation?.threads?.positions || {}).some(
              (p) => p.target?.kind === "event" && p.target.id === eventId,
            ),
        ),
      eventId,
      { timeout: 3000 },
    );
    assert(
      await workspace
        .locator("[data-event]")
        .evaluateAll(
          (es, id) => es.some((e) => e.dataset.event === id),
          eventId,
        ),
      "notification retains exact older event",
    );
    await workspace.evaluate(
      ({ root, board }) =>
        taskq.receive({
          open: {
            page: "conversations",
            root,
            board,
            task: 7,
            event: "149 #7 gone 71",
          },
        }),
      { root, board },
    );
    await workspace.waitForFunction(
      () =>
        sent.some(
          (m) =>
            m.a === "conversation-navigation" &&
            Object.values(m.navigation?.threads?.positions || {}).some(
              (p) => p.target?.id === "149 #7 gone 71",
            ),
        ),
      undefined,
      { timeout: 3000 },
    );
    assert.match(
      await workspace.locator(".history-target").innerText(),
      /another source is never substituted/,
      "missing notification event stays explicit",
    );
    // F17: an exact question outside the first page must accept its source cursor and survive reopening.
    await workspace.evaluate(
      ({ state, root, board }) => {
        const previous = window.ipc.postMessage;
        window.ipc.postMessage = (text) => {
          const m = JSON.parse(text);
          if (m.action !== "history") {
            previous(text);
            return;
          }
          sent.push(m);
          queueMicrotask(() =>
            taskq.receive({
              dialogueReceipt: {
                ...m,
                state: "accepted",
                value: {
                  revision: 9002,
                  start: 85,
                  next: 86,
                  total: 100,
                  complete: true,
                  rows: [
                    {
                      question: {
                        text: "Exact paged question",
                        choices: [],
                        evidence: [],
                      },
                      answered: {
                        text: "Exact paged answer",
                        source: {
                          id: 9003,
                          question: 9001,
                          asked_at: 100,
                          attempt: 71,
                        },
                        at: 101,
                      },
                    },
                  ],
                },
              },
            }),
          );
        };
        state.projects[0].tasks[0] = {
          ...state.projects[0].tasks[0],
          revision: 9002,
          history_count: 100,
          history_lazy: true,
          history: [],
        };
        taskq.receive(state);
        taskq.receive({
          open: {
            page: "conversations",
            root,
            board,
            task: 7,
            target: { kind: "question", revision: 9001 },
          },
        });
      },
      { state: structuredClone(state), root, board },
    );
    await workspace
      .locator('[data-question="9001"]')
      .waitFor({ timeout: 3000 });
    assert(
      await workspace.evaluate(() =>
        sent.some((m) => m.action === "history" && m.question === 9001),
      ),
      "targeted paging names immutable question identity",
    );
    for (const theme of ["light", "dark"]) {
      await home.evaluate(
        ({ state, theme }) => taskq.receive({ ...state, theme, font: 20 }),
        { state, theme },
      );
      await home.setViewportSize({ width: 320, height: 760 });
      assert.equal(
        await home
          .locator(".menu")
          .evaluate((e) => e.scrollWidth <= e.clientWidth + 1),
        true,
        "Home wraps at 320 / 20 / " + theme,
      );
    }
    if (process.env.TASKQ_EVIDENCE_2343) {
      fs.mkdirSync(process.env.TASKQ_EVIDENCE_2343, { recursive: true });
      await home.setViewportSize({ width: 420, height: 760 });
      await home.evaluate(
        (state) => taskq.receive({ ...state, font: 16, theme: "dark" }),
        state,
      );
      await home.screenshot({
        path: process.env.TASKQ_EVIDENCE_2343 + "/home.png",
      });
      await home.evaluate(
        (state) => taskq.receive({ ...state, font: 16, theme: "light" }),
        state,
      );
      await home.screenshot({
        path: process.env.TASKQ_EVIDENCE_2343 + "/home-light.png",
      });
      if ((await home.locator(".project-overview").count()) === 0)
        await home.locator(".viewbar [data-a=view-group]").click();
      await home.locator(".project-overview").first().click();
      await home.screenshot({
        path: process.env.TASKQ_EVIDENCE_2343 + "/project.png",
      });
      await workspace.screenshot({
        path: process.env.TASKQ_EVIDENCE_2343 + "/history.png",
      });
    }
    assert.deepEqual(errors, []);
    console.log(
      "PASS owner UI review browser: Home controls/sticky scroll, shared row, chronological multiworker questions, copy, provenance and exact targets. Native OS proof not supplied.",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
