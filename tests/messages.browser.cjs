// #1467 A1–A3/A7: actual bundled renderer, invented native message snapshots and receipts; no live effects.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const evidence=process.env.TASKQ_EVIDENCE_1467;if(evidence)fs.mkdirSync(evidence,{recursive:true});
const q='1'.repeat(64),r='2'.repeat(64),p='3'.repeat(64),board='https://example.org/fixture/issues';
const entries=[
 {key:q,kind:'question',title:'Choose the cart quantity',project:'Fixture',root:'fixture',board,task:7,revision:9,state:'ask',pending:true,read:false,archived:false},
 {key:r,kind:'request',title:'AI request · launched',root:'fixture',host:'fixture-host',id:'native-live',state:'launched',pending:true,read:false,archived:false,request:{next:'Read replies in the native runtime.',url:'https://claude.ai/code/session_Invented123'}},
 {key:p,kind:'request',title:'AI request · launching',root:'fixture',host:'fixture-host',id:'not-launched',state:'launching',pending:false,read:false,archived:false}
];
const snapshot={font:16,theme:'dark',host:'fixture-host',now:1000,projects:[{root:'fixture',name:'Fixture',issues:board,alive:true,complete:true,age:1,tasks_age_seconds:1,focus:[],labels:[],tasks:[{n:7,title:'Choose the cart quantity',state:'ask',revision:9}]}],messages:{entries,count:2,complete:true,reason:''},ai:{draft:'preserved composer draft',history:[],runtime:{available:true,name:'Claude Code',id:'claude'}},workspace_local:{}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try {
  const page=await browser.newPage({viewport:{width:800,height:620}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  if(evidence&&process.env.TASKQ_BEFORE_HTML_1467){
   await page.setContent(fs.readFileSync(process.env.TASKQ_BEFORE_HTML_1467,'utf8'));await page.evaluate(s=>taskq.receive(s),snapshot);
   assert.equal(await page.locator('[data-field=message-filter]').count(),0,'before: archive/list controls absent');
   await page.screenshot({path:evidence+'/messages-before-dark-16.png'});
  }
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:m=>sent.push(JSON.parse(m))};});
  await page.evaluate(s=>taskq.receive(s),snapshot);
  assert.equal(await page.locator('[data-a=message-open]').count(),3);
  if(evidence)await page.screenshot({path:evidence+'/messages-dark-16.png'});
  await page.locator('[data-field=message-filter]').selectOption('pending');
  assert.equal(await page.locator('[data-a=message-open]').count(),2);
  await page.locator('[data-a=message-open][data-key="'+r+'"]').focus();
  await page.locator('[data-a=message-open][data-key="'+r+'"]').press('Enter');
  const open=await page.evaluate(()=>sent.filter(m=>m.a==='message-action').at(-1));assert.equal(open.action,'read');
  assert.equal(await page.locator('h1').innerText(),'Message');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='ai-send'||m.a==='answer').length),0,'opening is never sending or answering');
  await page.evaluate(({id,key})=>taskq.receive({messageReceipt:{id,outcomes:[{key,action:'read',state:'accepted'}]}}),{id:open.id,key:r});
  await page.keyboard.press('Alt+ArrowLeft');assert.equal(await page.locator('[data-field=message-filter]').inputValue(),'pending');
  assert.equal(await page.evaluate(()=>document.activeElement.dataset.key),r,'Back restores invoker focus');
  await page.locator('[data-a=message-archive][data-key="'+r+'"]').click();
  const archive=await page.evaluate(()=>sent.filter(m=>m.a==='message-action').at(-1));assert.equal(archive.action,'archive');
  await page.evaluate(({id,key})=>taskq.receive({messageReceipt:{id,outcomes:[{key,action:'archive',state:'accepted'}]}}),{id:archive.id,key:r});
  const archived=structuredClone(snapshot);archived.messages.entries[1].archived=true;archived.messages.count=1;
  await page.evaluate(s=>taskq.receive(s),archived);assert.equal(await page.locator('[data-a=message-open]').count(),1);
  await page.locator('[data-field=message-filter]').selectOption('archived');
  assert.equal(await page.locator('[data-a=message-open]').count(),1);
  assert.match(await page.locator('.content').innerText(),/There is no undo/);
  if(evidence)await page.screenshot({path:evidence+'/archived-dark-16.png'});
  const saved=await page.evaluate(()=>sent.filter(m=>m.a==='workspace-set').at(-1).navigation);
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:m=>sent.push(JSON.parse(m))};});
  await page.evaluate(({s,navigation})=>taskq.receive({...s,workspace_navigation:navigation}),{s:archived,navigation:saved});
  assert.equal(await page.locator('[data-field=message-filter]').inputValue(),'archived','restart retains list/filter');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='message-action'||m.a==='ai-send').length),0,'rehydration replays no effects');
  await page.locator('.content [data-route=request]').click();
  assert.equal(await page.locator('[data-field=request]').inputValue(),'preserved composer draft','archive never clears composer drafts');
  await page.keyboard.press('Alt+ArrowLeft');
  const mixed=structuredClone(archived);mixed.messages.entries[2].archived=true;
  await page.evaluate(s=>taskq.receive(s),mixed);
  await page.locator('[data-a=message-delete-all]').click();
  const remove=await page.evaluate(()=>sent.filter(m=>m.a==='message-action').at(-1));assert.equal(remove.action,'delete');assert.deepEqual(remove.keys,[r,p]);
  await page.evaluate(({id,r,p})=>taskq.receive({messageReceipt:{id,outcomes:[{key:r,action:'delete',state:'accepted'},{key:p,action:'delete',state:'unknown',reason:'Request not-launched is launching; Delete refused before launch'}]}}),{id:remove.id,r,p});
  const partial=structuredClone(mixed);partial.messages.entries=partial.messages.entries.filter(e=>e.key!==r);
  await page.evaluate(s=>taskq.receive(s),partial);
  assert.match(await page.locator('[data-status=workspace]').innerText(),/Unknown:.*launching/);
  assert.equal(await page.locator('[data-a=message-open]').count(),1);
  await page.setViewportSize({width:360,height:640});
  await page.evaluate(s=>taskq.receive({...s,theme:'light',font:20,messages:{...s.messages,complete:false,count:null,reason:'Fixture: board source stale'}}),partial);
  assert.match(await page.locator('.content').innerText(),/Messages unknown: Fixture: board source stale/);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'narrow view has no horizontal clipping');
  if(evidence)await page.screenshot({path:evidence+'/partial-light-20-360.png'});
  await page.keyboard.press('Escape');assert(await page.evaluate(()=>sent.some(m=>m.a==='hide')));
  assert(!await page.evaluate(()=>sent.some(m=>['cancel','ai-clear','ai-send','answer'].includes(m.a))),'close/archive/delete never cancel work or clear command history');
  assert.deepEqual(errors,[]);
  console.log('PASS TW5–TW7: list/filter, keyboard open without resend, Back/focus, archive readback, native draft/restart preservation, mixed deletion/unknown and narrow layout');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
