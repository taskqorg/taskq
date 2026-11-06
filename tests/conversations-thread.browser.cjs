// Chain: Conversations (taskq.md) -> docs/design/ai-workspace.md § Task thread -> src/conversations.js on the real workspace page.
// Risk: a thread hides an attempt or a record, opening reads a full worker log, an image grows without bound, a board read
// moves the reader or replaces local records, click-to-text goes unmeasured, or a closed task pretends to accept a message.
// Invented data, no live board.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
const board = "https://example.org/alpha/issues",
  thread = "task:" + board + "/7",
  at = (seconds) => 1700000000 + seconds;
const worker = (attempt, extra = {}) => ({
  attempt,
  role: "worker",
  runtime: "claude",
  started: at(attempt * 100),
  local_session: true,
  session: "claude:fixture-" + attempt + " fixture-host",
  ...extra,
});
const rows = [
  {
    question: {
      text: "Which route should the worker keep?",
      choices: [],
      evidence: [],
      at: at(150),
    },
    answered: {
      revision: 30,
      by: "owner",
      text: "Keep the compatible route",
      at: at(160),
    },
  },
  {
    answered: {
      revision: 31,
      by: "owner",
      text: "Owner message between attempts",
      at: at(250),
    },
  },
];
const live = {
  n: 7,
  title: "Thread fixture",
  state: "doing",
  role: "worker",
  attempt: 3,
  runtime: "claude",
  revision: 40,
  local_session: true,
  session: "claude:fixture-3 fixture-host",
  workers: [
    worker(1),
    worker(2),
    worker(3),
    worker(4, { local_session: false, session: "", host: undefined }),
  ],
  reviewers: [
    {
      attempt: 9,
      role: "reviewer",
      started: at(350),
      session: "claude:fixture-9 fixture-host",
    },
  ],
  executions: [
    { attempt: 1, state: "ended" },
    { attempt: 2, state: "ended" },
    { attempt: 3, state: "running" },
  ],
  history: rows,
  history_count: rows.length,
  conversation: {
    id: "owner",
    brief: "Recorded task brief",
    verdict: "Recorded result",
  },
};
const state = (tasks) => ({
  font: 16,
  theme: "dark",
  host: "fixture-host",
  projects: [
    {
      root: "fixture-alpha",
      name: "Alpha",
      issues: board,
      board_kind: "github",
      alive: true,
      complete: true,
      tasks,
    },
  ],
  ai: { history: [] },
  conversations: { complete: true, items: [] },
  messages: { complete: true, entries: [] },
});
const page = async (browser) => {
  const p = await browser.newPage({ viewport: { width: 900, height: 520 } });
  p.errors = [];
  p.on("pageerror", (e) => p.errors.push(e.message));
  await p.setContent(fs.readFileSync("build/web/workspace.html", "utf8"));
  await p.evaluate(() => {
    window.sent = [];
    window.ipc = { postMessage: (t) => sent.push(JSON.parse(t)) };
  });
  return p;
};
const open = (p, task = 7) =>
  p.evaluate(
    ({ board, task }) =>
      taskq.receive({
        open: { page: "conversations", board, task, root: "fixture-alpha" },
      }),
    { board, task },
  );
const transcripts = (p) =>
  p.evaluate(() => sent.filter((m) => m.action === "transcript"));
const opens = (p) =>
  p.evaluate(() =>
    sent
      .filter(
        (m) =>
          m.a === "web-observation" &&
          /^js\.conversation\.open\./.test(m.row?.operation || ""),
      )
      .map((m) => m.row),
  );
const reply = (p, request, value) =>
  p.evaluate(
    ({ request, value }) =>
      taskq.receive({
        dialogueReceipt: {
          id: request.id,
          conversation: request.conversation,
          action: "transcript",
          state: "accepted",
          value,
        },
      }),
    { request, value },
  );
