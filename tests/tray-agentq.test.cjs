// Chain: #1391 A2-A4. Risk: unknown hides a warning, dismissal hides failures, or setup clicks lose their selected scope.
'use strict';
const assert=require('node:assert/strict');
const sent=[];const c={operationController:true,S:{projects:[],agentq:null},send:m=>sent.push(m)};
require('./tray-harness.cjs').install(c);
assert.equal(c.agentq(), '');
const base={kind:'shown',warning:true,dismissed:false,host:'test-host',platform:'macos',targets:{codex:['/invented/agentq','/invented/.codex'],claude:['/invented/agentq','/invented/.claude']},runtimes:[{runtime:'codex',client:'Codex',eligible:true},{runtime:'claude',client:'Claude Code',eligible:true}]};
c.S.agentq=base;
let html=c.agentq();assert.match(html,/Save on repeated context/);assert.match(html,/OK — set up agentq/);assert.match(html,/no saving is guaranteed/);assert.match(html,/test-host/);assert.match(html,/\/invented\/agentq/);
c.operationAction({a:'agentq-runtime',runtime:'claude'});
c.operationAction({a:'agentq-setup'});assert.equal(sent.at(-1).runtime,'claude');assert.match(sent.at(-1).id,/^[A-Za-z0-9-]+$/);
c.S.agentq={...base,dismissed:true};assert.equal(c.agentq(),'');assert.match(c.agentq(true),/Set up/);
for(const kind of ['unknown','failed','interrupted','stopped']){c.S.agentq={...base,kind,dismissed:true,reason:'kept diagnostic'};assert.match(c.agentq(),/kept diagnostic/);}
c.S.agentq={...base,kind:'running',warning:false};assert.equal(c.agentq(),'');assert.match(c.agentq(true),/agentq running/);
c.S.agentq={...base,kind:'setting-up',setup:{url:'codex://threads/fixture-thread',kind:'launched'}};assert.match(c.agentq(),/Setting up agentq/);assert.match(c.agentq(),/Open session/);
c.S.agentq={...base,runtimes:[{runtime:'codex',client:'Codex',eligible:false,reason:'access refused'}]};html=c.agentq();assert.match(html,/disabled/);assert.match(html,/access refused/);assert.match(html,/Install agentq from/);
console.log('PASS agentq onboarding: state presentation, dismissal, runtime scope and actionable unavailable');
