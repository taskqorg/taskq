'use strict';
const ui=require('./ui-runtime.js');
const execution=ui.execution;

const {fresh,textProblem,status,step,render,compose,receipt,mount,replay}=execution.module("answer-card",{
fresh:function(row) { return {draft: '', bound: row.revision, op: null, receipt: null}; },
textProblem:function(q, text) {
    const lines = text.split('\n').filter(l => l.trim());
    if (!lines.length) return 'Type an answer';
    if (lines.length > q.limits.lines) return 'At most ' + q.limits.lines + ' lines';
    if (lines.some(l => [...l].length > q.limits.chars)) return 'At most ' + q.limits.chars + ' characters per line';
    return null;
  },
status:function(row, local, cap) {
    const q = row.question;
    // a receipt belongs to the question it answered; one in flight or unknown holds across refreshes, an answered one
    // stays beside its task after the question is gone
    if (local.op && local.receipt && (HELD.includes(local.receipt.state) || local.op.revision === row.revision || !q)) return local.receipt.state;
    if (!manageable(row, cap) && (!q || !q.parsed || !cap || !cap.answer || !['ask', 'review'].includes(row.state))) return 'read-only';
    if (local.bound !== row.revision && (local.draft.trim() || local.op || local.confirmClose)) return 'stale';
    return 'actionable';
  },
step:function(row, local, cap, ev, newId) {
    const st = status(row, local, cap), l = {...local};
    const {start}=execution.module("answer-card.step",{start:payload => {
      l.confirmClose = false;
      l.op = {id: newId(), task: row.n, revision: row.revision, ...payload};
      // #1662 A4: the receipt names the question and the answer it carries, kept apart from the immutable op
      l.about = about(row, payload);
      l.bound = row.revision;
      l.receipt = {state: 'sending'};
      return {local: l, send: l.op};
    }});
    switch (ev.type) {
      case 'input':
        // a draft typed while stale stays bound to the question it was written for
        if (st !== 'stale') l.bound = row.revision;
        l.draft = ev.text;
        return {local: l, send: null};
      case 'choose': {
        if (!LIVE.includes(st)) return {local, send: null};
        const c = row.question.choices.find(x => x.id === ev.id);
        if (!c || c.consequential) return {local, send: null};
        return start({choice: c.id});
      }
      case 'submit':
        if (!cap || !cap.answer || !LIVE.includes(st) || textProblem(draftQuestion(row, cap), l.draft)) return {local, send: null};
        // an unparsed fallback card takes a requeue reason; a parsed owner action without options takes a text answer
        return start(fallback(row, cap) ? {action: 'requeue', text: l.draft.trim()} : {text: l.draft.trim()});
      case 'requeue':
        if (!manageable(row, cap) || !LIVE.includes(st) || textProblem(REQUEUE, l.draft)) return {local, send: null};
        return start({action: 'requeue', text: l.draft.trim()});
      case 'later':
      case 'close':
      case 'confirm-close':
        if (!manageable(row, cap) || !LIVE.includes(st)) return {local, send: null};
        if (ev.type === 'close') return {local: {...l, bound: row.revision, confirmClose: true}, send: null};
        if (ev.type === 'confirm-close' && !l.confirmClose) return {local, send: null};
        return start(ev.type === 'later' ? {action: 'later'} : {action: 'close', confirmed: true});
      case 'cancel-close':
        return {local: {...l, confirmClose: false}, send: null};
      case 'review':
        if (st === 'stale') { l.bound = row.revision; l.op = null; l.receipt = null; l.confirmClose = false; }
        return {local: l, send: null};
      case 'receipt':
        if (!l.op || ev.opId !== l.op.id) return {local, send: null};
        l.receipt = {state: ['accepted', 'rejected', 'stale', 'unknown', 'failed', 'partial'].includes(ev.state) ? ev.state : 'unknown', reason: ev.reason, next: ev.next};
        if (ev.state === 'accepted' && l.op.text) l.draft = '';
        return {local: l, send: null};
      case 'retry':
        // uncertain, unsent or partial delivery reconciles with the same id and payload, never a new one
        return RETRY.includes(st) ? {local: {...l, receipt: {state: 'sending'}}, send: l.op} : {local, send: null};
      default:
        return {local, send: null};
    }
  },
render:function(board, row, local, cap) {
    const q = row.question || {text: row.card || '', parsed: false, choices: [], evidence: []}, st = status(row, local, cap);
    const id = 'qac-' + String(board).replace(/[^\w-]/g, '_') + '-' + row.n, live = LIVE.includes(st), own = !(cap && cap.composer);
    const what = local.op ? '#' + row.n + ' ' + (local.about || about(row, local.op)) : '';
    let h = '<section class="qac" data-qac-state="' + st + '" aria-labelledby="' + id + '-t">';
    h += '<p class="qac-head" id="' + id + '-t"><span class="qac-mark" aria-hidden="true">?</span> <a href="' + href(row.url) + '" target="_blank" rel="noopener">#' + esc(row.n) + '</a> ' + esc(row.title) + '</p>';
    if (manageable(row, cap)) {
      h += q.parsed ? lines(q) + images(q) : '<pre class="qac-text">' + esc(q.text || row.card || 'No question text supplied') + '</pre>';
      if (own) h += q.parsed ? '<form class="qac-text-form" data-qac="submit"><textarea data-qac="input" rows="2" aria-label="Your answer" placeholder="What you did"' + (live ? '' : ' readonly') + '>' + esc(local.draft) + '</textarea><button type="submit"' + (live && !textProblem(q, local.draft) ? '' : ' disabled') + '>Send</button>' + hint(id, live && textProblem(q, local.draft)) + '</form>'
        : '<form class="qac-text-form" data-qac="submit"><textarea data-qac="input" rows="1" aria-label="Requeue reason" placeholder="Reason to requeue"' + (live ? '' : ' readonly') + '>' + esc(local.draft) + '</textarea><button type="submit"' + (live && !textProblem(REQUEUE, local.draft) ? '' : ' disabled') + '>Requeue</button>' + hint(id, live && textProblem(REQUEUE, local.draft)) + '</form>';
      // an owner action answers with text above; its requeue reason is the same draft, sent as an action
      h += '<div class="qac-choices">' + (q.parsed && own ? '<button type="button" data-qac="requeue"' + (live && !textProblem(REQUEUE, local.draft) ? '' : ' disabled') + '>Requeue</button>' : '') + '<button type="button" data-qac="later"' + (live ? '' : ' disabled') + '>Later</button><button type="button" data-qac="close"' + (live ? '' : ' disabled') + '>Close…</button></div>';
      if (local.confirmClose && live) h += '<p>Cancel #' + esc(row.n) + ' without publication? Running work will stop. The task is parked before closing.</p><div class="qac-choices"><button type="button" data-qac="confirm-close">Close without publishing</button><button type="button" data-qac="cancel-close">Keep task</button></div>';
      h += receipt(st, local.receipt && local.receipt.reason, row.url, id, local.receipt && local.receipt.next, !!(local.op && local.op.action), what);
      if (st === 'stale') h += '<button type="button" data-qac="review">I read the new question</button>';
      h += retry(st, local.receipt && local.receipt.reason);
      return h + '</section>';
    }
    if (!q.parsed) return h + '<pre class="qac-text">' + esc(q.text) + '</pre>' + receipt(st, st === 'read-only' ? 'This card has no answer form here' : local.receipt && local.receipt.reason, row.url, id, null, false, what) +
      retry(st, local.receipt && local.receipt.reason) + '</section>';
    h += lines(q) + images(q);
    if (q.choices.length) h += '<div class="qac-choices" role="group" aria-label="Choices">';
    for (const c of q.choices) {
      const off = !live || c.consequential, why = c.consequential ? 'Answer on the board: this choice closes or publishes the task' : '';
      // #1787: the button shows N. and the label; the detail follows in full, never cut; a card without one keeps its whole text
      const name = c.label == null ? c.text : c.label, said = [c.detail ? id + '-o' + c.id : '', why ? id + '-c' + c.id : ''].filter(Boolean).join(' ');
      h += '<button type="button" data-qac="choose" data-choice="' + c.id + '"' + (off ? ' disabled' : '') + (said ? ' aria-describedby="' + said + '"' : '') + '><span class="qac-number">' + c.id + '.</span> <span>' + esc(label(name)) +
        (c.recommended ? ' <span class="qac-star" role="img" aria-label="Recommended">★</span>' : '') + '</span></button>';
      // § 2: a button cannot hold an anchor; a link in the label opens beside it, one in the detail inside the detail
      for (const m of String(name).matchAll(LINK)) h += '<a class="qac-link" href="' + href(m[2]) + '" target="_blank" rel="noopener">' + esc(m[1]) + ' ↗</a>';
      if (c.detail) h += '<span class="qac-detail" id="' + id + '-o' + c.id + '">' + linked(c.detail) + '</span>';
      if (why) h += '<span class="qac-why" id="' + id + '-c' + c.id + '">' + esc(why) + ' <a href="' + href(row.url) + '" target="_blank" rel="noopener">board ↗</a></span>';
    }
    if (q.choices.length) h += '</div>';
    // § 2: the PM's pick sits beside the worker's star; it is a view, never an answer
    if (row.pick) h += '<p class="qac-pick"><span class="qac-muted">PM</span> ' + esc(row.n) + '.' + esc(row.pick.id) + ': ' + esc(row.pick.reason) + '</p>';
    if (own && (st !== 'read-only' || local.draft)) {
      const bad = textProblem(q, local.draft);
      // Q1: Other answer opens a compact field; a kept draft keeps it open; an owner action without options has only this field
      h += '<details class="qac-other"' + (local.draft || !q.choices.length ? ' open' : '') + '><summary>' + (q.choices.length ? 'Other answer' : 'Answer') + '</summary><form class="qac-text-form" data-qac="submit"><textarea id="' + id + '-d" data-qac="input" rows="2" aria-label="Your answer" placeholder="' + (q.choices.length ? 'Or type an answer' : 'What you did') + '"' + (live ? '' : ' readonly') + '>' + esc(local.draft) + '</textarea>' +
        '<button type="submit"' + (live && !bad ? '' : ' disabled') + '>Send</button>' + hint(id, live && bad) + '</form></details>';
    }
    const reason = st === 'read-only' ? (cap && cap.reason) || 'Answering is not available here' : local.receipt && local.receipt.reason;
    h += receipt(st, reason, row.url, id, local.receipt && local.receipt.next, false, what);
    if (st === 'stale') h += '<button type="button" data-qac="review">I read the new question</button>';
    h += retry(st, local.receipt && local.receipt.reason);
    // #1787: evidence links stay behind Details; Done, Problem and images show above
    const links = q.evidence.filter(e => !e.image);
    if (links.length) h += '<details class="qac-context"><summary>Evidence</summary><p class="qac-evidence">' +
      links.map(e => '<a href="' + href(e.url) + '" target="_blank" rel="noopener">' + esc(e.label) + '</a>').join(' · ') + '</p></details>';
    return h + '</section>';
  },
compose:function(row, local, cap) {
    // What a shared composer shows for one card: null when the card takes no free text here
    const q = row.question || {}, st = status(row, local, cap), text = !!(cap && cap.answer) && (fallback(row, cap) || !!q.parsed);
    if (!text) return null;
    const live = LIVE.includes(st);
    return {draft: local.draft, caret: local.caret, state: st, live, problem: live ? textProblem(draftQuestion(row, cap), local.draft) : LABELS[st],
      send: fallback(row, cap) ? 'Requeue' : 'Send', choices: !!(q.choices && q.choices.length), requeue: manageable(row, cap) && !fallback(row, cap) ? live ? textProblem(REQUEUE, local.draft) || '' : LABELS[st] : null};
  },
receipt:function(st, reason, url, id, next, action, what) {
    if (st === 'actionable') return '<p class="qac-receipt" id="' + id + '-r" role="status" aria-live="polite"></p>';
    return '<p class="qac-receipt qac-' + st + '" id="' + id + '-r" role="status" aria-live="polite">' + (what && st !== 'read-only' ? '<span class="qac-what">' + esc(what) + '</span> · ' : '') + esc(st === 'accepted' && action ? 'Action accepted on the board' : LABELS[st]) + (reason ? ': ' + esc(plain(reason)) : '') +
      (next ? '. ' + esc(next) : '') + (['read-only', 'unknown', 'partial'].includes(st) ? ' <a href="' + href(url) + '" target="_blank" rel="noopener">Open on board ↗</a>' : '') + '</p>';
  },
mount:function(el, adapter) {
const {apply,paint,markup}=execution.module("answer-card.mount",{
markup:function() {
      return [...cards].map(([k, c]) => '<div data-qac-key="' + esc(k) + '">' + render(c.board, c.row, locals.get(k), adapter.capability(c.board, c.row)) + '</div>').join('');
    },
apply:function(k, ev) {
      const {board, row} = cards.get(k), cap = adapter.capability(board, row), r = step(row, locals.get(k), cap, ev, newId);
      locals.set(k, r.local);
      adapter.save?.(board, row, r.local);
      if (ev.type !== 'input') paint();
      if (r.send) ui.effect('answer.send',()=>adapter.send(board,r.send),x=>{if(cards.has(k))apply(k,{type:'receipt',opId:r.send.id,...x});});
      else if (ev.type === 'input') {
        const s = el.querySelector('[data-qac-key="' + CSS.escape(k) + '"] button[type=submit]'), cap2 = adapter.capability(board, row);
        const live = LIVE.includes(status(row, r.local, cap2)), bad = textProblem(draftQuestion(row, cap2), r.local.draft);
        if (s) s.disabled = !!bad || !live;
        const rq = el.querySelector('[data-qac-key="' + CSS.escape(k) + '"] [data-qac=requeue]');
        if (rq) rq.disabled = !live || !!textProblem(REQUEUE, r.local.draft);
        // #1456: a disabled button states why; a state other than live already says so in the receipt
        const h = s && s.parentNode.querySelector('.qac-hint');
        if (h) h.textContent = live && bad || '';
        // the live textarea already shows this draft: a refresh with the same rows must not rebuild it
        painted = markup();
      }
    },
paint:function() {
      const a = document.activeElement, focus = a && el.contains(a) ? {k: a.closest('[data-qac-key]')?.dataset.qacKey, sel: a.dataset.qac === 'input' ? 'textarea' : a.dataset.choice ? '[data-choice="' + a.dataset.choice + '"]' : '[data-qac="' + a.dataset.qac + '"]', s: a.selectionStart, e: a.selectionEnd} : null;
      const scroll = el.scrollTop, open = new Set([...el.querySelectorAll('details[open]')].map(d => d.closest('[data-qac-key]')?.dataset.qacKey + ' ' + d.className));
      const html = markup();
      // an unchanged card keeps its nodes: focus, caret, open details and an in-progress edit stay untouched
      if (html === painted) return;
      el.innerHTML = painted = html;
      for (const d of el.querySelectorAll('details')) if (open.has(d.closest('[data-qac-key]').dataset.qacKey + ' ' + d.className)) d.open = true;
      el.scrollTop = scroll;
      for(const [k,local] of locals)if(Array.isArray(local.caret)&&local.caret.length===2){const input=el.querySelector('[data-qac-key="'+CSS.escape(k)+'"] textarea');input?.setSelectionRange(...local.caret);}
      const t = focus && focus.k && el.querySelector('[data-qac-key="' + CSS.escape(focus.k) + '"] ' + focus.sel);
      if (t) { t.focus(); if (focus.s != null && t.setSelectionRange) t.setSelectionRange(focus.s, focus.e); }
    }
});

    const locals = new Map(), cards = new Map();
    let painted = null;
    const newId = adapter.newId || (() => (root.crypto && root.crypto.randomUUID ? root.crypto.randomUUID() : 'qac-' + Date.now().toString(36) + Math.random().toString(36).slice(2)));


    ui.listen(el,'click', e => {
      const b = e.target.closest('button[data-qac]'), k = b && b.closest('[data-qac-key]')?.dataset.qacKey;
      if (k && !b.disabled) apply(k, b.dataset.qac === 'choose' ? {type: 'choose', id: Number(b.dataset.choice)} : {type: b.dataset.qac});
    });
    ui.listen(el,'submit', e => { e.preventDefault(); const k = e.target.closest('[data-qac-key]')?.dataset.qacKey; if (k) apply(k, {type: 'submit'}); });
    ui.listen(el,'keydown', e => { if (e.target.dataset.qac === 'input' && e.key === 'Enter' && !e.shiftKey && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); const k = e.target.closest('[data-qac-key]')?.dataset.qacKey; if(k) apply(k, {type: 'submit'}); } });
    for(const event of ['select','keyup','click'])ui.listen(el,event,e=>{
      const k=e.target.dataset.qac==='input'&&e.target.closest('[data-qac-key]')?.dataset.qacKey;
      if(k&&adapter.save){const c=cards.get(k),local={...locals.get(k),caret:[e.target.selectionStart,e.target.selectionEnd]};locals.set(k,local);adapter.save(c.board,c.row,local);}
    });
    ui.listen(el,'input', e => { const k = e.target.dataset.qac === 'input' && e.target.closest('[data-qac-key]')?.dataset.qacKey; if (k) apply(k, {type: 'input', text: e.target.value}); });
    return execution.module("answer-card.mount.controller",{
      receiveReceipt(board, opId, reply) {
        for (const [k, card] of cards) if (card.board === board && locals.get(k)?.op?.id === opId) {
          apply(k, {...reply, type: 'receipt', opId});
        }
      },
      // #1662 A3: an adapter whose capability has `composer` owns text entry in its shared composer and drives the card here;
      // text null records only the caret
      input(board, n, text, caret) {
        const k = key(board, {n});
        if (!cards.has(k)) return;
        if (text != null && text !== locals.get(k).draft) apply(k, {type: 'input', text});
        if (caret) { const c = cards.get(k), local = {...locals.get(k), caret}; locals.set(k, local); adapter.save?.(c.board, c.row, local); }
      },
      act(board, n, type) { const k = key(board, {n}); if (cards.has(k) && ['submit', 'requeue'].includes(type)) apply(k, {type}); },
      // rows: [{board, row}] of open questions; a row that left keeps no state
      update(rows) {
        cards.clear();
        for (const c of rows) { const k = key(c.board, c.row); cards.set(k, c); if (!locals.has(k)) locals.set(k, adapter.load?.(c.board, c.row) || fresh(c.row)); }
        for (const k of [...locals.keys()]) if (!cards.has(k)) locals.delete(k);
        paint();
      },
    });
  },
replay:function(fx, capability) {
    let row = fx.rows[0], local = fresh(row), n = 0;
    const sent = [], out = [];
    for (const ev of fx.events || []) {
      if (ev.type === 'refresh') { row = fx.rows[ev.row]; continue; }
      if (ev.type === 'check') { out.push({status: status(row, local, capability(fx.board, row)), html: render(fx.board, row, local, capability(fx.board, row)), sent: sent.slice(), draft: local.draft}); continue; }
      const r = step(row, local, capability(fx.board, row), ev.type === 'receipt' ? {...ev, opId: sent[sent.length - 1] && sent[sent.length - 1].id} : ev, () => 'op-' + ++n);
      local = r.local;
      if (r.send) sent.push(r.send);
    }
    return out;
  }
});
// TaskQ shared answer card (docs/design/answer-card.md). Source: https://github.com/taskqorg/taskq src/answer-card.js,
// MIT, published with each TaskQ release as taskq-<version>-answer-card.js and listed in SHA256SUMS; never edit a copy.
// Input: one status --json task row whose `question` is TaskQ's canonical parse, plus the adapter's capability.
// The component has no network client, parser, credentials or task state machine: `send` is the adapter's.
const root=globalThis;
  const STATES = ['actionable', 'sending', 'accepted', 'stale', 'rejected', 'failed', 'partial', 'unknown', 'read-only'];
  // failed: the bridge refused before dispatch, nothing reached the board; partial: a two-step action stopped after its first
  const LABELS = {actionable: 'Waiting for your answer', sending: 'Sending…', accepted: 'Answer accepted on the board',
    stale: 'The question changed: review it before sending', rejected: 'Answer not accepted', failed: 'Not sent: nothing reached the board',
    partial: 'Partly applied: check the board', unknown: 'Delivery unknown: check the board', 'read-only': 'Read only'};
  const LIVE = ['actionable', 'rejected', 'failed'], HELD = ['sending', 'unknown', 'failed', 'partial'], RETRY = ['unknown', 'failed', 'partial'];
  const REQUEUE = {limits: {lines: 1, chars: 232}};
  const {esc}=execution.module("answer-card",{esc:s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'}[c]))});
  const {key}=execution.module("answer-card",{key:(board, row) => board + '#' + row.n});
  // only web links: card text is worker-written
  const {href}=execution.module("answer-card",{href:u => /^https?:\/\//i.test(u || '') ? esc(u) : '#'});
  // § 2 card links `[label](url)`: anchors show only the label
  const LINK = /\[([^\]]*)\]\(([^)\s]*)\)/g;
  const {label}=execution.module("answer-card",{label:s => String(s == null ? '' : s).replace(LINK, '$1')});
  const {linked}=execution.module("answer-card",{linked:s => {
    const p = String(s == null ? '' : s).split(LINK);
    let h = esc(p[0]);
    for (let i = 1; i < p.length; i += 3) h += '<a href="' + href(p[i + 1]) + '" target="_blank" rel="noopener">' + esc(p[i]) + '</a>' + esc(p[i + 2]);
    return h;
  }});

  // Local state of one card: draft and the revision it was written against, the one operation in flight and its receipt.


  // Free text the TaskQ `answer N --text` limits accept; null: sendable.


  // § 2 visual evidence: ask images stay in view as thumbnails that open full size; without the app's copy, the link
  // Visual choice (#1787): one image per option, image K captioned K.
  const {images}=execution.module("answer-card",{images:q => {
    const im = (q.evidence || []).filter(e => e.image), n = (q.choices || []).length, each = n >= 2 && im.length === n;
    return im.length ? '<p class="qac-images">' + im.map((e, i) => (each ? '<span class="qac-shot">' : '') + '<a href="' + href(e.url) + '" target="_blank" rel="noopener" title="Open ' + esc(e.label) + ' full size">' +
      (/^data:image\/(png|jpeg|gif|webp);base64,/.test(e.src || '') ? '<img class="qac-thumb" src="' + esc(e.src) + '" alt="' + esc(e.label) + '">' : esc(e.label) + ' ↗') + '</a>' +
      (each ? '<span class="qac-caption">' + (i + 1) + '.</span></span>' : '')).join('') + '</p>' : '';
  }});
  // One render (#1787): the Done and Problem lines, each after its icon, in full
  const {lines}=execution.module("answer-card",{lines:q => (q.done ? '<p class="qac-done"><span class="qac-icon" role="img" aria-label="Done">✓</span> ' + linked(q.done) + '</p>' : '') +
    '<p class="qac-problem"><span class="qac-icon" role="img" aria-label="Problem">⚑</span> ' + linked(q.problem) + '</p>'});
  const {manageable}=execution.module("answer-card",{manageable:(row, cap) => row.state === 'ask' && cap && cap.answer && cap.manage && !(row.question && row.question.choices && row.question.choices.length)});
  // The tray fallback proper: an unparsed card whose only free text is a requeue reason.
  const {fallback}=execution.module("answer-card",{fallback:(row, cap) => manageable(row, cap) && !(row.question && row.question.parsed)});
  // What one receipt answers: the source-numbered choice and its text, a text answer or a fallback action.
  const {about}=execution.module("answer-card",{about:(row, op) => {
    if (op.action) return {requeue: 'requeue', later: 'later', close: 'close without publication'}[op.action] || op.action;
    if (op.text != null) return 'text answer';
    const c = row.question && row.question.choices && row.question.choices.find(x => x.id === op.choice);
    return 'answer ' + op.choice + (c ? '. ' + label(c.label == null ? c.text : c.label) : '');
  }});
  // #1699 A4: a retained correlation conflict reads in user terms and offers Check delivery, never a resend
  const CONFLICT = 'Correlation id is already pending for a different request';
  const {retry,plain}=execution.module("answer-card",{retry:(st, reason) => RETRY.includes(st) ? '<button type="button" data-qac="retry">' + (st === 'failed' && reason !== CONFLICT ? 'Send again' : 'Check delivery') + '</button>' : '',
    plain:reason => reason === CONFLICT ? 'An earlier answer with this operation is still pending with different content; Check delivery reads its outcome' : reason});
  // Why the text button is off while the card is live; an empty node otherwise, so typing updates it in place.
  const {hint}=execution.module("answer-card",{hint:(id, why) => '<span class="qac-hint" id="' + id + '-h" aria-live="polite">' + esc(why || '') + '</span>'});
  const {draftQuestion}=execution.module("answer-card",{draftQuestion:(row, cap) => fallback(row, cap) ? REQUEUE : row.question});

  // One of STATES for a row, its local state and the adapter's capability {answer, reason}.


  // Pure transition: (local, event) -> {local, send}. `send` is an operation to hand to the adapter, else null.
  // Events: input {text}, choose {id}, submit, review, receipt {opId, state, reason, next}, refresh {row}, retry.






  // Browser binding: one container, rows by board. adapter = {capability(board, row) -> {answer, reason},
  // send(board, op) -> Promise<{state, reason, next}>, newId?() }. update() keeps drafts, focus and scroll.


  // Conformance replay shared by every consumer harness: the fixture's events through `step` with the consumer's
  // adapter capability; returns each checkpoint's {status, html, sent} for the fixture's expectations.


  const api = {STATES, fresh, status, step, render, compose, mount, replay, textProblem};
  module.exports=api;
