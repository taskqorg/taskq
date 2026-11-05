"use strict";
// The grammar is shared by source emitters, saved navigation and hydration.
const { execution } = require("./ui-runtime.js");
module.exports = execution.module("reading-anchor", {
  key: function (kind, ...parts) {
    return [kind, ...parts].join(":");
  },
  valid: function (value) {
    return (
      typeof value === "string" &&
      value.length <= 2048 &&
      /^(?:(?:task|brief|question|answer|result|event):[0-9]+(?::[0-9]+)?|owner-request:[a-z0-9-]+|report:[a-z0-9:-]+)$/.test(
        value,
      )
    );
  },
  native: function (item) {
    return (
      (item.attempt ? "attempt:" + item.attempt + ":" : "") +
      item.at +
      ":" +
      item.i
    );
  },
  boardTarget: function (value) {
    const key =
        /^(?:report:)?(question|answer|event):[0-9]+:([0-9]+)(?::[a-z0-9:]+)?$/.exec(
          value || "",
        ),
      legacy = /^report:question-[0-9]+-([0-9]+)-[0-9]+$/.exec(value || "");
    return key
      ? {
          kind: key[1] === "question" ? "question" : "event",
          id: Number(key[2]),
        }
      : legacy
        ? { kind: "question", id: Number(legacy[1]) }
        : null;
  },
  position: function (v) {
    return (
      !!v &&
      typeof v === "object" &&
      Number.isFinite(v.y) &&
      (module.exports.valid(v.key) ||
        (Number.isSafeInteger(v.at) &&
          v.at >= 0 &&
          Number.isSafeInteger(v.i) &&
          v.i >= 0 &&
          (v.part === undefined ||
            (Number.isSafeInteger(v.part) &&
              v.part >= 0 &&
              v.part <= 16 * 1024 * 1024))))
    );
  },
  mark: function (viewport, nodes, identity) {
    const box = viewport.getBoundingClientRect(),
      top = box.top,
      selection = viewport.ownerDocument?.getSelection();
    let first = null,
      last = null;
    const selected =
      selection &&
      !selection.isCollapsed &&
      [...nodes].find((node) => {
        const r = node.getBoundingClientRect();
        return (
          node.checkVisibility() &&
          r.bottom > top &&
          r.top < box.bottom &&
          (selection.containsNode(node, true) ||
            node.contains(selection.anchorNode) ||
            node.contains(selection.focusNode))
        );
      });
    if (selected)
      return {
        ...identity(selected),
        y: selected.getBoundingClientRect().top - top,
      };
    for (const node of nodes) {
      const r = node.getBoundingClientRect();
      if (!node.checkVisibility() || r.height <= 0 || r.bottom <= top) continue;
      if (!first) first = node;
      if (r.top <= top) last = node;
      else if (last) break;
    }
    const node = last || first;
    return node
      ? { ...identity(node), y: node.getBoundingClientRect().top - top }
      : null;
  },
  hold: function (viewport, anchor, node, stick = false) {
    const delta = stick
      ? viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop
      : node
        ? node.getBoundingClientRect().top -
          viewport.getBoundingClientRect().top -
          anchor.y
        : 0;
    if (Math.abs(delta) > 0.5) viewport.scrollTop += delta;
    return stick || !!node;
  },
});
