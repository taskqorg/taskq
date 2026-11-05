// Chain: F16/F17, Q-A4/Q-A5: shared chronological questions retain 64-record, revision-fenced source paging and confirmed tails.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  try {
    const page = await browser.newPage(),
      errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.setContent(fs.readFileSync("build/web/workspace.html", "utf8"));
    await page.evaluate(() => {
      window.reads = [];
      window.ipc = {
        postMessage: (text) => {
          const m = JSON.parse(text);
          if (m.action !== "history") return;
          reads.push(m);
          const start =
            m.before === undefined ? m.start : Math.max(0, m.before - 64);
          queueMicrotask(() =>
            taskq.receive({
              dialogueReceipt: {
                ...m,
                state: "accepted",
                value: {
                  revision: 71,
                  start,
                  next: start + 64,
                  total: 100000,
                  complete: true,
                  rows: Array.from({ length: 64 }, (_, i) => ({
                    question: {
                      text: "Invented recorded question " + (start + i),
                      choices: [],
                      evidence: [],
                    },
                    answered: {
                      revision: start + i + 1,
                      text: "Invented recorded answer " + (start + i),
                    },
                  })),
                },
              },
            }),
          );
        },
      };
      taskq.receive(
        (window.fixture = {
          font: 20,
          theme: "dark",
          projects: [
            {
              root: "fixture",
              name: "Invented",
              issues: "https://example.org/issues",
              alive: true,
              complete: true,
              tasks: [
                {
                  n: 7,
                  title: "Current question first",
                  state: "ask",
                  revision: 71,
                  history_count: 100000,
                  history_lazy: true,
                  question: {
                    text: "Current invented question",
                    choices: [],
                    evidence: [],
                  },
                },
              ],
              events: [],
              reports: { complete: true, reports: [], history: [] },
            },
          ],
          messages: { complete: true, entries: [] },
          conversations: {
            complete: true,
            items: [],
            navigation: {
              threads: { selected: "task:https://example.org/issues/7" },
            },
          },
        }),
      );
    });
    assert.equal(
      await page.evaluate(() => reads.length),
      1,
      "F16 loads one bounded history page in the shared reader",
    );
    await page.waitForFunction(
      () => document.querySelectorAll(".asked").length === 64,
    );
    assert.match(
      await page.locator(".conversation-reader").innerText(),
      /Recorded questions and owner messages · 100000/,
    );
    assert.equal(
      await page.evaluate(() => reads[0].before),
      100000,
      "A3 first open reads the latest board page",
    );
    await page
      .getByRole("button", { name: "Previous questions", exact: true })
      .click();
    await page
      .getByRole("article", {
        name: "Question 99873 of Invented#7",
        exact: true,
      })
      .waitFor();
    assert.equal(
      await page.locator(".asked").count(),
      128,
      "prepend retains the previous visible page",
    );
    assert.equal(await page.evaluate(() => reads.at(-1).before), 99936);
    await page
      .getByRole("article", {
        name: "Question 99937 of Invented#7",
        exact: true,
      })
      .waitFor();
    assert.equal(
      await page.locator(".current-question").count(),
      1,
      "current question occurs once in the shared history",
    );
    await page.evaluate(() => {
      const t = fixture.projects[0].tasks[0];
      Object.assign(t, {
        state: "doing",
        revision: 72,
        history_source_revision: 71,
        history_source_count: 100000,
        history_count: 100001,
        history_tail: true,
        history: [
          {
            question: {
              text: "Confirmed receipt question",
              choices: [],
              evidence: [],
            },
            answered: { revision: 72, by: "you", text: "Ship" },
          },
        ],
      });
      taskq.receive(fixture);
    });
    await page
      .getByRole("article", { name: "Question 72 of Invented#7", exact: true })
      .waitFor();
    await page
      .getByText("Confirmed receipt question", { exact: true })
      .waitFor();
    assert.equal(
      await page.locator(".asked [data-qac=choose]:not(:disabled)").count(),
      0,
      "historical choices stay read-only",
    );
    const confirmed = page.locator('[data-reading-key="answer:7:72"]');
    await confirmed.waitFor();
    assert.match(
      await confirmed.locator(".history-block-head .meta").innerText(),
      /Answered by you/,
    );
    assert.equal(
      await confirmed.locator(".conversation-text").innerText(),
      "Ship",
    );
    if (process.env.TASKQ_EVIDENCE_2110) {
      fs.mkdirSync(process.env.TASKQ_EVIDENCE_2110, { recursive: true });
      await page.screenshot({
        path: process.env.TASKQ_EVIDENCE_2110 + "/pending-tail.png",
      });
    }
    assert.equal(
      await page.evaluate(() => reads.at(-1).revision),
      71,
      "older source prefix uses its own immutable revision, independent of the accepted receipt",
    );
    assert.match(
      await page.locator(".conversation-reader").innerText(),
      /Recorded questions and owner messages · 100001/,
    );
    for (const target of [
      { kind: "event", id: "72" },
      { kind: "question", revision: 72 },
    ]) {
      const before = await page.evaluate(() => reads.length);
      await page.evaluate(
        (target) =>
          taskq.receive({
            open: {
              page: "conversations",
              root: "fixture",
              board: "https://example.org/issues",
              task: 7,
              target,
            },
          }),
        target,
      );
      await page.waitForFunction((before) => reads.length > before, before);
      const request = await page.evaluate(() => reads.at(-1));
      assert.equal(request.before, 100000);
      assert.equal(request.event, undefined);
      assert.equal(
        request.question,
        undefined,
        "confirmed targets never ask the prefix for a missing receipt",
      );
      await page.evaluate(
        () =>
          new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
          ),
      );
      assert(
        !/Recorded questions changed|unavailable in this exact/.test(
          await page.locator(".conversation-reader").innerText(),
        ),
      );
      const targetNode =
        target.kind === "event"
          ? confirmed
          : page.locator('[data-question="72"]');
      const position = await targetNode.evaluate((n) => {
        const reader = n.closest(".conversation-reader");
        return {
          y: n.getBoundingClientRect().top - reader.getBoundingClientRect().top,
          height: reader.clientHeight,
          bottom: reader.scrollHeight - reader.scrollTop - reader.clientHeight,
        };
      });
      assert(
        position.y >= -2 &&
          position.y < position.height &&
          (position.y <= 2 || position.bottom <= 2),
        "exact receipt target is aligned, or visibly clamped at the native scroll end: " +
          JSON.stringify(position),
      );
    }
    assert.equal(
      await page.evaluate(() =>
        reads.some((m) => ["send", "reply", "answer"].includes(m.action)),
      ),
      false,
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS #2110 100k recorded-question total, bounded initial body read, 64-record pages, exact source revision and previous cursor; current question remains single in chronological history",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
