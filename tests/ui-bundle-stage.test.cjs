// #2365 A2, A7: a bundle publication has its own names and pointer, and never joins or displaces a binary release.
// Defects: a page changed after its manifest was written would publish; a bundle release marked latest would move DashQ's pinned assets.
"use strict";
const assert = require("node:assert"),
  fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path"),
  crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");
const stage = path.join(__dirname, "..", "scripts", "ui-bundle-stage.cjs");
const root = fs.mkdtempSync(path.join(os.tmpdir(), "ui-bundle-stage-")),
  web = path.join(root, "web"),
  out = path.join(root, "out");
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
try {
  fs.mkdirSync(web);
  const pages = {
      "tray.html": "<html>tray</html>",
      "workspace.html": "<html>workspace</html>",
      "operations.json": "[]",
    },
    files = {};
  for (const [name, text] of Object.entries(pages)) {
    fs.writeFileSync(path.join(web, name), text);
    files[name] = { sha256: sha(text), size: text.length };
  }
  fs.writeFileSync(
    path.join(web, "manifest.json"),
    JSON.stringify({
      schema: 1,
      bundle: "1800000000-abcdef012345",
      bridge: 7,
      files,
    }),
  );
  const ran = spawnSync(process.execPath, [stage, web, out], {
    encoding: "utf8",
  });
  assert.strictEqual(ran.status, 0, ran.stderr);
  assert.strictEqual(
    ran.stdout,
    "tag=ui-1800000000-abcdef012345\npointer=ui-latest-b7.json\n",
  );
  assert.deepStrictEqual(
    JSON.parse(fs.readFileSync(path.join(out, "ui-latest-b7.json"), "utf8")),
    {
      schema: 1,
      bundle: "1800000000-abcdef012345",
      bridge: 7,
      tag: "ui-1800000000-abcdef012345",
    },
  );
  const sums = fs
    .readFileSync(path.join(out, "SHA256SUMS"), "utf8")
    .trim()
    .split("\n")
    .map((line) => line.split("  "));
  assert.deepStrictEqual(
    sums.map((line) => line[1]).sort(),
    ["manifest.json", ...Object.keys(pages)]
      .map((name) => "taskq-ui-1800000000-abcdef012345-" + name)
      .sort(),
  );
  for (const [digest, name] of sums)
    assert.strictEqual(
      sha(fs.readFileSync(path.join(out, name))),
      digest,
      name,
    );
  fs.writeFileSync(path.join(web, "tray.html"), "<html>changed</html>");
  const refused = spawnSync(
    process.execPath,
    [stage, web, path.join(root, "second")],
    { encoding: "utf8" },
  );
  assert.notStrictEqual(
    refused.status,
    0,
    "a page that differs from its manifest must not publish",
  );
  assert.match(refused.stderr, /tray\.html differs from its manifest/);
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
const workflow = fs
  .readFileSync(
    path.join(__dirname, "..", ".github", "workflows", "release.yml"),
    "utf8",
  )
  .replace(/\r\n/g, "\n");
const job = workflow.slice(
  workflow.indexOf("  publish-ui:"),
  workflow.indexOf("\n  tag:\n"),
);
assert.match(
  job,
  /release-check\.py/,
  "a bundle needs the same exact-source check as a binary",
);
assert.doesNotMatch(job, /cargo /, "a bundle publication builds no binary");
for (const release of job.match(/gh release create [^\n]+/g))
  assert.match(
    release,
    /--prerelease --latest=false/,
    "a bundle release must never become latest: " + release,
  );
assert.ok(
  job.indexOf("dist/taskq-ui-*") < job.indexOf("dist/$POINTER"),
  "the pointer moves only after the bundle is up",
);
assert.match(
  workflow,
  /\n  tag:\n    if: github\.ref_name != 'ui-live'/,
  "a ui-live push must not tag a binary release",
);
console.log("ui bundle stage test passed");
