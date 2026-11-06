"use strict";
// The grammar is shared by source emitters, saved navigation and hydration.
// This module is the only owner of scroll offsets (scripts/check-web.cjs). A position is row keys with their offsets
// from the viewport top, never a scroll value; docs/design/ai-workspace.md § Scroll position states the bounds.
const ui = require("./ui-runtime.js"),
  { execution } = ui;
const KEYED = "[data-reading-key],[data-row-key],[data-key]",
  ROWS = 8,
  DRIFT = 2,
  QUIET = 150,
  PRESS = 5000,
  REPORT = 5000;
const regions = new WeakMap(),
  written = new WeakMap(),
  reported = new Map(),
  waiting = new Map(),
  watched = new WeakSet();
const gate = { moving: false, pressed: null, last: 0, timer: null };
const drift = ui.measured(
  "scroll.drift",
  [
    "tasks",
    "history",
    "conversations",
    "bounded",
    "answer-card",
    "finding-composer",
    "workspace",
  ],
  ["insert-above", "append", "removal", "late-image", "resize", "refresh"],
);
// A page may register 256 telemetry methods; rows and gate ticks are hot, so only entry points count against that bound.
const A = execution
  .module("reading-anchor", {
    methods: function () {
      return {
        key: function (kind, ...parts) {
          return [kind, ...parts].join(":");
        },
        valid: function (value) {
          return (
            typeof value === "string" &&
            value.length <= 2048 &&
            /^(?:(?:task|brief|question|answer|result|event):[0-9]+(?::[0-9]+)?|attempt:[0-9]+|owner-request:[a-z0-9-]+|report:[a-z0-9:-]+)$/.test(
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
            legacy = /^report:question-[0-9]+-([0-9]+)-[0-9]+$/.exec(
              value || "",
            );
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
            (A.valid(v.key) ||
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
            if (!node.checkVisibility() || r.height <= 0 || r.bottom <= top)
              continue;
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
          if (Math.abs(delta) > 0.5) A.by(viewport, delta);
          return stick || !!node;
        },
        write: function (viewport, value) {
          viewport.scrollTop = value;
          written.set(viewport, viewport.scrollTop);
        },
        // The region still sits where this owner last put it, so its scroll event reports no navigation.
        wrote: function (viewport) {
          return Math.abs(viewport.scrollTop - written.get(viewport)) <= 1;
        },
        by: function (viewport, delta) {
          A.write(viewport, viewport.scrollTop + delta);
        },
        top: function (viewport) {
          A.write(viewport, 0);
        },
        bottom: function (viewport) {
          A.write(viewport, viewport.scrollHeight);
        },
        reveal: function (viewport, node, offset = 0) {
          A.by(
            viewport,
            node.getBoundingClientRect().top -
              viewport.getBoundingClientRect().top -
              offset,
          );
        },
        scrolled: function (viewport) {
          return viewport.scrollTop > 0;
        },
        atBottom: function (viewport, within = 24) {
          return (
            viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <
            within
          );
        },
        rowKey: function (node) {
          const d = node.dataset;
          return d?.readingKey || d?.rowKey || d?.key || null;
        },
        find: function (viewport, key) {
          if (key[0] === "#") {
            let node = viewport;
            for (const i of key.slice(1).split("."))
              node = node?.children?.[Number(i)];
            return node || null;
          }
          const k = CSS.escape(key);
          return viewport.querySelector(
            '[data-reading-key="' +
              k +
              '"],[data-row-key="' +
              k +
              '"],[data-key="' +
              k +
              '"]',
          );
        },
        // Content without keyed rows (plain text, a form) keys its visible children by structural path: there is no pixel fallback.
        // A child that straddles the viewport top holds its own children first, then the rows after it at each outer level.
        structural: function (viewport, box) {
          const levels = [],
            rows = [];
          for (let parent = viewport, prefix = "#"; ;) {
            const kids = [...(parent.children || [])],
              i = kids.findIndex((k) => {
                const r = k.getBoundingClientRect();
                return r.height > 0 && r.bottom > box.top;
              });
            if (i < 0) break;
            if (
              kids[i].getBoundingClientRect().top < box.top &&
              kids[i].children.length
            ) {
              levels.push({ kids, prefix, at: i + 1 });
              parent = kids[i];
              prefix += i + ".";
              continue;
            }
            levels.push({ kids, prefix, at: i });
            break;
          }
          while (levels.length && rows.length < ROWS) {
            const { kids, prefix, at } = levels.pop();
            for (let n = at; n < kids.length && rows.length < ROWS; n++) {
              const r = kids[n].getBoundingClientRect();
              if (r.top >= box.bottom) break;
              if (r.height > 0)
                rows.push({ key: prefix + n, y: Math.round(r.top - box.top) });
            }
          }
          return rows;
        },
        // A keyed control inside a keyed row belongs to that row: only the outermost node of a key is a row.
        snapshot: function (viewport) {
          if (!viewport?.getBoundingClientRect) return { rows: [], width: 0 };
          const box = viewport.getBoundingClientRect(),
            rows = [];
          let outer = null;
          for (const node of viewport.querySelectorAll(KEYED)) {
            if (outer?.contains(node)) continue;
            outer = node;
            const r = node.getBoundingClientRect();
            if (r.height <= 0 || r.bottom <= box.top) continue;
            if (r.top >= box.bottom) break;
            rows.push({ key: A.rowKey(node), y: Math.round(r.top - box.top) });
            if (rows.length === ROWS) break;
          }
          return {
            rows: rows.length ? rows : A.structural(viewport, box),
            width: viewport.clientWidth,
            height: viewport.scrollHeight,
          };
        },
        // A window resize event can draw before the observer corrects, when new line wraps have already moved the rows:
        // the reader's place is still the one from the last width.
        held: function (viewport) {
          const position = regions.get(viewport)?.position;
          return position && position.width !== viewport?.clientWidth
            ? position
            : A.snapshot(viewport);
        },
        // A saved number is not a position: it reads as none.
        saved: function (value) {
          return (
            !!value &&
            Array.isArray(value.rows) &&
            value.rows.length <= ROWS &&
            value.rows.every(
              (r) =>
                typeof r?.key === "string" &&
                r.key.length > 0 &&
                r.key.length <= 256 &&
                Number.isFinite(r.y),
            )
          );
        },
        // Stores are byte-bounded, so only the leading rows persist.
        compact: function (position, rows = 3) {
          return A.saved(position)
            ? { rows: position.rows.slice(0, rows) }
            : null;
        },
        // Runs before paint. It holds the first surviving row at its offset; no survivor opens the region at its start.
        restore: function (viewport, position, surface, cause) {
          if (
            !A.saved(position) ||
            !viewport?.getBoundingClientRect ||
            !position.rows.length
          ) {
            if (viewport) A.top(viewport);
            return false;
          }
          const row = A.survivor(viewport, position);
          if (row < 0) {
            A.top(viewport);
            return false;
          }
          const held = position.rows[row],
            delta =
              A.find(viewport, held.key).getBoundingClientRect().top -
              viewport.getBoundingClientRect().top -
              held.y;
          if (Math.abs(delta) > 0.5) A.by(viewport, delta);
          if (surface)
            A.measure(
              viewport,
              position,
              surface,
              cause ||
                (row > 0 || delta < -0.5
                  ? "removal"
                  : delta > 0.5
                    ? "insert-above"
                    : A.added(viewport, position)
                      ? "append"
                      : "refresh"),
            );
          return true;
        },
        // A row new to the view, in a region that grew, is an append; a row that only grew is a refresh.
        added: function (viewport, position) {
          return (
            viewport.scrollHeight > (position.height ?? Infinity) &&
            A.snapshot(viewport).rows.some(
              (r) => !position.rows.some((p) => p.key === r.key),
            )
          );
        },
        survivor: function (viewport, position) {
          return position.rows.findIndex(
            (r) => A.find(viewport, r.key)?.getBoundingClientRect().height > 0,
          );
        },
        // A reader that holds its own anchor (a board or native record) still answers for the rows it showed.
        measure: function (viewport, position, surface, cause = "refresh") {
          const row = A.survivor(viewport, position);
          if (row >= 0 && A.shift(viewport, position, row) > DRIFT)
            A.drifted(surface, cause);
        },
        // The held row and the contiguous rows after it. A width change rewraps text, so it measures the held row only.
        shift: function (viewport, position, row = 0) {
          const top = viewport.getBoundingClientRect().top,
            rows =
              position.width !== undefined &&
              position.width !== viewport.clientWidth
                ? position.rows.slice(row, row + 1)
                : position.rows.slice(row);
          let worst = 0;
          for (const r of rows) {
            const node = A.find(viewport, r.key);
            if (!node) break;
            worst = Math.max(
              worst,
              Math.abs(node.getBoundingClientRect().top - top - r.y),
            );
          }
          return worst;
        },
        drifted: function (surface, cause) {
          const key = surface + "." + cause,
            now = performance.now();
          if (reported.has(key) && now - reported.get(key) < REPORT) return;
          reported.set(key, now);
          drift([surface, cause], { outcome: "failed", error: "scroll-drift" });
        },
        // A region keeps its position through layout that no render caused: a resize or a late image.
        track: function (viewport, surface) {
          if (!viewport?.getBoundingClientRect) return;
          let region = regions.get(viewport);
          if (!region) {
            region = { surface, position: null, image: false, observer: null };
            regions.set(viewport, region);
            if (globalThis.ResizeObserver)
              region.observer = new ResizeObserver(() => {
                if (gate.moving || !region.position) return;
                const cause = region.image ? "late-image" : "resize";
                region.image = false;
                if (
                  region.position.width !== viewport.clientWidth ||
                  A.shift(viewport, region.position) > 0.5
                )
                  A.restore(viewport, region.position, region.surface, cause);
                region.position = A.snapshot(viewport);
              });
          }
          region.surface = surface;
          region.position = A.snapshot(viewport);
          region.observer?.observe(viewport);
          for (const child of viewport.children)
            region.observer?.observe(child);
        },
        // The gate. Wheel, touch, scrollbar drag and inertia keep regions busy until QUIET ms pass with no input and no press.
        // Chromium sends no pointerup after a scrollbar press, so a press stops holding after PRESS ms.
        watch: function (target) {
          if (watched.has(target)) return;
          watched.add(target);
          const options = { capture: true, passive: true };
          ui.listen(
            target,
            "scroll",
            (e) => {
              const el =
                e.target?.nodeType === 9 ? e.target.scrollingElement : e.target;
              // A scroll event alone may be layout or this module: only input in progress or its inertia keeps the gate.
              if (gate.moving || A.pressed()) A.moved();
              const region = regions.get(el);
              if (region) region.position = A.snapshot(el);
            },
            options,
          );
          for (const type of ["wheel", "touchmove"])
            ui.listen(target, type, () => A.moved(), options);
          for (const type of ["touchstart", "pointerdown"])
            ui.listen(
              target,
              type,
              () => {
                gate.pressed = performance.now();
              },
              options,
            );
          for (const type of [
            "touchend",
            "touchcancel",
            "pointerup",
            "pointercancel",
          ])
            ui.listen(
              target,
              type,
              () => {
                gate.pressed = null;
                if (gate.moving) A.moved();
              },
              options,
            );
          // A deliberate action is navigation, which does not wait.
          for (const type of ["click", "keydown"])
            ui.listen(
              target,
              type,
              () => {
                gate.moving = false;
              },
              options,
            );
          ui.listen(
            target,
            "load",
            (e) => {
              if (e.target?.tagName !== "IMG") return;
              for (let n = e.target.parentElement; n; n = n.parentElement) {
                const region = regions.get(n);
                if (region) region.image = true;
              }
            },
            options,
          );
        },
        pressed: function () {
          return (
            gate.pressed !== null && performance.now() - gate.pressed < PRESS
          );
        },
        moved: function () {
          gate.moving = true;
          gate.last = performance.now();
          if (gate.timer === null) gate.timer = ui.defer(A.quiet, QUIET);
        },
        quiet: function () {
          gate.timer = null;
          const left = QUIET - (performance.now() - gate.last);
          if (gate.moving && (A.pressed() || left > 1)) {
            gate.timer = ui.defer(A.quiet, Math.max(left, 16));
            return;
          }
          gate.moving = false;
          const run = [...waiting.values()];
          waiting.clear();
          for (const fn of run) fn();
        },
        busy: function () {
          return gate.moving;
        },
        // One waiting apply per owner: the newest data wins.
        after: function (owner, fn) {
          waiting.set(owner, fn);
        },
        // Focus returns to the same control in the row with the same key, so a reorder keeps focus.
        focusMark: function (
          root,
          active = root?.ownerDocument?.activeElement,
        ) {
          if (
            !active ||
            !root?.contains?.(active) ||
            active === root.ownerDocument?.body
          )
            return null;
          const row = active.closest?.(KEYED),
            data = JSON.stringify(active.dataset),
            scope = row || root;
          if (
            !row &&
            data === "{}" &&
            !["H1", "SUMMARY"].includes(active.tagName)
          )
            return null;
          return {
            row: row ? A.rowKey(row) : null,
            tag: active.tagName,
            data,
            index: A.controls(scope, active.tagName, data).indexOf(active),
          };
        },
        controls: function (scope, tag, data) {
          return [
            ...(scope.tagName === tag ? [scope] : []),
            ...scope.querySelectorAll(tag),
          ].filter((node) => JSON.stringify(node.dataset) === data);
        },
        focusTarget: function (root, mark) {
          if (!mark) return null;
          const row = mark.row ? A.find(root, mark.row) : root;
          return row
            ? A.controls(row, mark.tag, mark.data)[mark.index] || null
            : null;
        },
        // Bounded blocks show whole lines: a cut first line scrolls in, a cut last line shrinks the block.
        wholeLines: function (c) {
          if (c.scrollHeight <= c.clientHeight) return;
          const top = c.getBoundingClientRect().top + c.clientTop;
          const lines = () => {
            const r = document.createRange(),
              w = document.createTreeWalker(c, NodeFilter.SHOW_TEXT),
              a = [];
            for (let n; (n = w.nextNode());) {
              r.selectNodeContents(n);
              a.push(...r.getClientRects());
            }
            return a;
          };
          const shrink = (d) => (c.style.maxHeight = c.offsetHeight - d + "px");
          const cut = (edge) =>
            Math.min(
              ...lines()
                .filter((l) => l.top < edge && l.bottom > edge)
                .map((l) => l.top),
            );
          const head = cut(top);
          if (head < top && c.scrollTop + c.clientHeight < c.scrollHeight - 1)
            A.by(c, head - top);
          else if (head < top) {
            const d =
              Math.min(
                ...lines()
                  .filter((l) => l.top >= top)
                  .map((l) => l.top),
              ) - top;
            if (d < c.clientHeight) {
              shrink(d);
              A.by(c, d);
            }
          }
          const bottom = top + c.clientHeight,
            tail = cut(bottom);
          if (tail > top && tail < bottom) shrink(bottom - tail);
        },
      };
    },
  })
  .methods();
module.exports = {
  ...A,
  ...execution.module("reading-anchor", {
    key: A.key,
    valid: A.valid,
    native: A.native,
    boardTarget: A.boardTarget,
    position: A.position,
    mark: A.mark,
    hold: A.hold,
    restore: A.restore,
    snapshot: A.snapshot,
    track: A.track,
  }),
};
