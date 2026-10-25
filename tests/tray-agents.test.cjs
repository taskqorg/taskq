// Chain: #1702 Agents page in src/tray-app.js. Risk: off AgentQ reads as a setup prompt, and remote client facts look known without a source.
const assert=require('node:assert/strict');
const context={operationController:true,S:{host:'mac',version:'4.6.40',projects:[{host:'mac',version:'4.6.40',hosts:[{name:'mac',age_text:'5s'},{name:'hermes-fra1',age_text:'40s'}],tasks:[{executions:[{host:'win'}]}]}],
  agents:{at:1,age:'1m',fresh:true,rows:[{runtime:'codex',version:'0.159.3',state:'signed-in',refreshed:'2026-10-19T18:14:41Z'}]},agentq:{kind:'stopped',host:'mac',platform:'macos',reason:'agentq off or unreachable · last status is stale and its process is absent',runtimes:[{runtime:'claude',client:'Claude Code',eligible:true}],targets:{claude:['a','b']}}}};
require('./tray-harness.cjs').install(context);
// A2: off and unknown AgentQ say so with their reason; no runtime picker or setup button
for(const kind of ['stopped','unknown']){
  context.S.agentq.kind=kind;const h=context.agentq(true);
  assert(h.includes(kind==='stopped'?'agentq off or unreachable':'agentq status unknown')&&h.includes(context.S.agentq.reason),kind);
  assert(!h.includes('agentq-setup')&&!h.includes('agentq-runtime'),kind+': setup is not the recovery');
  assert(h.includes('agentq-check'),kind+': Check again stays');
}
context.S.agentq.kind='shown';assert(context.agentq(true).includes('agentq-setup'),'not installed still offers setup');
// A3: remote hosts show heartbeat freshness only; client facts read unknown with the missing source; expiry never from refresh
context.S.agentq=null;const page=context.agents();
assert(page.includes('hermes-fra1 · heartbeat 40s old · clients unknown'),'heartbeat freshness is sourced');
assert(page.includes('win · heartbeat unknown: no host row'),'host seen only in an execution has no heartbeat row');
assert(page.includes('Codex · unknown: hermes-fra1 heartbeat publishes no client version, sign-in, account or access'),'remote client facts name the missing source');
assert(page.includes('Expiry unknown')&&page.includes('not an expiry'),'refresh time is never an expiry');
console.log('PASS agents page: off/unknown AgentQ without setup prompt; remote client facts unknown with source; expiry only when sourced');