const item = (attempt, i) => ({
  at: at(attempt * 100) + 10 + i,
  i: 0,
  role: "assistant",
  text: "Worker " + attempt + " line " + i,
  updated: at(attempt * 100) + 10 + i,
});
const order = (p) =>
  p.evaluate(() =>
    [...document.querySelectorAll(".conversation-reader [data-time]")]
      .filter((n) => n.checkVisibility())
      .map((n) => Number(n.dataset.time)),
  );

const liveThread = async (browser) => {
  const p = await page(browser);
  try {
    await p.evaluate((s) => taskq.receive(s), state([live]));
    await open(p);
    await p.waitForSelector(".attempt-row");
    await p.waitForTimeout(40);
    const text = await p.locator(".conversation-reader").innerText();
    for (const shown of [
      "Worker 1",
      "Worker 2",
      "Worker 3",
      "Worker 4",
      "Reviewer 9",
      "Which route should the worker keep?",
      "Keep the compatible route",
      "Owner message between attempts",
    ])
      assert(text.includes(shown), "a thread lists " + shown);
    assert.match(
      text,
      /session not recorded/,
      "an attempt with no recorded session stays a visible unknown",
    );
    assert.match(
      text,
      /a reviewer log is not served in the thread/,
      "a reviewer attempt stays a visible unknown",
    );
    const times = await order(p);
    assert(times.length >= 6, "records carry their times, saw " + times.length);
    assert.deepEqual(
      times,
      [...times].sort((a, b) => a - b),
      "records read in time order",
    );

    // Opening reads only the live attempt's bounded tail.
    await p.waitForFunction(() => sent.some((m) => m.action === "transcript"));
    const read = await transcripts(p);
    assert.deepEqual(
      read.map((m) => m.attempt),
      [3],
      "opening a thread reads no ended attempt log",
    );
    await reply(p, read[0], {
      generation: "fixture-3",
      items: [item(3, 0)],
      start: 0,
      end: 1,
      size: 1,
    });
    await p.waitForFunction(() =>
      document.body.textContent.includes("Worker 3 line 0"),
    );

    // A click opens one attempt's full log as pages.
    await p.locator('[data-c="worker-attempt"][data-attempt="1"]').click();
    await p.waitForFunction(() =>
      sent.some((m) => m.action === "transcript" && m.attempt === 1),
    );
    const first = (await transcripts(p)).find((m) => m.attempt === 1);
    const large = {
      at: at(120),
      i: 1,
      role: "image",
      text: "",
      updated: at(120),
      image: {
        name: "Tall fixture",
        width: 1000,
        height: 3000,
        src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
      },
    };
    await reply(p, first, {
      generation: "fixture-1",
      items: [item(1, 40), large],
      start: 40,
      end: 60,
      size: 100,
    });
    await p.waitForFunction(() =>
      document.body.textContent.includes("Worker 1 line 40"),
    );
    const image = p.locator(".transcript-image img").first();
    await image.waitFor({ state: "attached" });
    assert.equal(
      await image.getAttribute("loading"),
      "lazy",
      "a thread image loads lazily",
    );
    await image.scrollIntoViewIfNeeded();
    await p.waitForFunction(
      () => document.querySelector(".transcript-image img")?.complete,
    );
    const box = await image.boundingBox(),
      width = await p
        .locator(".conversation-reader")
        .evaluate((e) => e.clientWidth);
    assert(
      box.height <= 480 && box.width <= width,
      "a thread image stays bounded, saw " + box.width + "x" + box.height,
    );
    const row = p.locator('.attempt-row[data-reading-key="attempt:1"]');
    assert.equal(
      await row.locator('[data-c="worker-earlier"]').count(),
      1,
      "an opened log pages back",
    );
    assert.equal(
      await row.locator('[data-c="worker-later"]').count(),
      1,
      "an opened log pages forward",
    );
    await row.locator('[data-c="worker-earlier"]').click();
    await p.waitForFunction(
      () =>
        sent.filter((m) => m.action === "transcript" && m.attempt === 1)
          .length > 1,
    );
    const earlier = (await transcripts(p)).filter((m) => m.attempt === 1)[1];
    assert.equal(
      earlier.before,
      40,
      "the next page reads before the shown window",
    );
    const ended = (await transcripts(p)).filter((m) => m.attempt === 2);
    assert.equal(ended.length, 0, "an attempt nobody opened is still unread");

    // A live attempt accepts a message through the board.
    await p.locator("[data-c-field=steer]").fill("Keep this intent");
    await p.locator("[data-a=answer-steer]").click();
    const op = await p.evaluate(() => sent.find((m) => m.a === "answer").op);
    assert.equal(op.action, "steer");

    // Click-to-text is one measure per open, with cold or warm state and the source.
    const cold = await opens(p);
    assert.deepEqual(
      cold.map((r) => r.operation),
      ["js.conversation.open.cold.snapshot"],
      "one open records one cold measure",
    );
    assert(
      Number.isFinite(cold[0].duration_us),
      "the measure carries the elapsed time",
    );
    await p.evaluate(
      (s) => taskq.receive(s),
      state([
        live,
        {
          ...live,
          n: 8,
          title: "Other thread",
          attempt: 1,
          state: "review",
          workers: [worker(1)],
          executions: [{ attempt: 1, state: "ended" }],
          history: [],
          history_count: 0,
          reviewers: [],
        },
      ]),
    );
    await open(p, 8);
    await p.waitForFunction(() =>
      document
        .querySelector("[data-thread-id]")
        ?.dataset.threadId.endsWith("/8"),
    );
    await p.waitForTimeout(40);
    assert.match(
      await p.locator(".conversation-detail").innerText(),
      /Reviewer input is read-only/,
      "a reviewer attempt states why no message goes",
    );
    await open(p, 7);
    await p.waitForFunction(() =>
      document
        .querySelector("[data-thread-id]")
        ?.dataset.threadId.endsWith("/7"),
    );
    await p.waitForTimeout(40);
    const measures = (await opens(p)).map((r) => r.operation);
    assert.deepEqual(
      measures,
      [
        "js.conversation.open.cold.snapshot",
        "js.conversation.open.cold.snapshot",
        "js.conversation.open.warm.snapshot",
      ],
      "a reopen measures warm",
    );
    await p.evaluate((s) => taskq.receive(s), state([live]));
    await p.waitForTimeout(40);
    assert.equal(
      (await opens(p)).length,
      3,
      "a snapshot redraw is not an open",
    );
    await p.evaluate(() =>
      taskq.receive({ open: { page: "operations", operation: "resources" } }),
    );
    await p.evaluate(() => taskq.receive({ open: { page: "conversations" } }));
    await p.waitForTimeout(40);
    assert.equal(
      (await opens(p)).at(-1).operation,
      "js.conversation.open.warm.snapshot",
      "coming back to the page opens the selected thread",
    );
    assert.equal((await opens(p)).length, 4, "coming back measures once");
    assert.deepEqual(p.errors, []);
  } finally {
    await p.close();
  }
};

