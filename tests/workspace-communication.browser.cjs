// #1464 A1/A3/A6: real bundled workspace, invented native snapshots; no native/board effects.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const project={root:'fixture-alpha',name:'Alpha',issues:'https://example.org/alpha/issues',alive:true,complete:true,age:1,focus:[],labels:[],tasks:[]};
const evidence=process.env.TASKQ_EVIDENCE_1464;if(evidence)fs.mkdirSync(evidence,{recursive:true});
const snapshot={font:16,theme:'dark',host:'fixture',now:1000,projects:[project],ai:{draft:'',history:[],runtime:{available:true,name:'Claude Code',id:'claude'}},workspace_local:{}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:800,height:600}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(s=>taskq.receive(s),snapshot);
  assert.equal(await page.locator('[data-a=refresh]').count(),1,'G08: workspace needs a named Refresh recovery control');
  await page.locator('[data-a=refresh]').focus();
  await page.evaluate(s=>taskq.receive({...s,ai:{...s.ai,request:{id:'new',kind:'starting',next:'Starting native session'}}}),snapshot);
  assert.equal(await page.evaluate(()=>document.activeElement.dataset.a),'refresh','G08: changed update retains Refresh focus');
  assert(await page.locator('[role=status][aria-live=polite]').count(),'G08: routine changes announced politely');
  // #1662 A3: the legacy New AI request route kept a second request composer beside Conversations. Risk kept: a stale
  // link or tray button reaching a removed form. An old request link opens a new Conversations draft for its project.
  await page.evaluate(()=>taskq.receive({open:{page:'request',root:'fixture-alpha'}}));
  assert.equal(await page.locator('h1').innerText(),'Conversations');
  assert.equal(await page.locator('[data-a=request-send], [data-field=request]').count(),0,'no legacy request form remains');
  assert.equal(await page.evaluate(()=>document.activeElement.id),'conversation-draft','New focuses the new draft');
  assert.equal(await page.locator('[data-c-field=root]').inputValue(),'fixture-alpha');
  assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='ai-send').length),0,'opening a draft sends nothing');
  // Unavailable context stays explicit, never routes an answer to another project.
  await page.evaluate(()=>taskq.receive({open:{page:'answer',board:'https://missing.example/issues',task:1}}));
  if(evidence)await page.screenshot({path:evidence+'/task-unavailable-dark-16.png'});
  assert.match(await page.locator('.content').innerText(),/task is unavailable/);assert.equal(await page.locator('.qac').count(),0);
  await page.evaluate(()=>taskq.receive({open:{page:'removed-route'}}));assert.match(await page.locator('.content').innerText(),/surviving destination is Messages/);
  await page.evaluate(()=>taskq.receive({open:{page:'messages'}}));assert(!/Requested destination/.test(await page.locator('.content').innerText()),'valid navigation clears the obsolete destination warning');
  const fx=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json','utf8')),row=fx.cases.find(c=>c.name.startsWith('stale')).rows[0],board=project.issues;
  const question={...snapshot,projects:[{...project,tasks:[row]}]};
  await page.evaluate(s=>taskq.receive(s),question);await page.evaluate(c=>taskq.receive({open:c}),{page:'answer',board,task:row.n,revision:row.revision});
  const answer=page.locator('[data-c-field=answer]');
  await answer.fill('answer draft after restart');await answer.evaluate(e=>e.setSelectionRange(4,9));await answer.dispatchEvent('select');
  const savedAnswer=await page.evaluate(()=>({local:messages.filter(m=>m.a==='answer-local').at(-1),navigation:messages.filter(m=>m.a==='workspace-set').at(-1).navigation}));
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(({s,saved})=>taskq.receive({...s,workspace_navigation:saved.navigation,workspace_local:{[saved.local.key]:saved.local.local}}),{s:question,saved:savedAnswer});
  if(evidence)await page.screenshot({path:evidence+'/answer-restored-dark-16.png'});
  assert.equal(await answer.inputValue(),'answer draft after restart');assert.deepEqual(await answer.evaluate(e=>[e.selectionStart,e.selectionEnd]),[4,9]);
  assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='answer').length),0,'reload never replays an answer');
  await answer.press('Enter');await page.waitForFunction(()=>messages.some(m=>m.a==='answer'));
  const operation=await page.evaluate(()=>messages.find(m=>m.a==='answer').op),pending=await page.evaluate(()=>messages.filter(m=>m.a==='answer-local').at(-1));
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(({s,saved,pending})=>taskq.receive({...s,workspace_navigation:saved.navigation,workspace_local:{[pending.key]:pending.local}}),{s:question,saved:savedAnswer,pending});
  assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'unknown');assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='answer').length),0);
  await page.locator('[data-qac=retry]').click();await page.waitForFunction(()=>messages.some(m=>m.a==='answer'));
  assert.deepEqual(await page.evaluate(()=>messages.find(m=>m.a==='answer').op),operation,'restart reconciliation retains exact ID/revision/payload');
  assert.deepEqual(errors,[]);console.log('PASS A3/A4/A6/A7: Refresh focus, polite status, request route opens Conversations, answer composer draft and caret reload, immutable retry, unavailable recovery');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
