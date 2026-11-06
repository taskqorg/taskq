// Native Windows proof of docs/design/ui-bundle.md (#2365 A2-A6, A8): the candidate tray, its own service and a stand-in
// worker run from a private install and temp folder, adopt bundles from a local publication, and are read over WebView2's
// debugging port. Nothing outside the private folder changes. Usage after `node scripts/build-web.cjs build/web` and
// `cargo build --locked`: node scripts/ui-bundle-proof.cjs <out-dir>. Windows open and close on the desktop for about 3 minutes.
// `--store <install>` only writes an adopted bundle into an install, for `TASKQ_UI_USE_STORE=1 taskq tray --snapshot` on macOS,
// which has no debugging port: that page tries to leave for a remote URL and still shows its banner if the fence refused it.
"use strict";
const fs = require("node:fs"),
  path = require("node:path"),
  os = require("node:os"),
  crypto = require("node:crypto"),
  http = require("node:http");
const { spawn, spawnSync } = require("node:child_process"),
  assert = require("node:assert/strict");
const repo = path.join(__dirname, ".."),
  out = path.resolve(
    process.argv[2] ||
      path.join(require("os").tmpdir(), "taskq-ui-bundle-proof"),
  );
const base = fs.mkdtempSync(path.join(os.tmpdir(), "ui-proof-")),
  install = path.join(base, "install"),
  temp = path.join(base, "temp"),
  channel = path.join(base, "publication");
const port = 19000 + (process.pid % 500),
  stage = path.join(repo, "scripts", "ui-bundle-stage.cjs"),
  web = path.join(repo, "build", "web");
const evidence = { platform: "Windows WebView2", steps: {} },
  children = [];
const wait = (ms) => new Promise((done) => setTimeout(done, ms));
const sha = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");
const version = /^version = "(.+)"/m.exec(
  fs.readFileSync(path.join(repo, "Cargo.toml"), "utf8"),
)[1];
const embedded = JSON.parse(
  fs.readFileSync(path.join(web, "manifest.json"), "utf8"),
);
const epoch = Number(embedded.bundle.split("-")[0]);
const environment = {
  ...process.env,
  TASKQ_INSTALL: install,
  TEMP: temp,
  TMP: temp,
  TASKQ_HOST: "ui-proof-" + process.pid,
  TASKQ_INCIDENT_REPORTS: "off",
  TASKQ_UI_RELEASE_DIR: channel,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=" + port,
  WEBVIEW2_USER_DATA_FOLDER: path.join(base, "webview"),
};
for (const name of [
  "TASKQ_TASK",
  "TASKQ_ATTEMPT",
  "TASKQ_ROLE",
  "TASKQ_ROOT",
  "CODEX_THREAD_ID",
  "GH_TOKEN",
  "GITHUB_TOKEN",
  "GITLAB_TOKEN",
  "COREQ_TRACE_CONTEXT",
])
  delete environment[name];

function start(name, args) {
  const log = fs.openSync(path.join(out, name + ".log"), "w");
  const child = spawn(
    "cargo",
    [
      "run",
      "-q",
      "--locked",
      "--manifest-path",
      path.join(repo, "Cargo.toml"),
      "--",
      ...args,
    ],
    { cwd: base, env: environment, stdio: ["ignore", log, log] },
  );
  children.push(child);
  return child;
}
const json = (url) =>
  new Promise((done, fail) =>
    http
      .get(url, (r) => {
        let body = "";
        r.on("data", (c) => (body += c));
        r.on("end", () => {
          try {
            done(JSON.parse(body));
          } catch (e) {
            fail(e);
          }
        });
      })
      .on("error", fail),
  );
