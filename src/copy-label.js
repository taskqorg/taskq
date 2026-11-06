"use strict";
const { h } = require("preact"),
  ui = require("./ui-runtime.js"),
  assets = require("./ui-icons.json");
const { execution } = ui;
const api = execution.module("copyLabel", {
  escape: function (value) {
    return String(value ?? "").replace(
      /[&<>"']/g,
      (c) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[c],
    );
  },
  task: function (project, n) {
    return (project || "Board") + "#" + n;
  },
  // A local dependency arrives as a bare #N; another repository's already names its project.
  ref: function (project, ref) {
    return String(ref).startsWith("#") ? (project || "Board") + ref : ref;
  },
  html: function (text) {
    const value = api.escape(text);
    return (
      '<span class="copy-label"><span class="copy-value">' +
      value +
      '</span><button type="button" class="copy-label-button" data-copy-label="' +
      value +
      '" aria-label="Copy ' +
      value +
      '" title="Copy ' +
      value +
      '">' +
      assets.copy +
      '</button><span class="sr-only" role="status" aria-live="polite"></span></span>'
    );
  },
  node: function (text) {
    return h("span", { dangerouslySetInnerHTML: { __html: api.html(text) } });
  },
  mount: function (menu) {
    ui.listen(
      menu,
      "click",
      (e) => {
        const b = e.target.closest("[data-copy-label]");
        if (!b) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        const doc = b.ownerDocument,
          active = doc.activeElement,
          selection = doc.getSelection(),
          ranges = selection
            ? Array.from({ length: selection.rangeCount }, (_, i) =>
                selection.getRangeAt(i).cloneRange(),
              )
            : [],
          caret =
            active?.selectionStart == null
              ? null
              : [active.selectionStart, active.selectionEnd],
          field = doc.createElement("textarea");
        field.value = b.dataset.copyLabel;
        field.style.cssText = "position:fixed;left:-10000px;top:0";
        doc.body.append(field);
        let ok = false;
        try {
          field.select();
          ok = doc.execCommand("copy");
        } catch {
        } finally {
          field.remove();
          active?.focus({ preventScroll: true });
          if (caret) active.setSelectionRange(...caret);
          else if (selection) {
            selection.removeAllRanges();
            for (const range of ranges) selection.addRange(range);
          }
        }
        const message = ok ? "Copied" : "Copy unavailable";
        b.title = message;
        b.innerHTML = ok ? assets.check : assets["circle-x"];
        b.nextElementSibling.textContent = message;
      },
      true,
    );
    ui.listen(
      menu,
      "keydown",
      (e) => {
        if (e.target.closest("[data-copy-label]")) {
          e.stopImmediatePropagation();
          return;
        }
        const row = e.target.closest(".task-target[role=button]");
        if (row === e.target && ["Enter", " "].includes(e.key)) {
          e.preventDefault();
          row.click();
        }
      },
      true,
    );
  },
});
module.exports = api;
