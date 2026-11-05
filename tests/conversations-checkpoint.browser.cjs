// #2366 A1/A2/A3: actual bundle; invented board/native IPC, never provider or board effects.
// Protect latest mixed opening and paragraph/focus/selection retention; timings qualify this browser only.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  const observations = [];
  try {
    for (const count of [12, 4096]) {
      const page = await browser.newPage({
          viewport: { width: 1100, height: 740 },
        }),
        errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.setContent(
        fs.readFileSync("build/web/workspace.html", "utf8"),
      );
      await page.evaluate((count) => {
        window.sent = [];
        window.historyCount = count;
        window.nativeCount = count;
        window.began = performance.now();
        const board = "https://example.org/invented/issues",
          id = "task:" + board + "/7";
        window.ipc = {
          postMessage: (text) => {
            const m = JSON.parse(text);
            sent.push(m);
            if (m.a !== "conversation-dialogue") return;
            if (m.action === "history") {
              if (m.event > count) {
                queueMicrotask(() =>
                  taskq.receive({
                    dialogueReceipt: {
                      ...m,
                      state: "unknown",
                      reason: "Requested exact event is unavailable",
                    },
                  }),
                );
                return;
              }
              const start = m.event
                  ? m.event - 1
                  : m.question
                    ? m.question - 1
                    : m.before !== undefined
                      ? Math.max(0, m.before - 64)
                      : m.start,
                end = Math.min(count, start + 64);
              queueMicrotask(() =>
                taskq.receive({
                  dialogueReceipt: {
                    ...m,
                    state: "accepted",
                    value: {
                      revision: 71,
                      start,
                      next: end,
                      total: count,
                      complete: true,
                      rows: Array.from({ length: end - start }, (_, i) => ({
                        question: null,
                        answered: {
                          text: "Board event " + (start + i),
                          at: 1700000000 + (start + i) * 2,
                          source: {
                            kind: "steer",
                            role: "owner",
                            id: start + i + 1,
                          },
                        },
                      })),
                    },
                  },
                }),
              );
            } else if (m.action === "transcript") {
              const end = m.before ?? nativeCount,
                start = m.after ?? Math.max(0, end - 64),
                stop = Math.min(
                  nativeCount,
                  m.after === undefined ? end : start + 64,
                );
              queueMicrotask(() =>
                taskq.receive({
                  dialogueReceipt: {
                    ...m,
                    state: "accepted",
                    value: {
                      generation: "invented-v1",
                      start,
                      end: stop,
                      size: nativeCount,
                      following: true,
                      activity: "working",
                      items: Array.from({ length: stop - start }, (_, i) => ({
                        at: start + i,
                        i: 0,
                        role: "assistant",
                        text:
                          "Native passage " +
                          (start + i) +
                          ". " +
                          "Readable paragraph. ".repeat(18),
                        updated: new Date(
                          (1700000001 + (start + i) * 2) * 1000,
                        ).toISOString(),
                      })),
                    },
                  },
                }),
              );
            }
          },
        };
        taskq.receive(
          (window.fixture = {
            font: 16,
            theme: "dark",
            host: "invented",
            projects: [
              {
                root: "fixture",
                name: "Invented",
                issues: board,
                alive: true,
                complete: true,
                tasks: [
                  {
                    n: 7,
                    title: "Mixed history",
                    state: "doing",
                    role: "worker",
                    attempt: 71,
                    revision: 71,
                    runtime: "codex",
                    session: "codex:invented invented",
                    local_session: true,
                    host: "invented",
                    executions: [{ attempt: 71, state: "running" }],
                    history_count: count,
                    history_lazy: true,
                  },
                ],
              },
            ],
            messages: { complete: true, entries: [] },
            conversations: {
              complete: true,
              items: [],
              navigation: { threads: { selected: id } },
            },
          }),
        );
      }, count);
      await page.locator('[data-reading-key^="event:"]').first().waitFor();
      await page.locator(".dialogue-transcript [data-key]").first().waitFor();
      await page.evaluate(
        () =>
          new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
          ),
      );
      const first = await page.evaluate(() => ({
        firstMeaningfulMs: performance.now() - began,
        initialBoard: sent.find((m) => m.action === "history"),
        nativeReads: sent.filter((m) => m.action === "transcript").length,
        boardReads: sent.filter((m) => m.action === "history").length,
        latestBoardResident: !![
          ...document.querySelectorAll('[data-reading-key^="event:"]'),
        ].find((e) =>
          e.textContent.includes("Board event " + (historyCount - 1)),
        ),
        latestNativeResident: document
          .querySelector(".dialogue-transcript")
          .textContent.includes("Native passage " + (historyCount - 1)),
      }));
      const anchor = await page
        .locator(".conversation-reader")
        .evaluate((el) => {
          el.scrollTop = Math.max(0, el.scrollHeight - el.clientHeight * 2);
          el.dispatchEvent(new Event("scroll"));
          const top = el.getBoundingClientRect().top,
            n = [...el.querySelectorAll("[data-native-part]")].find(
              (n) => n.getBoundingClientRect().bottom > top,
            );
          if (!n) return null;
          const range = document.createRange();
          range.selectNodeContents(n);
          getSelection().removeAllRanges();
          getSelection().addRange(range);
          return {
            key: n.closest("[data-key]").dataset.key,
            part: n.dataset.nativePart,
            y: n.getBoundingClientRect().top - top,
            text: getSelection().toString(),
          };
        });
      assert(anchor, "visible paragraph has an identity");
      await page.locator('[data-key="' + anchor.key + '"] .copy-block').focus();
      await page.evaluate(() => {
        nativeCount++;
      });
      await page.waitForFunction(
        (n) =>
          document
            .querySelector(".dialogue-transcript")
            .textContent.includes("Native passage " + n),
        count,
      );
      await page.setViewportSize({ width: 980, height: 740 });
      await page.evaluate(
        () =>
          new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
          ),
      );
      const after = await page.evaluate((a) => {
        const el = document.querySelector(".conversation-reader"),
          n = el.querySelector(
            '[data-key="' + a.key + '"] [data-native-part="' + a.part + '"]',
          );
        return {
          drift: n
            ? n.getBoundingClientRect().top -
              el.getBoundingClientRect().top -
              a.y
            : null,
          selection: getSelection().toString() === a.text,
          focus:
            document.activeElement.closest("[data-key]")?.dataset.key === a.key,
          nativeReads: sent.filter((m) => m.action === "transcript").length,
          forbidden: sent.some(
            (m) =>
              m.a === "answer" || ["send", "reply", "watch"].includes(m.action),
          ),
        };
      }, anchor);
      observations.push({ count, ...first, ...after });
      assert.equal(after.forbidden, false);
      assert.deepEqual(errors, []);
      if (!process.env.TASKQ_BASELINE_2366) {
        assert(
          first.latestBoardResident,
          "latest board page accompanies latest native history",
        );
        assert(first.latestNativeResident);
        assert.equal(after.selection, true);
        assert.equal(after.focus, true);
        assert(
          after.drift !== null && Math.abs(after.drift) <= 2,
          "paragraph drift stays within 2 CSS px",
        );
        const retained = async (label) => {
          await page.evaluate(
            () =>
              new Promise((r) =>
                requestAnimationFrame(() => requestAnimationFrame(r)),
              ),
          );
          const actual = await page.evaluate((a) => {
            const el = document.querySelector(".conversation-reader"),
              n = el.querySelector(
                '[data-key="' +
                  a.key +
                  '"] [data-native-part="' +
                  a.part +
                  '"]',
              );
            return {
              y:
                n?.getBoundingClientRect().top - el.getBoundingClientRect().top,
              text: getSelection().toString(),
              focus: document.activeElement.closest("[data-key]")?.dataset.key,
            };
          }, anchor);
          assert(
            Math.abs(actual.y - anchor.y) <= 2,
            label +
              " keeps source paragraph: " +
              JSON.stringify({ expected: anchor.y, actual: actual.y }),
          );
          assert.equal(
            actual.text,
            anchor.text,
            label + " keeps selected text",
          );
          assert.equal(
            actual.focus,
            anchor.key,
            label + " keeps focused source control",
          );
        };
        if (count > 64) {
          const before = await page.evaluate(
            () =>
              sent.filter(
                (m) => m.action === "transcript" && m.before !== undefined,
              ).length,
          );
          await page.evaluate(() =>
            document.querySelector("[data-c=worker-earlier]").click(),
          );
          await page.waitForFunction(
            (n) =>
              sent.filter(
                (m) => m.action === "transcript" && m.before !== undefined,
              ).length > n,
            before,
          );
          await retained("native prepend");
          const reads = await page.evaluate(
            () => sent.filter((m) => m.action === "history").length,
          );
          await page.evaluate(
            () =>
              document.querySelector(
                ".conversation-reader button:not([data-c])",
              )?.textContent,
          );
          await page
            .getByRole("button", { name: "Previous questions", exact: true })
            .evaluate((e) => e.click());
          await page.waitForFunction(
            (n) => sent.filter((m) => m.action === "history").length > n,
            reads,
          );
          await retained("board prepend");
          await page.evaluate((a) => {
            const record = document.querySelector('[data-key="' + a.key + '"]'),
              prior = record.previousElementSibling,
              spacer = document.createElement("div");
            spacer.dataset.delayedFixture = "";
            spacer.style.height = "137px";
            prior.prepend(spacer);
          }, anchor);
          await retained("late child size");
          await page.evaluate((a) => {
            const record = document.querySelector('[data-key="' + a.key + '"]'),
              details =
                record.previousElementSibling?.querySelector(".block-details");
            if (details) details.open = true;
          }, anchor);
          await retained("expanding Details");
        }
        await page.evaluate(() =>
          taskq.receive({
            open: {
              page: "conversations",
              root: "fixture",
              board: "https://example.org/invented/issues",
              task: 7,
              target: { kind: "event", id: "1" },
            },
          }),
        );
        await page.waitForFunction(() =>
          sent.some((m) => m.action === "history" && m.event === 1),
        );
        await page.locator('[data-event="1"]').waitFor();
        const exact = await page
          .locator('[data-event="1"]')
          .evaluate((n) =>
            Math.abs(
              n.getBoundingClientRect().top -
                n.closest(".conversation-reader").getBoundingClientRect().top,
            ),
          );
        assert(exact <= 2, "exact event outranks saved reading");
        await page.evaluate(() =>
          document.querySelector("[data-c=worker-latest]").click(),
        );
        await page.waitForFunction(
          (n) =>
            document
              .querySelector(".conversation-reader")
              .textContent.includes("Board event " + (n - 1)),
          count,
        );
        assert.equal(
          await page.locator(".history-target").count(),
          0,
          "explicit Latest clears the old exact target",
        );
      }
      await page.close();
    }
    console.log(
      JSON.stringify(
        {
          platform: process.platform,
          renderer:
            "Playwright Chromium (invented IPC; no native/HTTP measurement)",
          observations,
        },
        null,
        2,
      ),
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