async function until(what, test, ms = 120000) {
  const deadline = Date.now() + ms;
  let last;
  while (Date.now() < deadline) {
    try {
      const value = await test();
      if (value) return value;
    } catch (e) {
      last = e;
    }
    await wait(300);
  }
  throw Error("timed out: " + what + (last ? " (" + last.message + ")" : ""));
}
class Page {
  constructor(socket, id) {
    this.socket = socket;
    this.id = id;
    this.next = 1;
    this.calls = new Map();
    socket.addEventListener("message", (e) => {
      const m = JSON.parse(e.data);
      const call = this.calls.get(m.id);
      if (call) {
        this.calls.delete(m.id);
        m.error ? call.fail(Error(m.error.message)) : call.done(m.result);
      }
    });
  }
  send(method, params = {}) {
    const id = this.next++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((done, fail) => this.calls.set(id, { done, fail }));
  }
  async eval(expression) {
    const r = await this.send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (r.exceptionDetails)
      throw Error(
        r.exceptionDetails.exception?.description || r.exceptionDetails.text,
      );
    return r.result.value;
  }
  async shot(name) {
    const r = await this.send("Page.captureScreenshot", { format: "png" });
    fs.writeFileSync(
      path.join(out, name + ".png"),
      Buffer.from(r.data, "base64"),
    );
  }
  close() {
    this.socket.close();
  }
}
async function attach(target) {
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    socket.addEventListener("open", done);
    socket.addEventListener("error", fail);
  });
  return new Page(socket, target.id);
}
// Webviews are told apart by what the binary injects: only Conversations has a native frame flag.
async function page(kind) {
  const targets = (
    await json("http://127.0.0.1:" + port + "/json/list")
  ).filter((t) => t.type === "page");
  for (const target of targets) {
    const candidate = await attach(target);
    try {
      if (
        (await candidate.eval(
          "window.taskqNativeFrame===true&&!!window.taskq",
        )) ===
          (kind === "workspace") &&
        (await candidate.eval("!!window.taskq"))
      )
        return candidate;
    } catch {}
    candidate.close();
  }
  throw Error("no " + kind + " page yet");
}
const bundleOf = (p) =>
  p.eval(
    "document.getElementById('ui-proof-banner')?.dataset.bundle||'embedded'",
  );
const onBundle = async (kind, bundle) => {
  const p = await page(kind);
  const on =
    (await bundleOf(p)) === bundle &&
    (await p.eval("document.readyState")) === "complete";
  if (on) return p;
  p.close();
  return null;
};
const store = () => {
  try {
    return JSON.parse(
      fs.readFileSync(path.join(install, "ui", "state.json"), "utf8"),
    );
  } catch {
    return {};
  }
};
const serviceRecord = (dir) => {
  try {
    return (
      JSON.parse(fs.readFileSync(path.join(dir, "service"), "utf8")).pid ?? null
    );
  } catch {
    return null;
  }
};
const trayPid = () =>
  Number(fs.readFileSync(path.join(install, "tray"), "utf8").trim());

// One bundle-only publication: the candidate pages with one visible change, staged by the same script as release.yml.
let serial = 0;
function pages(dir, bundle, { label, bridge = embedded.bridge, tail = "" }) {
  const files = {};
  fs.mkdirSync(dir, { recursive: true });
  for (const name of Object.keys(embedded.files)) {
    let bytes = fs.readFileSync(path.join(web, name));
    if (name.endsWith(".html")) {
      const banner = `<div id="ui-proof-banner" data-bundle="${bundle}" style="position:fixed;right:8px;bottom:8px;z-index:99;padding:2px 8px;background:#b45309;color:#fff;font:12px sans-serif">${label}</div>${tail}</body>`;
      bytes = Buffer.from(
        bytes
          .toString("utf8")
          .replace(/<\/body>(?![\s\S]*<\/body>)/, () => banner),
      );
    }
    fs.writeFileSync(path.join(dir, name), bytes);
    files[name] = { sha256: sha(bytes), size: bytes.length };
  }
  fs.writeFileSync(
    path.join(dir, "manifest.json"),
    JSON.stringify({ schema: 1, bundle, bridge, files }),
  );
}
function publish({
  label,
  bridge = embedded.bridge,
  tail = "",
  damage = false,
}) {
  serial++;
  const bundle = epoch + serial * 100 + "-" + "a".repeat(11) + serial,
    dir = path.join(base, "build-" + serial);
  pages(dir, bundle, { label, bridge, tail });
  const staged = path.join(base, "staged-" + serial);
  const ran = spawnSync(process.execPath, [stage, dir, staged], {
    encoding: "utf8",
  });
  assert.equal(ran.status, 0, ran.stderr);
  const tag = "ui-" + bundle,
    release = path.join(channel, tag),
    pointer = path.join(staged, "ui-latest-b" + bridge + ".json");
  fs.mkdirSync(release, { recursive: true });
  fs.mkdirSync(path.join(channel, "ui-channel"), { recursive: true });
  for (const name of fs.readdirSync(staged))
    if (name.startsWith("taskq-ui-") || name === "SHA256SUMS")
      fs.copyFileSync(path.join(staged, name), path.join(release, name));
  if (damage)
    fs.appendFileSync(
      path.join(release, "taskq-ui-" + bundle + "-tray.html"),
      "<!-- changed after SHA256SUMS -->",
    );
  const value = JSON.parse(fs.readFileSync(pointer, "utf8"));
  value.bridge = embedded.bridge;
  fs.writeFileSync(
    path.join(channel, "ui-channel", "ui-latest-b" + embedded.bridge + ".json"),
    JSON.stringify(value),
  );
  return bundle;
}
const check = (menu) =>
  menu.eval("window.ipc.postMessage(JSON.stringify({a:'ui-check'}))");
