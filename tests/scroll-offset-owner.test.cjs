// Chain: Scroll position (taskq.md) -> docs/design/ai-workspace.md § Scroll position and § Task thread -> scripts/check-web.cjs, src/ui-runtime.js and src/conversations.js.
// Risk: a surface reads or writes a scroll offset outside the anchor, its fact never reaches telemetry, a page outgrows the telemetry method bound, or a record without a time leaves its source.
const assert = require("node:assert/strict"),
  fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path");
const messages = [];
global.ipc = { postMessage: (text) => messages.push(JSON.parse(text)) };
const { scrollViolations } = require("../scripts/check-web.cjs"),
  ui = require("../src/ui-runtime.js"),
  conversations = require("../src/conversations.js");

const root = fs.mkdtempSync(path.join(os.tmpdir(), "taskq-scroll-"));
try {
  fs.mkdirSync(path.join(root, "src", "nested"), { recursive: true });
  const write = (name, text) =>
    fs.writeFileSync(path.join(root, "src", name), text);
  write("reading-anchor.js", "viewport.scrollTop = 1;\n");
  write("tray.rs", 'let harness = "el.scrollTop = 1";\n');
  write("clean.js", "const box = el.getBoundingClientRect();\n");
  write("read.js", "\nconst y = el.scrollTop;\n");
  write("nested/to.js", "el.scrollTo(0, 1);\n");
  write("by.js", "el.scrollBy(0, 1);\n");
  assert.deepEqual(
    scrollViolations(root).sort(),
    ["by.js:1", "nested/to.js:1", "read.js:2"],
    "the check fails on any scroll offset outside the owner and the named exception",
  );
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
assert.deepEqual(
  scrollViolations(path.join(__dirname, "..")),
  [],
  "the source holds no scroll offset outside the anchor",
);

const fact = ui.measured("scroll.drift", ["tasks"], ["refresh"]);
assert.equal(
  fact(["tasks", "refresh"], { outcome: "failed", error: "scroll-drift" }),
  true,
);
const row = messages.at(-1).row;
assert.equal(row.operation, "js.scroll.drift.tasks.refresh");
assert.deepEqual(
  [row.kind, row.outcome, row.error],
  ["ui", "failed", "scroll-drift"],
);
assert(row.trace && row.span, "the action reader needs a trace and a span");
const posted = messages.length;
assert.equal(
  fact(["private", "refresh"]),
  false,
  "a name outside the closed lists is refused",
);
assert.equal(messages.length, posted, "a refused name posts nothing");

const results = [];
for (let n = 0; n < 320; n++)
  results.push(ui.execution.module("bound.m" + n, { read: () => n }).read());
assert.deepEqual(
  results,
  Array.from({ length: 320 }, (_, n) => n),
  "a page past the telemetry method bound keeps working",
);

const order = (items) =>
  conversations.chronological(items).map((item) => item.id);
assert.deepEqual(
  order([
    { id: "b1", source: "b", updated: 30 },
    { id: "a1", source: "a", updated: 10 },
    { id: "a2", source: "a" },
    { id: "a3", source: "a", updated: 40 },
    { id: "b0", source: "b" },
  ]),
  ["a1", "a2", "b1", "b0", "a3"],
  "records read in source-time order and a record without a time stays among its source's records",
);
assert.deepEqual(
  order([
    { id: "c1", source: "c" },
    { id: "a1", source: "a", updated: 10 },
  ]),
  ["a1", "c1"],
  "a source without any time stays visible after the timed records",
);
console.log(
  "PASS scroll offsets stay in the anchor, drift facts reach the action reader, the telemetry bound holds and untimed records stay with their source",
);
