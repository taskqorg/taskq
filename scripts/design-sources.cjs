// Design source manifests (#1841): docs/design/<name>.sources.json lists the pinned files and icons;
// `npm run build` derives their SHA-256 (UTF-8, LF) into build/web/<name>.sources.json, which the release publishes.
// Kept out of build-web.cjs: that file is a README shots UI source (docs/shots/update.sh), and this step changes no tray byte.
"use strict";
const fs = require("node:fs"),
  path = require("node:path"),
  crypto = require("node:crypto");
const root = path.resolve(__dirname, "..");
const names = [
  "answer-card",
  "project-blocks",
  "finding-composer",
  "pm-composer",
  "task-workspace",
];
const sha = (data) => crypto.createHash("sha256").update(data).digest("hex");
const text = (file) =>
  fs.readFileSync(path.join(root, file), "utf8").replace(/\r\n/g, "\n");
const render = (manifest) => JSON.stringify(manifest, null, 2) + "\n";
function derive(name) {
  const manifest = JSON.parse(text("docs/design/" + name + ".sources.json")),
    derived = { ...manifest };
  // A listed manifest pins the derived manifest, not its committed file list.
  derived.files = Object.fromEntries(
    manifest.files.map((file) => {
      const dep = /^docs\/design\/(.+)\.sources\.json$/.exec(file);
      return [file, sha(dep ? render(derive(dep[1])) : text(file))];
    }),
  );
  if (manifest.icons) {
    const { assets } = require(path.join(root, "src/project-blocks.js"));
    derived.icons = Object.fromEntries(
      manifest.icons.map((icon) => [icon, sha(assets[icon])]),
    );
  }
  return derived;
}
function verifyAnswerCard(directory, version) {
  const assert = require("node:assert/strict");
  const derived = derive("answer-card");
  assert.deepEqual(Object.keys(derived.files), [
    "src/answer-card.js",
    "src/reply-files.js",
    "src/answer-card.css",
    "tests/fixtures/answer-card.json",
  ]);
  assert.equal(
    text("build/web/answer-card.sources.json"),
    render(derived),
    "stale answer-card source manifest",
  );
  for (const [source, suffix] of [
    ["build/web/answer-card.js", "answer-card.js"],
    ["src/answer-card.css", "answer-card.css"],
    ["tests/fixtures/answer-card.json", "answer-card-fixtures.json"],
    ["build/web/answer-card.sources.json", "answer-card-sources.json"],
  ]) {
    assert.equal(
      sha(fs.readFileSync(path.join(directory, `taskq-${version}-${suffix}`))),
      sha(fs.readFileSync(path.join(root, source))),
      "staged bytes: " + suffix,
    );
  }
}
module.exports = { names, derive, render, verifyAnswerCard };
if (require.main === module && process.argv[2] === "--verify-release") {
  if (process.argv.length !== 5)
    throw Error("usage: --verify-release DIST VERSION");
  verifyAnswerCard(path.resolve(process.argv[3]), process.argv[4]);
  console.log("PASS staged answer-card files and source binding");
} else if (require.main === module) {
  const out = path.resolve(process.argv[2] || path.join(root, "build/web"));
  fs.mkdirSync(out, { recursive: true });
  for (const name of names)
    fs.writeFileSync(
      path.join(out, name + ".sources.json"),
      render(derive(name)),
    );
}
