// Chain: #1400, #1426 newest setup request view in src/tray-app.js. Risk: the owner cannot open the setup session or tell runtime, login and access failures apart.
const assert=require('node:assert/strict');
const context={S:{projects:[],setup:null},esc:s=>String(s).replaceAll('<','&lt;').replaceAll('"','&quot;')};
require('./tray-harness.cjs').install(context);
assert.equal(context.setup(),'','no request, no block');
context.S.setup={kind:'launched',next:'Open the session to read and answer',url:'https://claude.ai/code/session_01Ab'};
assert(context.setup().includes('data-url="https://claude.ai/code/session_01Ab">Open session'),'launched session opens');
context.S.setup={kind:'launched',next:'x',url:'https://evil.example/session_01Ab'};
assert(!context.setup().includes('data-url'),'only a Claude session link opens');
for(const kind of ['runtime','login','access','failed','interrupted']){
  context.S.setup={kind,reason:'r-'+kind,next:'n-'+kind};
  const h=context.setup();
  assert(h.includes('Setup · '+kind)&&h.includes('r-'+kind)&&h.includes('n-'+kind),kind);
}
console.log('PASS setup block: Open session link, distinct runtime/login/access/failed/interrupted states');
