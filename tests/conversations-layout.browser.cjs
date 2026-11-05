// #2366 A6/A8: divider/navigation in the released bundle; no native/board effects.
const fs = require("node:fs"),
  assert = require("node:assert/strict"),
  { chromium } = require("playwright");
(async () => {
  const browser = await chromium.launch(require("./browser.cjs")());
  try {
    const page = await browser.newPage({
        viewport: { width: 1100, height: 740 },
      }),
      errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    const html = fs.readFileSync("build/web/workspace.html", "utf8");
    const fixture = {
      font: 16,
      theme: "dark",
      host: "invented",
      projects: [
        {
          root: "fixture",
          name: "Invented",
          issues: "https://example.org/issues",
          alive: true,
          complete: true,
          tasks: [],
        },
      ],
      ai: { runtime: { id: "codex", available: false } },
      messages: { complete: true, entries: [] },
      conversations: {
        complete: true,
        items: [
          {
            id: "invented-draft",
            revision: 1,
            title: "Independent draft",
            root: "fixture",
            runtime: "codex",
            text: "Saved text and selection",
            caret: [2, 6],
            scroll: 0,
            files: [],
            archived: false,
          },
        ],
        navigation: { selected: "invented-draft", list: "recent" },
      },
    };
    const boot = async (navigation) => {
      await page.setContent(html);
      await page.evaluate(
        ({ fixture, navigation }) => {
          window.sent = [];
          window.ipc = { postMessage: (text) => sent.push(JSON.parse(text)) };
          if (navigation) fixture.conversations.navigation = navigation;
          taskq.receive(fixture);
        },
        { fixture, navigation },
      );
    };
    await boot();
    const divider = page.getByRole("separator", { name: "Sidebar width" }),
      input = page.locator("[data-c-field=text]");
    await input.fill("Unsent long draft\n".repeat(40));
    await input.evaluate((e) => e.setSelectionRange(3, 17));
    const before = await page.locator(".conversation-list").boundingBox();
    await divider.focus();
    await page.keyboard.press("ArrowRight");
    assert.equal(
      Math.round(
        (await page.locator(".conversation-list").boundingBox()).width -
          before.width,
      ),
      10,
      "keyboard changes the preferred width",
    );
    const box = await divider.boundingBox();
    await page.mouse.move(box.x + 3, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x + 53, box.y + 100);
    await page.mouse.up();
    const preferred = await page
      .locator(".conversation-list")
      .evaluate((e) => e.getBoundingClientRect().width);
    assert(preferred > before.width + 40, "pointer divider changes width");
    await page.setViewportSize({ width: 1000, height: 740 });
    assert(
      Math.abs(
        (await page.locator(".conversation-list").boundingBox()).width -
          preferred,
      ) <= 1,
      "ordinary resize changes only right pane",
    );
    await page
      .getByRole("button", { name: "Hide sidebar", exact: true })
      .click();
    assert.equal(await page.locator(".conversation-list").isVisible(), false);
    await page
      .getByRole("button", { name: "Show sidebar", exact: true })
      .click();
    for (const font of [14, 16, 18, 20])
      for (const width of [350, 620, 940]) {
        await page.setViewportSize({ width, height: 740 });
        await page.evaluate((font) => taskq.receive({ font }), font);
        if (width <= 620) {
          assert.equal(await divider.isVisible(), false);
          await page.getByRole("button", { name: "List", exact: true }).click();
          assert(await page.locator(".conversation-list").isVisible());
          await page.getByRole("button", { name: "Back", exact: true }).click();
        }
        assert.equal(
          await input.inputValue(),
          "Unsent long draft\n".repeat(40),
        );
        assert.equal(
          await page.evaluate(
            () => document.documentElement.scrollWidth > innerWidth,
          ),
          false,
          "no horizontal overflow",
        );
      }
    await page.setViewportSize({ width: 1100, height: 740 });
    assert(
      Math.abs(
        (await page.locator(".conversation-list").boundingBox()).width -
          preferred,
      ) <= 1,
      "narrow clamp never overwrites preference",
    );
    await page
      .getByRole("button", { name: "Hide sidebar", exact: true })
      .click();
    await page.waitForFunction(() =>
      sent.some(
        (m) => m.a === "conversation-navigation" && m.navigation?.pane?.hidden,
      ),
    );
    const navigation = await page.evaluate(
      () =>
        sent.filter((m) => m.a === "conversation-navigation").at(-1).navigation,
    );
    assert.equal(navigation.pane.hidden, true);
    assert(Math.abs(navigation.pane.width - preferred) <= 1);
    await boot(navigation);
    assert.equal(await page.locator(".conversation-list").isVisible(), false);
    await page
      .getByRole("button", { name: "Show sidebar", exact: true })
      .click();
    assert(
      Math.abs(
        (await page.locator(".conversation-list").boundingBox()).width -
          preferred,
      ) <= 1,
    );
    assert.equal(
      await page.evaluate(() =>
        sent.some((m) => ["send", "reply"].includes(m.action)),
      ),
      false,
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS saved keyboard/pointer divider, wide hide/show, narrow preference preservation, drafts, supported fonts and reload",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
