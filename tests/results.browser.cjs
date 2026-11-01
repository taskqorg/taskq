// #1940 owner expansion / §7 Results: one cached result flow on Menu and Conversations.
// Risk: ordinary completions are invisible, navigation fetches a board, or Reopen loses its reason/readback.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
// RC1/RC4: replaces obsolete menu result-engine assertions. Card/receipt/refusal/matrix protection remains in reports.browser.cjs and reports-cache.test.cjs; centre protects entry/scope/history.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const board='https://example.org/fixture/issues',sha='a'.repeat(40);
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const p=await browser.newPage({viewport:{width:940,height:700}}),errors=[];p.on('pageerror',e=>errors.push(e.message));await p.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 const state={font:18,theme:'dark',projects:[{root:'fixture',issues:board,name:'Fixture',alive:true,complete:true,tasks:[{n:13,title:'One selected task',state:'ask',revision:30}],reports:{complete:true,reports:[]}}],conversations:{complete:true,items:[]},messages:{complete:true,entries:[]},ai:{history:[]}};
 await p.evaluate(({state,board})=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};taskq.receive(state);taskq.receive({open:{page:'conversations',root:'fixture',board,task:13}});},{state,board});
 state.projects[0].tasks=[];state.projects[0].reports.reports=[{n:13,sha,title:'One selected task',kind:'task',url:board+'/13',closed:100,read:false,visible:true,outcome:'success',body:'Changed: Delivered.\nChecks: exact source -- passed\nOpen: none'}];await p.evaluate(s=>taskq.receive(s),state);
 assert.equal(await p.locator('.conversation-row[aria-current=page][data-n="13"]').count(),1,'one task identity survives question to result');assert.match(await p.locator('.report-card').innerText(),/exact source -- passed/);assert.equal(await p.locator('[data-result-card=decision]').count(),0,'ordinary result has no required acknowledgement');
 assert.equal(await p.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='reports'&&['list','open','accept'].includes(m.action)).length),0,'closure display and local navigation never send an answer/Accept or board read');assert.deepEqual(errors,[]);console.log('PASS selected question becomes one ordinary completion in the shared reader');
 }finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
