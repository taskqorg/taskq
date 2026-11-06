// UI execution uses the existing native capture writer; no collector or storage here.
"use strict";
const core = require("@taskq/coreq");
const shared = core.shared(),
  events = new WeakMap();
// Register a method when used: loading several screens must not exhaust CoreQ's bounded metric registry.
// Calls retain their original operation name, receiver, execution context and result/error capture.
const execution = {
  ...shared,
  module(name, methods) {
    const wrapped = {};
    for (const [key, descriptor] of Object.entries(
      Object.getOwnPropertyDescriptors(methods),
    )) {
      let method;
      Object.defineProperty(wrapped, key, {
        ...descriptor,
        value: function (...args) {
          // A page past the telemetry method bound keeps working unmeasured: telemetry never takes a page down.
          if (!method) {
            try {
              method = shared.module(name, { [key]: descriptor.value })[key];
            } catch (error) {
              if (error.message !== "telemetry method limit") throw error;
              method = descriptor.value;
            }
          }
          return Reflect.apply(method, this, args);
        },
      });
    }
    return Object.freeze(wrapped);
  },
};
const frequent = new Set([
  "scroll",
  "pointermove",
  "input",
  "wheel",
  "touchmove",
  "load",
]);
function send(message) {
  const context = execution.context();
  globalThis.ipc?.postMessage(
    JSON.stringify({
      ...message,
      ...(context
        ? { context: "1 " + context.trace + " " + context.span }
        : {}),
    }),
  );
}
// A measured fact with a closed name: `prefix.<one of each list>`, inventoried by scripts/check-web.cjs.
// `taskq profile actions` reads the row like any action; it carries its own elapsed time and no user text.
let measuredRun = null,
  measuredSerial = 0;
function measured(prefix, ...lists) {
  return function (
    parts,
    { duration_us = 0, outcome = "ok", error = "none" } = {},
  ) {
    if (
      !globalThis.ipc ||
      parts.length !== lists.length ||
      parts.some((part, i) => !lists[i].includes(part))
    )
      return false;
    measuredRun ??= [...crypto.getRandomValues(new Uint8Array(16))]
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    const context = execution.context(),
      span = measuredRun + "." + ++measuredSerial;
    globalThis.ipc.postMessage(
      JSON.stringify({
        a: "web-observation",
        row: {
          kind: "ui",
          operation: "js." + [prefix, ...parts].join("."),
          trace: context?.trace || span,
          span,
          parent: context?.span || null,
          started_ms: Date.now(),
          duration_us: Math.max(0, Math.round(duration_us)),
          outcome,
          error,
        },
      }),
    );
    return true;
  };
}
function connect() {
  core.connect((row) => {
    if (!globalThis.ipc) return false;
    globalThis.ipc.postMessage(JSON.stringify({ a: "web-observation", row }));
    return true;
  });
}
function listen(target, type, callback, options) {
  const method = execution.module("ui.events", { [type]: callback })[type];
  const handler = execution.action("ui." + type, function (event) {
    if (event && typeof event === "object")
      events.set(
        event,
        execution.bind((fn) => fn()),
      );
    return Reflect.apply(method, this, [event]);
  });
  const receive = function (event) {
    const inherited = event && events.get(event);
    return inherited
      ? inherited(() => Reflect.apply(method, this, [event]))
      : frequent.has(type)
        ? Reflect.apply(method, this, [event])
        : Reflect.apply(handler, this, [event]);
  };
  target.addEventListener(type, receive, options);
  return () => target.removeEventListener(type, receive, options);
}
function defer(callback, delay = 0) {
  return setTimeout(
    execution.bind(execution.action("ui.timer", callback)),
    delay,
  );
}
// #1548: a steady repaint; its module callback counts in the 2 s summary instead of two action rows per run.
function repeat(callback, delay) {
  return setTimeout(callback, delay);
}
function cancel(timer) {
  clearTimeout(timer);
}
function effect(name, start, receive) {
  execution.effect(
    name,
    () => Promise.resolve().then(execution.bind(start)),
    (result) =>
      receive(
        result.ok
          ? result.value
          : {
              state: "unknown",
              reason: result.unknown
                ? "Completion unknown"
                : String(result.value?.message || result.value),
            },
      ),
  );
}
module.exports = {
  execution,
  connect,
  send,
  listen,
  defer,
  repeat,
  cancel,
  effect,
  measured,
};