const closedTask = (extra = {}) => ({
  n: 7,
  title: "Closed fixture",
  state: "closed",
  revision: 51,
  close: "Completed",
  brief: "Recorded task brief",
  history: rows,
  workers: [worker(1), worker(2), worker(3)],
  ...extra,
});
const closedThread = async (browser, boardReply) => {
  const p = await page(browser);
  try {
    await p.evaluate((s) => taskq.receive(s), state([]));
    await open(p);
    await p.waitForFunction(() =>
      sent.some((m) => m.a === "completed" && !m.transcript && !m.board),
    );
    const local = await p.evaluate(() =>
      sent.find((m) => m.a === "completed" && !m.transcript && !m.board),
    );
    await p.evaluate(
      ({ local, task }) =>
        taskq.receive({
          completed: { root: local.root, id: local.id, tasks: [task] },
        }),
      { local, task: closedTask() },
    );
    await p.waitForSelector(".attempt-row");
    await p.waitForFunction(() => sent.some((m) => m.action === "history"));
    const history = await p.evaluate(() =>
      sent.find((m) => m.action === "history"),
    );
    await p.evaluate(
      ({ history, rows }) =>
        taskq.receive({
          dialogueReceipt: {
            ...history,
            state: "accepted",
            value: {
              revision: 51,
              start: 0,
              next: rows.length,
              total: rows.length,
              complete: true,
              rows,
            },
          },
        }),
      { history, rows },
    );
    await p.waitForFunction(() =>
      document.body.textContent.includes("Owner message between attempts"),
    );
    await p.waitForTimeout(40);

    // First paint is local, every attempt is listed, no log was read, and the board read runs behind it.
    const text = await p.locator(".conversation-reader").innerText();
    for (const shown of [
      "Worker 1",
      "Worker 2",
      "Worker 3",
      "Which route should the worker keep?",
      "Owner message between attempts",
    ])
      assert(text.includes(shown), "a closed thread lists " + shown);
    assert.deepEqual(
      await transcripts(p),
      [],
      "opening a closed thread reads no worker log",
    );
    assert.equal(
      await p.locator("[data-c-field=steer]").count(),
      0,
      "a closed task has no message box",
    );
    assert.match(
      await p.locator(".conversation-detail").innerText(),
      /This task is closed or no longer on the live board\. No worker message will be sent\./,
    );
    const measure = await opens(p);
    assert.deepEqual(
      measure.map((r) => r.operation),
      ["js.conversation.open.cold.cache"],
      "a closed first paint records its local source",
    );
    const request = await p.evaluate(() =>
      sent.find((m) => m.a === "completed" && m.board),
    );
    assert(request, "the board read runs in the background");
    assert.equal(
      request.transcript,
      undefined,
      "the board read names no transcript",
    );

    // The board read updates in place with the reader's row held.
    const reader = p.locator(".conversation-reader");
    await reader.evaluate((e) => {
      e.style.height = "160px";
      e.style.flex = "none";
    });
    await reader.hover();
    await p.mouse.wheel(0, 120);
    await p.waitForTimeout(260);
    const key = '[data-reading-key="attempt:3"]',
      y = () =>
        p
          .locator(key)
          .evaluate(
            (n) =>
              n.getBoundingClientRect().top -
              n.closest(".conversation-reader").getBoundingClientRect().top,
          ),
      before = await y();
    await p.evaluate(
      ({ request, reply }) =>
        taskq.receive({
          completed: { root: request.root, id: request.id, ...reply },
        }),
      { request, reply: boardReply },
    );
    await p.waitForTimeout(80);
    assert(
      Math.abs((await y()) - before) <= 2,
      "a board read holds the reader, moved " + ((await y()) - before),
    );
    assert.deepEqual(p.errors, []);
    return p;
  } catch (e) {
    await p.close();
    throw e;
  }
};

(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  try {
    await liveThread(browser);
    console.log("PASS live thread");
    const updated = await closedThread(browser, {
      tasks: [
        closedTask({
          title: "Closed fixture from the board",
          workers: [worker(0.5), worker(1), worker(2), worker(3)],
        }),
      ],
    });
    assert.match(
      await updated.locator(".conversation-reader").innerText(),
      /Worker 0\.5/,
      "the board read adds what it found in place",
    );
    await updated.close();
    console.log("PASS closed thread, board read");
    const failed = await closedThread(browser, {
      reason: "Board fixture refused",
    });
    const after = await failed.locator(".conversation-reader").innerText();
    assert.match(
      after,
      /Showing the local record\. Board read failed: Board fixture refused/,
    );
    assert.match(
      after,
      /Worker 3/,
      "a failed board read leaves the local records",
    );
    await failed.close();
    console.log("PASS closed thread, board failure");
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(String(e.stack || e).slice(0, 1500));
  process.exitCode = 1;
});