// An open, unpinned menu keeps its page until it closes; the proof closes and opens it as a reader does.
const hide = (menu) =>
  menu.eval("window.ipc.postMessage(JSON.stringify({a:'hide'}))");
const raise = async () => {
  fs.writeFileSync(path.join(temp, "taskq-tray.raise"), "");
  await wait(1500);
};

function seed() {
  for (const dir of [install, temp, channel, out])
    fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(install, "current"), version);
  const body = Buffer.from("Attachment kept across adoption\n"),
    hash = sha(body),
    file = {
      id: "proof-file",
      name: "proof-notes.md",
      mime: "text/markdown",
      hash,
      bytes: body.length,
    };
  const conversations = {
    "proof-selected": {
      revision: 1,
      title: "Draft kept across adoption",
      text: Array.from(
        { length: 300 },
        (_, i) => "Reading line " + (i + 1),
      ).join("\n"),
      files: [file],
    },
  };
  for (let i = 0; i < 60; i++)
    conversations["proof-row-" + i] = {
      revision: 1,
      title: "Other draft " + i,
      text: "Other",
    };
  fs.writeFileSync(
    path.join(install, "tray-requests.json"),
    JSON.stringify({ conversations }),
  );
  const attachments = path.join(
    install,
    "setup",
    "proof-selected",
    "attachments",
  );
  fs.mkdirSync(attachments, { recursive: true });
  fs.writeFileSync(path.join(attachments, hash), body);
}
const readDraft = `(()=>{const f=document.querySelector('[data-c-field=text]');const row=document.querySelector('.conversation-row[aria-current=page]');const reader=document.querySelector('.conversation-reader');return {text:f?.value,caret:f?[f.selectionStart,f.selectionEnd]:null,focused:document.activeElement===f,selected:row?.dataset.id||row?.dataset.thread||null,selectedText:row?.textContent||null,files:document.body.textContent.includes('proof-notes.md'),reader:reader?.scrollTop};})()`;
// The errors list is the place the reason stays visible; open it for the picture and return its text.
const openErrors = (menu) =>
  menu.eval(
    "(()=>{const panel=document.querySelector('details.errors-panel');panel.open=true;return document.querySelector('.errors-list').textContent;})()",
  );

