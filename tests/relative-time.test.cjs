// #1930 A2–A4: reject ambiguous/normalized dates; units never come from magnitude or transcript cursors.
"use strict";
const assert = require("node:assert/strict"),
  time = require("../src/relative-time.js");
const now = Date.parse("2026-10-31T12:00:00Z");
for (const bad of [
  null,
  undefined,
  "",
  0,
  -1,
  NaN,
  Infinity,
  {},
  "1793448000000",
  "2026-10-31",
  "2026-10-31T12:00:00",
  "2026-02-30T12:00:00Z",
  "2026-10-31T24:00:00Z",
  "2026-10-31T12:00:00+25:00",
])
  assert.equal(time.value(bad), null, String(bad));
assert.equal(time.value("2026-10-31T19:00:00+07:00"), now);
assert.equal(time.value(now), now);
assert.equal(time.seconds(now / 1000), now);
assert.equal(time.seconds(String(now / 1000)), null);
for (const [delta, want] of [
  [0, "<1m"],
  [-180000, "3m"],
  [-7200000, "2h"],
  [-345600000, "4d"],
  [-60000, "1m"],
  [-3600000, "1h"],
  [-86399000, "23h"],
  [-86400000, "1d"],
])
  assert.equal(time.label(now + delta, now), want);
assert.equal(
  time.label(now + 60000, now),
  "<1m",
  "a past moment recorded under clock skew reads as the shortest age, never a countdown",
);
for (const [delta, want] of [
  [0, "now"],
  [-60000, "now"],
  [1000, "0:01"],
  [60000, "0:01"],
  [3600000, "1:00"],
  [86400000, "1d"],
])
  assert.equal(time.label(now + delta, now, true), want);
// Chain: indicators.md Time -> the forms src/time.rs writes too. Risk: the CLI and a window read the same moment differently.
const forms = JSON.parse(
  require("node:fs").readFileSync("tests/fixtures/time-forms.json", "utf8"),
);
assert.equal(time.unknown, forms.unknown);
for (const [seconds, want] of forms.left)
  assert.equal(time.left(seconds), want, seconds + " s left");
for (const [seconds, want] of forms.age)
  assert.equal(time.age(seconds), want, seconds + " s old");
for (const [seconds, want] of forms.utc)
  assert.equal(time.utc(seconds * 1000), want);
const viewer = process.env.TZ;
try {
  for (const row of forms.local) {
    process.env.TZ = row.zone;
    assert.equal(
      time.local(row.t * 1000, row.now * 1000),
      row.want,
      JSON.stringify(row),
    );
  }
  process.env.TZ = "UTC";
  const at = forms.local[0].t * 1000;
  assert.equal(
    time.clock(at, at),
    "14:30",
    "the date is omitted while it does not affect interpretation",
  );
  assert.equal(time.clock(at, at + 86400000), "Thu Nov 05, 14:30");
} finally {
  if (viewer === undefined) delete process.env.TZ;
  else process.env.TZ = viewer;
}
const unknown = time.stamp(null, "Tool", "Native record has no timestamp");
assert.match(
  unknown.props["aria-label"],
  /Tool.*\? unavailable.*Native record/,
);
assert.equal(unknown.props.children, "? unavailable");
const due = time.stamp(Date.now() + 3600000, "Next", undefined, true, true);
assert.match(due.props.class, /time-future/);
assert.equal(due.props["data-time-left"], "");
assert.match(due.props.children, /^(0:59|1:00)$/);
const known = time.stamp("2026-10-31T19:00:00+07:00", "Message");
assert.equal(known.props["data-time-source"], "2026-10-31T19:00:00+07:00");
assert.match(known.props.title, /^Message · Sat Oct 31( 2026)?, \d\d:00 · /);
assert(
  known.props.title.endsWith(
    " · " + Intl.DateTimeFormat().resolvedOptions().timeZone,
  ),
);
const zone = process.env.TZ;
try {
  process.env.TZ = "UTC";
  assert.match(time.stamp(now, "Message").props.title, /, 12:00 · UTC$/);
  process.env.TZ = "Asia/Bangkok";
  assert.match(
    time.stamp(now, "Message").props.title,
    /, 19:00 · Asia\/Bangkok$/,
  );
} finally {
  if (zone === undefined) delete process.env.TZ;
  else process.env.TZ = zone;
}
console.log(
  "PASS #1930 relative formatting, explicit units, malformed dates and source attribution",
);
