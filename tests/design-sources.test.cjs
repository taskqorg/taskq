// Chain: staged release answer-card bytes -> verifier; missing/corrupt assets must refuse.
const fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path"),
  assert = require("node:assert/strict");
const { verifyAnswerCard } = require("../scripts/design-sources.cjs");
const stage = fs.mkdtempSync(path.join(os.tmpdir(), "taskq-release-assets-"));
const assets = [
  ["build/web/answer-card.js", "answer-card.js"],
  ["src/answer-card.css", "answer-card.css"],
  ["tests/fixtures/answer-card.json", "answer-card-fixtures.json"],
  ["build/web/answer-card.sources.json", "answer-card-sources.json"],
];
try {
  for (const [source, suffix] of assets)
    fs.copyFileSync(source, path.join(stage, `taskq-test-${suffix}`));
  verifyAnswerCard(stage, "test");
  for (const [source, suffix] of assets) {
    const file = path.join(stage, `taskq-test-${suffix}`);
    fs.unlinkSync(file);
    assert.throws(() => verifyAnswerCard(stage, "test"), /ENOENT/);
    fs.writeFileSync(file, "corrupt staged asset");
    assert.throws(() => verifyAnswerCard(stage, "test"), /staged bytes/);
    fs.copyFileSync(source, file);
  }
  verifyAnswerCard(stage, "test");
  const manifest = "build/web/answer-card.sources.json",
    original = fs.readFileSync(manifest);
  try {
    fs.writeFileSync(manifest, "stale source binding");
    assert.throws(
      () => verifyAnswerCard(stage, "test"),
      /stale answer-card source manifest/,
    );
    fs.unlinkSync(manifest);
    assert.throws(() => verifyAnswerCard(stage, "test"), /ENOENT/);
  } finally {
    fs.writeFileSync(manifest, original);
  }
} finally {
  fs.rmSync(stage, { recursive: true, force: true });
}
console.log(
  "PASS staged release assets: valid, every missing asset, every corrupt asset",
);