async function main() {
  seed();
  const realService = serviceRecord(
    path.join(process.env.LOCALAPPDATA || "", "taskq"),
  );
  const worker = spawn(process.execPath, ["-e", "setInterval(()=>{},1000)"], {
    stdio: "ignore",
  });
  children.push(worker);
  start("service", ["service", "run"]);
  await until("private service", () => serviceRecord(install), 300000);
  const service = serviceRecord(install);
  start("tray", ["tray"]);

  // A5: a fresh install with nothing published opens from its embedded copy.
  let menu = await until("menu page", () => page("menu"), 300000);
  await raise();
  await until("menu ready", () =>
    menu.eval(
      "document.querySelector('[data-route]')!==null||document.body.textContent.length>40",
    ),
  );
  await menu.eval(
    "window.ipc.postMessage(JSON.stringify({a:'workspace',open:{page:'conversations'}}))",
  );
  let workspace = await until("Conversations page", () => page("workspace"));
  await until("rows", () =>
    workspace.eval("document.querySelectorAll('.conversation-row').length>10"),
  );
  evidence.steps.fresh_offline = {
    menu: await bundleOf(menu),
    conversations: await bundleOf(workspace),
    state: store(),
  };
  assert.equal(evidence.steps.fresh_offline.menu, "embedded");
  assert.equal(evidence.steps.fresh_offline.conversations, "embedded");
  await workspace.shot("embedded-conversations");
  await menu.shot("embedded-menu");

  // A4: an open draft, its file, the selected conversation and the reading position.
  await workspace.eval(
    "document.querySelector('.conversation-row[data-id=\"proof-selected\"]').click()",
  );
  await until("selected draft", () =>
    workspace.eval(
      "!!document.querySelector('[data-c-field=text]')&&document.querySelector('[data-c-field=text]').value.startsWith('Reading line 1')",
    ),
  );
  await workspace.eval(
    "(()=>{const f=document.querySelector('[data-c-field=text]');f.focus();f.setSelectionRange(0,0);})()",
  );
  await workspace.send("Input.insertText", { text: "Unsaved proof text. " });
  await workspace.eval(
    "(()=>{const f=document.querySelector('[data-c-field=text]');f.setSelectionRange(3,9);f.dispatchEvent(new Event('select',{bubbles:true}));})()",
  );
  // Only real input moves a saved reading position, so the proof turns the wheel.
  const box = await workspace.eval(
    "(()=>{const r=document.querySelector('.conversation-reader').getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2};})()",
  );
  await workspace.send("Input.dispatchMouseEvent", {
    type: "mouseWheel",
    x: box.x,
    y: box.y,
    deltaX: 0,
    deltaY: 120,
  });
  await wait(1000);
  const before = await workspace.eval(readDraft);
  assert.ok(
    before.text.startsWith("Unsaved proof text. "),
    "typed text landed",
  );
  assert.ok(before.reader > 0, "the reader has a position to keep");

  // A2/A3: bundle-only publication to adoption, one check, same processes.
  const pids = {
    service,
    worker: worker.pid,
    tray: trayPid(),
    real_service: realService,
    version,
  };
  const good = publish({ label: "Proof bundle A" }),
    published = Date.now();
  await check(menu);
  await hide(menu);
  menu.close();
  workspace.close();
  workspace = await until("Conversations on the bundle", () =>
    onBundle("workspace", good),
  );
  const adopted = Date.now();
  menu = await until("menu on the bundle", () => onBundle("menu", good));
  await until("files and draft restored", () =>
    workspace.eval(
      "!!document.querySelector('[data-c-field=text]')&&document.querySelector('[data-c-field=text]').value.startsWith('Unsaved proof text. ')",
    ),
  );
  await wait(1000);
  const after = await workspace.eval(readDraft);
  evidence.steps.adoption = {
    bundle: good,
    publication_to_conversations_ms: adopted - published,
    before: { ...before, text: before.text.slice(0, 40) },
    after: { ...after, text: after.text.slice(0, 40) },
    same_text: before.text === after.text,
  };
  assert.equal(after.text, before.text, "draft text");
  assert.deepEqual(after.caret, before.caret, "caret");
  assert.equal(after.selected, before.selected, "selected conversation");
  assert.equal(after.files, true, "attached file");
  assert.equal(after.reader, before.reader, "reading position");
  const requests = JSON.parse(
    fs.readFileSync(path.join(install, "tray-requests.json"), "utf8"),
  ).conversations["proof-selected"];
  evidence.steps.adoption.saved = {
    sent: requests.sent ?? null,
    reply: requests.reply ?? null,
  };
  assert.equal(requests.sent ?? null, null, "adoption sent nothing");
  assert.equal(requests.reply ?? null, null, "adoption replayed nothing");
  await until(
    "commit after clean start",
    () => store().current === good,
    30000,
  );
  const pidsAfter = {
    service: serviceRecord(install),
    worker: worker.exitCode === null ? worker.pid : null,
    tray: trayPid(),
    real_service: serviceRecord(
      path.join(process.env.LOCALAPPDATA || "", "taskq"),
    ),
    version: fs.readFileSync(path.join(install, "current"), "utf8"),
  };
  evidence.steps.processes = { before: pids, after: pidsAfter };
  assert.deepEqual(pidsAfter, pids);
  await workspace.send("Page.enable");
  await workspace.shot("adopted-conversations");
  await raise();
  await menu.shot("adopted-menu");
  await hide(menu);

  // A6: remote, file and second-document navigation, new windows and connections stay out of the bridged page.
  await workspace.eval("window.proofDocument=true");
  const fence = {};
  for (const [name, url] of [
    ["remote", "https://example.com/"],
    [
      "file",
      "file:///" + path.join(install, "tray-requests.json").replace(/\\/g, "/"),
    ],
    [
      "inline",
      'data:text/html,<script>window.ipc&&window.ipc.postMessage(\'{"a":"quit"}\')</script>',
    ],
  ]) {
    await workspace.eval("location.href=" + JSON.stringify(url));
    await wait(1500);
    fence[name] = {
      same_document: await workspace.eval("window.proofDocument===true"),
    };
    assert.equal(fence[name].same_document, true, name);
    const navigated = await workspace
      .send("Page.navigate", { url })
      .catch((e) => ({ errorText: e.message }));
    await wait(1500);
    fence[name].driver_navigate = navigated.errorText || "no error text";
    fence[name].same_document_after_driver = await workspace.eval(
      "window.proofDocument===true",
    );
    assert.equal(
      fence[name].same_document_after_driver,
      true,
      name + " driver",
    );
  }
  fence.window_open = await workspace.eval(
    "(()=>{const w=window.open('https://example.com/');return w===null||w.closed})()",
  );
  fence.fetch = await workspace.eval(
    "fetch('https://example.com/').then(()=>'connected',e=>'refused: '+e.message)",
  );
  assert.equal(fence.window_open, true);
  assert.match(fence.fetch, /^refused/);
  evidence.steps.fence = fence;
  assert.equal(
    fs.existsSync(path.join(install, "tray")),
    true,
    "an inline document could not quit the tray",
  );

  // A5: wrong checksum, wrong bridge and failed start each leave the good bundle running and say why.
  const shown = async (text) => {
    const value = await menu.eval("document.body.textContent");
    return value.includes(text);
  };
  const damaged = publish({ label: "Damaged", damage: true });
  await check(menu);
  await until("checksum refusal", () => store().bad?.[damaged], 60000);
  evidence.steps.wrong_checksum = {
    bundle: damaged,
    reason: store().bad[damaged],
    running: await bundleOf(menu),
  };
  assert.equal(evidence.steps.wrong_checksum.running, good);
  const bridged = publish({
    label: "Wrong bridge",
    bridge: embedded.bridge + 98,
  });
  await check(menu);
  await until("bridge refusal", () => store().bad?.[bridged], 60000);
  evidence.steps.wrong_bridge = {
    bundle: bridged,
    reason: store().bad[bridged],
    running: await bundleOf(menu),
  };
  assert.equal(evidence.steps.wrong_bridge.running, good);
  await raise();
  await until(
    "reason in the errors list",
    () => shown("UI bundle " + bridged + " refused"),
    30000,
  );
  evidence.steps.wrong_bridge.shown = await openErrors(menu);
  await menu.shot("refused-reason");
  await hide(menu);
  const failing = publish({
    label: "Failed start",
    tail: "<script>throw Error('proof bundle failed start')</script>",
  });
  await check(menu);
  await until("failed start retired", () => store().bad?.[failing], 120000);
  menu.close();
  workspace.close();
  menu = await until(
    "menu back on the good bundle",
    () => onBundle("menu", good),
    60000,
  );
  workspace = await until(
    "Conversations back on the good bundle",
    () => onBundle("workspace", good),
    60000,
  );
  evidence.steps.failed_start = {
    bundle: failing,
    reason: store().bad[failing],
    running: await bundleOf(menu),
    state: store(),
  };
  await raise();
  await until(
    "failed start reason shown",
    () => shown("UI bundle " + failing),
    30000,
  );
  evidence.steps.failed_start.shown = await openErrors(menu);
  await menu.shot("failed-start-reason");
  await workspace.shot("failed-start-conversations");
  assert.equal(
    (await workspace.eval(readDraft)).text,
    before.text,
    "draft survives the fallback too",
  );
  evidence.result = "pass";
}
if (process.argv[2] === "--store") {
  const target = path.resolve(process.argv[3]),
    bundle = epoch + 100 + "-" + "a".repeat(12);
  pages(path.join(target, "ui", bundle), bundle, {
    label: "Adopted bundle",
    tail: "<script>setTimeout(()=>{location.href='https://example.com/';},50);setTimeout(()=>{document.getElementById('ui-proof-banner').textContent='Adopted bundle, remote navigation refused';},300);</script>",
  });
  fs.writeFileSync(
    path.join(target, "ui", "state.json"),
    JSON.stringify({ current: bundle, previous: null, bad: {} }),
  );
  fs.writeFileSync(path.join(target, "current"), version);
  fs.rmSync(base, { recursive: true, force: true });
  console.log(bundle);
  process.exit(0);
}
main()
  .catch((e) => {
    evidence.result = "fail";
    evidence.error = String(e.stack || e);
    process.exitCode = 1;
  })
  .finally(async () => {
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(
      path.join(out, "evidence.json"),
      JSON.stringify(evidence, null, 1),
    );
    const pid = (() => {
        try {
          return trayPid();
        } catch {
          return null;
        }
      })(),
      service = serviceRecord(install);
    for (const p of [pid, service, ...children.map((c) => c.pid)])
      if (p)
        spawnSync("taskkill", ["/PID", String(p), "/T", "/F"], {
          stdio: "ignore",
        });
    await wait(1500);
    console.log(
      evidence.result === "pass" ? "PASS see " + out : "FAIL " + evidence.error,
    );
    try {
      fs.rmSync(base, { recursive: true, force: true });
    } catch {}
  });
