// #2110 Q-A5/CS1–CS6: one whole record may exceed a reader cache; retain it and disclose protected overflow.
const assert = require("node:assert/strict"),
  ui = require("../src/ui-runtime.js"),
  conversations = require("../src/conversations.js"),
  { h } = require("preact");
ui.defer = () => 0;
ui.cancel = () => {};
const sent = [];
globalThis.ipc = { postMessage: (text) => sent.push(JSON.parse(text)) };
const api = conversations.create({
  window: {
    crypto: require("node:crypto").webcrypto,
    getSelection: () => null,
  },
  document: {},
  menu: {
    querySelector() {
      return null;
    },
    querySelectorAll() {
      return [];
    },
  },
  draw() {},
  navigate() {},
});
const failures = [],
  text = "Invented whole record ".repeat(60000),
  root = "fixture",
  sha = "a".repeat(40),
  report = { root, n: 7, sha };
for (const [name, check] of [
  [
    "long tool results retain every text page",
    () => {
      const item = {
          at: 99,
          i: 0,
          role: "tool",
          text: "Invented tool",
          result: "a".repeat(32768) + "last tool page",
        },
        nodes = (v) =>
          Array.isArray(v)
            ? v.flatMap(nodes)
            : v?.props
              ? [v, ...nodes(v.props.children)]
              : [];
      let rendered = api.nativeItem(item);
      const next = nodes(rendered).find(
        (v) => v.type === "button" && v.props.children === "Next text",
      );
      assert(next, "tool results expose Next text");
      next.props.onClick();
      rendered = api.nativeItem(item);
      assert.match(JSON.stringify(rendered), /last tool page/);
      assert(
        nodes(rendered).some(
          (v) => v.type === "button" && v.props.children === "Previous text",
        ),
      );
      assert.equal(item.result.length, 32782, "source remains whole");
    },
  ],
  [
    "task text pages stay scoped across equal task numbers and revisions",
    () => {
      const t = { n: 7, revision: 71 },
        text = "First paragraph. ".repeat(3000),
        nodes = (v) =>
          Array.isArray(v)
            ? v.flatMap(nodes)
            : v?.props
              ? [v, ...nodes(v.props.children)]
              : [];
      const first = api.taskText(t, text, "question", "fixture.one"),
        next = nodes(first).find(
          (v) => v.type === "button" && v.props.children === "Next text",
        );
      next.props.onClick();
      const a = JSON.stringify(
          api.taskText(t, text, "question", "fixture.one"),
        ),
        b = JSON.stringify(api.taskText(t, text, "question", "fixture.two"));
      assert.match(a, /Previous text/, "same source retains its page");
      assert(
        !b.includes("Previous text"),
        "another project starts at its own first page",
      );
      assert.notEqual(
        nodes(first).find((v) => v.props?.["data-reading-key"])?.props[
          "data-reading-key"
        ],
        nodes(api.taskText(t, text, "question", "fixture.two")).find(
          (v) => v.props?.["data-reading-key"],
        )?.props["data-reading-key"],
        "anchor identities include the exact source",
      );
    },
  ],
  // Shared reader budgets are divided across up to eight retained attempts. One intact record may exceed its share.
  [
    "shared native page discloses protected whole-record overflow",
    () => {
      const whole = text.repeat(4),
        item = { at: 0, i: 0, role: "assistant", text: whole },
        page = { items: [item], start: 0, end: 42 },
        node = {
          dataset: { key: "0:0" },
          closest() {
            return null;
          },
          getBoundingClientRect() {
            return { top: 0, bottom: 80 };
          },
        },
        reader = {
          clientHeight: 100,
          getBoundingClientRect() {
            return { top: 0, bottom: 100 };
          },
          querySelector() {
            return {
              querySelectorAll() {
                return [node];
              },
            };
          },
        };
      assert.equal(
        api.trim(reader, page, 75, 4 * 1024 * 1024),
        false,
        "visible whole record remains pinned",
      );
      assert.match(page.limitReason, /MiB.*pinned/);
      const rendered = JSON.stringify(api.nativeItem(page.items[0]));
      assert.match(
        rendered,
        /Message text/,
        "whole record stays accessible through bounded text pages",
      );
      assert.equal(page.items[0].text, whole, "source is never truncated");
      page.items = [{ ...item, text: "Small invented record" }];
      api.trim(reader, page, 75, 4 * 1024 * 1024);
      assert.equal(page.limitReason, "", "small source clears overflow");
    },
  ],
  [
    "recorded question page discloses its pinned bytes",
    () => {
      const p = { root, issues: "https://example.org/issues" },
        t = { n: 8, revision: 71, history_count: 1, history_lazy: true };
      let retained;
      const render = (t) => {
        retained = t.history;
        return h("p", null, "Invented question available");
      };
      api.history(p, t, render);
      const request = sent.at(-1),
        rows = [{ question: { text }, answered: { text: "Invented answer" } }];
      api.receive({
        dialogueReceipt: {
          ...request,
          state: "accepted",
          value: {
            revision: 71,
            start: 0,
            next: 1,
            total: 1,
            complete: true,
            rows,
          },
        },
      });
      assert.match(
        JSON.stringify(api.history(p, t, render)),
        /MiB pinned while reading/,
      );
      assert.equal(retained[0].question.text, text);
      assert.equal(api.bounds().historyRecords, 1);
      assert(api.bounds().historyBytes > 1024 * 1024);
      api.history(p, { ...t, n: 9, revision: 72 }, render);
      const next = sent.at(-1);
      api.receive({
        dialogueReceipt: {
          ...next,
          state: "accepted",
          value: {
            revision: 72,
            start: 0,
            next: 1,
            total: 1,
            complete: true,
            rows: [{ question: { text: "Small invented record" } }],
          },
        },
      });
      assert.equal(
        api.bounds().historyRecords,
        1,
        "the earlier body is evicted, not retained alongside the next source",
      );
      assert(
        !JSON.stringify(
          api.history(p, { ...t, n: 9, revision: 72 }, render),
        ).includes("MiB pinned while reading"),
        "small replacement clears overflow",
      );
    },
  ],
])
  try {
    check();
    console.log("PASS " + name);
  } catch (e) {
    failures.push(name + ": " + e.message.split("\n")[0]);
  }
assert.deepEqual(failures, []);
