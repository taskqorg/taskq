// #1831 A1-A5: real bundled view, invented native receipts. Rust checks own session selection and tool privacy.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  try {
    const page = await browser.newPage({
        viewport: { width: 900, height: 720 },
      }),
      errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    for (const runtime of ["claude", "codex"]) {
      const board = "https://example.org/alpha/issues",
        thread = "task:" + board + "/7";
      const task = {
        n: 7,
        title: "Live worker",
        state: "doing",
        role: "worker",
        attempt: 71,
        runtime,
        model: "DO_NOT_INFER_CURRENT_MODEL",
        local_session: true,
        session_id: "invented-native",
        session: runtime + ":invented-native fixture-host",
        executions: [{ attempt: 71, state: "running" }],
      };
      task.workers = [{ ...task }];
      const state = {
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
            tasks: [task],
          },
        ],
        ai: { history: [] },
        conversations: { complete: true, items: [] },
        messages: { complete: true, entries: [] },
      };
      await page.setContent(
        fs.readFileSync("build/web/workspace.html", "utf8"),
      );
      await page.evaluate(() => {
        window.sent = [];
        window.ipc = { postMessage: (text) => sent.push(JSON.parse(text)) };
      });
      await page.evaluate((s) => taskq.receive(s), state);
      await page.evaluate(
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
      async function read() {
        await page.waitForFunction(() =>
          sent.some(
            (m) => m.a === "conversation-dialogue" && m.action === "transcript",
          ),
        );
        return page.evaluate(() => {
          const i = sent.findIndex(
            (m) => m.a === "conversation-dialogue" && m.action === "transcript",
          );
          return sent.splice(i, 1)[0];
        });
      }
      async function reply(m, value) {
        await page.evaluate((r) => taskq.receive({ dialogueReceipt: r }), {
          id: m.id,
          conversation: thread,
          action: "transcript",
          state: "accepted",
          value,
        });
      }
      const panel = page.locator(".worker-live");
      let m = await read();
      assert.equal(
        m.session,
        undefined,
        "A2: browser must not select a native session",
      );
      assert.equal(m.attempt, 71);
      assert.equal(m.root, "fixture-alpha");
      const retryAt = Date.now();
      await page.evaluate((r) => taskq.receive({ dialogueReceipt: r }), {
        id: m.id,
        conversation: thread,
        action: "transcript",
        state: "unknown",
        reason: "Invented transcript unavailable",
      });
      m = await read();
      assert(
        Date.now() - retryAt >= 900,
        "A1/R12: unavailable initial transcripts retry at the one-second cadence",
      );
      await reply(m, {
        items: [
          {
            at: 0,
            i: 0,
            role: "assistant",
            text: "First worker message",
            model: "recorded-" + runtime,
            session: runtime + ":invented-native",
          },
          { at: 10, i: 0, role: "tool", text: "Bash" },
        ],
        end: 20,
      });
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("First worker message"),
      );
      assert.match(
        await panel.innerText(),
        /Tool · Bash/,
        "A1: first render includes tool lines",
      );
      assert.match(
        await panel.locator('[data-key="attempt:71:0:0"]').innerText(),
        new RegExp("recorded-" + runtime),
      );
      assert.doesNotMatch(
        await panel.innerText(),
        /DO_NOT_INFER_CURRENT_MODEL/,
        "historical blocks never borrow task defaults",
      );
      const began = Date.now();
      m = await read();
      assert.equal(m.after, 20);
      await reply(m, {
        items: [
          { at: 20, i: 0, role: "owner", text: "Please keep the draft" },
          { at: 30, i: 0, role: "assistant", text: "Appended reply" },
        ],
        end: 40,
      });
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("Appended reply"),
      );
      assert(
        Date.now() - began < 2000,
        "A1: appended owner and worker turns arrive within 2 s",
      );
      assert.match(
        await panel
          .locator('[data-key="attempt:71:20:0"] .history-block-head .meta')
          .innerText(),
        /^You(?: ·)? via the board/,
      );
      assert.equal(
        await panel
          .locator('[data-key="attempt:71:20:0"] .conversation-text')
          .innerText(),
        "Please keep the draft",
      );
      m = await read();
      await reply(m, {
        items: Array.from({ length: 25 }, (_, i) => ({
          at: 40 + i * 10,
          i: 0,
          role: "assistant",
          text: "Progress " + i + " " + "reading context ".repeat(12),
        })),
        end: 300,
      });
      const reader = page.locator(".conversation-reader");
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("Progress 24"),
      );
      assert(
        await reader.evaluate(
          (e) => e.scrollHeight - e.scrollTop - e.clientHeight < 2,
        ),
        "D4: follows while at the end",
      );
      await panel
        .locator('[data-key="attempt:71:90:0"]')
        .scrollIntoViewIfNeeded();
      await reader.dispatchEvent("scroll"); // explicit instrument intent; programmatic layout scroll is otherwise ignored
      await page.waitForFunction(
        () => !!document.querySelector("[data-c=worker-latest]"),
      );
      const anchor = await panel
        .locator('[data-key="attempt:71:90:0"]')
        .boundingBox();
      m = await read();
      await reply(m, {
        items: [
          { at: 300, i: 0, role: "assistant", text: "New while reading above" },
        ],
        end: 310,
      });
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("New while reading above"),
      );
      assert(
        Math.abs(
          (await panel.locator('[data-key="attempt:71:90:0"]').boundingBox())
            .y - anchor.y,
        ) <= 2,
        "D4: append preserves the reading anchor",
      );
      await page.locator("[data-c=worker-latest]").click();
      assert(
        await reader.evaluate(
          (e) => e.scrollHeight - e.scrollTop - e.clientHeight < 2,
        ),
        "D4: explicit latest resumes following",
      );
      m = await read();
      await reply(m, { items: [], end: 310 });
      await page.setViewportSize({ width: 540, height: 720 });
      await page.locator("[data-c=list]").click();
      await page.waitForTimeout(1200);
      assert.equal(
        await page.evaluate(
          () =>
            sent.filter(
              (m) =>
                m.a === "conversation-dialogue" && m.action === "transcript",
            ).length,
        ),
        0,
        "A3: the narrow list stops reading its hidden thread",
      );
      await page.locator("[data-c=list]").click();
      await page.setViewportSize({ width: 900, height: 720 });
      m = await read();
      await reply(m, { ended: true, items: [] });
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("Worker attempt ended"),
      );
      await page.waitForTimeout(1200);
      assert.equal(
        await page.evaluate(
          () =>
            sent.filter(
              (m) =>
                m.a === "conversation-dialogue" && m.action === "transcript",
            ).length,
        ),
        0,
        "A3: ended response stops polling",
      );
      // A replaced attempt starts with a new cursor; a late old reply cannot pollute it.
      task.attempt = 72;
      task.workers.push({ ...task, workers: undefined });
      task.executions = [{ attempt: 72, state: "running" }];
      await page.evaluate((s) => taskq.receive(s), state);
      m = await read();
      assert.equal(m.attempt, 72);
      assert.equal(m.after, undefined);
      await page.evaluate(() => taskq.receive({ open: { page: "sessions" } }));
      await reply(m, {
        items: [{ at: 40, i: 0, role: "assistant", text: "Late old reply" }],
        end: 50,
      });
      await page.waitForTimeout(1200);
      assert.equal(
        await page.evaluate(
          () =>
            sent.filter(
              (m) =>
                m.a === "conversation-dialogue" && m.action === "transcript",
            ).length,
        ),
        0,
        "A3: leaving the thread cancels the read",
      );
      assert.equal(await panel.count(), 0);
      task.state = "review";
      await page.evaluate((s) => taskq.receive(s), state);
      await page.evaluate(
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
      assert.equal(
        await panel.count(),
        1,
        "#2159: review retains worker history without a live worker",
      );
      m = await read();
      await reply(m, {
        items: [],
        start: 0,
        end: 0,
        size: 0,
        following: false,
        activity: "ended",
      });
      assert.equal(
        await page.locator("[data-a=answer-steer]:enabled").count(),
        0,
        "review history grants no worker steer",
      );
      task.state = "doing";
      task.attempt = 73;
      task.workers.push({ ...task, workers: undefined });
      task.executions = [{ attempt: 73, state: "running" }];
      await page.evaluate((s) => taskq.receive(s), state);
      m = await read();
      await page.evaluate(() => taskq.receive({ workerPanelClosed: true }));
      await reply(m, {
        items: [{ at: 0, i: 0, role: "assistant", text: "Hidden reply" }],
        end: 1,
      });
      await page.evaluate((s) => taskq.receive(s), state);
      await page.waitForTimeout(1200);
      assert.equal(
        await page.evaluate(
          () =>
            sent.filter(
              (m) =>
                m.a === "conversation-dialogue" && m.action === "transcript",
            ).length,
        ),
        0,
        "A3: closing the window cancels reads through subsequent snapshots",
      );
      await page.evaluate(
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
      m = await read();
      await page.evaluate(() => taskq.receive({ workerPanelClosed: true }));
      await reply(m, { items: [], end: 0 });
      await page.waitForTimeout(1200);
      assert.equal(
        await page.evaluate(
          () =>
            sent.filter(
              (m) =>
                m.a === "conversation-dialogue" && m.action === "transcript",
            ).length,
        ),
        0,
        "A3: native close cancels polling too",
      );
    }
    {
      // #1832 A1/A3: a worker on another host gets the same panel and request; its host answers over the host link, and
      // a host that does not answer reads unknown with the reason, never an empty session.
      const board = "https://example.org/alpha/issues",
        thread = "task:" + board + "/7";
      const task = {
        n: 7,
        title: "Remote worker",
        state: "doing",
        role: "worker",
        attempt: 81,
        runtime: "codex",
        local_session: false,
        host: "other-host",
        session: "codex:invented-native other-host",
        executions: [{ attempt: 81, state: "running" }],
      };
      await page.setContent(
        fs.readFileSync("build/web/workspace.html", "utf8"),
      );
      await page.evaluate(() => {
        window.sent = [];
        window.ipc = { postMessage: (text) => sent.push(JSON.parse(text)) };
      });
      await page.evaluate((s) => taskq.receive(s), {
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
            tasks: [task],
          },
        ],
        ai: { history: [] },
        conversations: { complete: true, items: [] },
        messages: { complete: true, entries: [] },
      });
      await page.evaluate(
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
      const take = async () => {
        await page.waitForFunction(() =>
          sent.some(
            (m) => m.a === "conversation-dialogue" && m.action === "transcript",
          ),
        );
        return page.evaluate(() => {
          const i = sent.findIndex(
            (m) => m.a === "conversation-dialogue" && m.action === "transcript",
          );
          return sent.splice(i, 1)[0];
        });
      };
      const panel = page.locator(".worker-live");
      let m = await take();
      assert.deepEqual(
        [m.attempt, m.root, m.session, m.host],
        [81, "fixture-alpha", undefined, undefined],
        "A2: the browser names the attempt, never a session or host",
      );
      assert.match(await panel.innerText(), /Worker 81.*codex/);
      await page.evaluate((r) => taskq.receive({ dialogueReceipt: r }), {
        id: m.id,
        conversation: thread,
        action: "transcript",
        state: "unknown",
        reason: "host other-host did not answer within 15 s",
      });
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("did not answer"),
      );
      const text = await panel.innerText();
      assert.match(
        text,
        /Worker 81 unavailable: host other-host did not answer within 15 s/,
      );
      assert.doesNotMatch(
        text,
        /No messages|Reading session/,
        "A3: unknown is never shown as an empty session",
      );
      m = await take();
      await page.evaluate((r) => taskq.receive({ dialogueReceipt: r }), {
        id: m.id,
        conversation: thread,
        action: "transcript",
        state: "accepted",
        value: {
          items: [
            { at: 0, i: 0, role: "assistant", text: "Remote worker line" },
          ],
          start: 0,
          end: 10,
          size: 10,
        },
      });
      await page.waitForFunction(() =>
        document
          .querySelector(".worker-live")
          ?.textContent.includes("Remote worker line"),
      );
      assert.equal(
        await panel.locator(".dialogue-unknown").count(),
        0,
        "the next answered read clears the unknown reason; missing native time remains explicit",
      );
      m = await take();
      assert.equal(
        m.after,
        10,
        "D4: the next read continues after the remote page",
      );
    }
    {
      // #2159 A4: the common document survives Ask/ended without granting steer or following a stopped worker.
      const retained = await browser.newPage({
          viewport: { width: 900, height: 720 },
        }),
        retainedErrors = [];
      retained.on("pageerror", (e) => retainedErrors.push(e.message));
      await retained.clock.install();
      const board = "https://example.org/retained/issues",
        thread = "task:" + board + "/7";
      const task = {
        n: 7,
        title: "Retained worker",
        state: "ask",
        role: "worker",
        attempt: 71,
        revision: 80,
        since: 200,
        card: "Done: read\nProblem: Inspect the retained source",
        question: { text: "Done: read\nProblem: Inspect the retained source" },
        worker: {
          attempt: 71,
          role: "worker",
          runtime: "codex",
          host: "fixture",
          local_session: true,
          session: "codex:retained-native fixture",
        },
        executions: [{ attempt: 71, state: "ended" }],
      };
      const state = {
        font: 20,
        theme: "dark",
        projects: [
          {
            root: "fixture-retained",
            name: "Invented",
            issues: board,
            alive: true,
            complete: true,
            tasks: [task],
          },
        ],
        messages: { complete: true, entries: [] },
        conversations: { complete: true, items: [] },
      };
      const src =
        "data:image/png;base64," +
        fs.readFileSync("tests/fixtures/rich-document.png").toString("base64");
      await retained.setContent(
        fs.readFileSync("build/web/workspace.html", "utf8"),
      );
      await retained.evaluate(
        ({ state, board, src }) => {
          window.sent = [];
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
                        generation: "invented-retained-source",
                        following: false,
                        activity: "waiting",
                        start: 0,
                        end: 300,
                        size: 300,
                        items: [
                          {
                            at: 100,
                            i: 0,
                            role: "assistant",
                            updated: 100000,
                            text: "## Retained code\n```javascript\nconst answer = 42;\n```",
                          },
                          {
                            at: 200,
                            i: 0,
                            role: "image",
                            updated: 300000,
                            text: "",
                            image: {
                              name: "Invented fixture content",
                              src,
                              width: 900,
                              height: 760,
                            },
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
              board,
              task: 7,
              root: "fixture-retained",
            },
          });
        },
        { state, board, src },
      );
      await retained.locator(".worker-live").waitFor({ timeout: 1500 });
      await retained.getByRole("heading", { name: "Retained code" }).waitFor();
      await retained.clock.runFor(100);
      assert.equal(
        await retained.locator(".current-question").count(),
        1,
        "current question remains present in the unified reader",
      );
      assert.equal(
        await retained.locator(".transcript-image img").getAttribute("alt"),
        "Invented fixture content",
      );
      assert(
        (await retained.locator("code .syntax-keyword").count()) > 0,
        "retained worker code uses the shared language renderer",
      );
      assert.equal(
        await retained.locator("[data-a=answer-steer]").count(),
        0,
        "Ask never offers worker steer",
      );
      assert(
        await retained
          .locator(".current-question")
          .evaluate((e) => !!e.closest(".dialogue-transcript")),
        "current question stays in the shared chronological reader",
      );
      assert.deepEqual(
        await retained
          .locator(".dialogue-transcript>article")
          .evaluateAll((es) => es.map((e) => e.dataset.time)),
        ["100000", "200000", "300000"],
        "question retains its source-time position between native blocks",
      );
      await retained.screenshot({
        path: require("node:path").join(
          require("node:os").tmpdir(),
          "taskq-2159-retained-ask.png",
        ),
      });
      await retained.clock.runFor(2500);
      assert.equal(
        await retained.evaluate(
          () => sent.filter((m) => m.action === "transcript").length,
        ),
        1,
        "Ask history is not polled",
      );
      await retained.locator("[data-c=worker-latest]").click();
      assert.equal(
        await retained.evaluate(
          () => sent.filter((m) => m.action === "transcript").length,
        ),
        2,
        "explicit Latest can reread retained content",
      );
      for (const stateName of ["later", "failed", "ready", "review"]) {
        task.state = stateName;
        task.attempt = null;
        task.role = stateName === "review" ? "reviewer" : null;
        task.session = null;
        task.question = null;
        task.card = null;
        task.revision++;
        await retained.evaluate((s) => taskq.receive(s), state);
        await retained.clock.runFor(1200);
        assert.equal(
          await retained.locator(".worker-live").count(),
          1,
          stateName + " retains the same worker document",
        );
        assert.equal(
          await retained
            .locator("[data-a=answer-steer]:enabled,[data-answer]")
            .count(),
          0,
          stateName + " read access grants no worker answer/steer",
        );
        assert.equal(
          await retained.evaluate(
            () => sent.filter((m) => m.action === "transcript").length,
          ),
          2,
          stateName + " cannot start polling",
        );
      }
      await retained.screenshot({
        path: require("node:path").join(
          require("node:os").tmpdir(),
          "taskq-2159-retained-worker.png",
        ),
      });
      await retained.evaluate(() => {
        window.ipc = { postMessage: (text) => sent.push(JSON.parse(text)) };
      });
      task.state = "doing";
      task.attempt = 71;
      task.role = "worker";
      task.executions = [{ attempt: 71, state: "running" }];
      task.revision++;
      await retained.evaluate((s) => taskq.receive(s), state);
      await retained.clock.runFor(1100);
      const pending = await retained.evaluate(() =>
        sent.filter((m) => m.action === "transcript").at(-1),
      );
      assert.equal(
        await retained.evaluate(
          () => sent.filter((m) => m.action === "transcript").length,
        ),
        3,
        "current running worker resumes the same read source",
      );
      task.state = "ask";
      task.revision++;
      await retained.evaluate((s) => taskq.receive(s), state);
      await retained.evaluate(
        (m) =>
          taskq.receive({
            dialogueReceipt: {
              ...m,
              state: "accepted",
              value: {
                generation: "invented-retained-source",
                following: true,
                activity: "working",
                start: 300,
                end: 300,
                size: 300,
                items: [],
              },
            },
          }),
        pending,
      );
      await retained.clock.runFor(2500);
      assert.equal(
        await retained.evaluate(
          () => sent.filter((m) => m.action === "transcript").length,
        ),
        3,
        "late Doing reply cannot restart polling in Ask",
      );
      assert.deepEqual(retainedErrors, []);
      assert.equal(
        await retained.evaluate(() =>
          sent.some(
            (m) =>
              m.a === "answer" ||
              ["reply", "send", "retry-turn"].includes(m.action),
          ),
        ),
        false,
        "read journey never sends or resumes input",
      );
      await retained.close();
    }
    assert.deepEqual(errors, []);
    console.log(
      "#1831/#1832 live worker panel: first render, append, attempt fence, teardown and remote host passed",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
