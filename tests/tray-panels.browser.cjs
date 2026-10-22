// R6/R12, TW3, G05/G06: process figures and fault visibility before one complete task list.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const project={root:'/fixture/alpha',name:'Alpha',host:'mac',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,tasks_age_seconds:0,observed:1000,hosts:[{name:'mac',updated:990}],focus:['all'],labels:[],tasks:[{n:1,state:'doing',title:'A full wrapped task title that remains readable across narrow menu widths and large type',runtime:'codex',host:'mac',local_session:true,session_url:'codex://threads/fixture',revision:1,executions:[{id:11,attempt:10,host:'mac',runtime:'codex',role:'worker',state:'running',observed:990}]}]};
const snapshot={platform:'windows',font:16,theme:'dark',host:'mac',now:1000,version:'fixture',projects:[project],notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:320,height:760}}),failures=[];
  page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  const check=(ok,reason)=>{if(!ok)failures.push(reason);};
  const shot=async name=>{if(process.env.PANEL_EVIDENCE){fs.mkdirSync(process.env.PANEL_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.PANEL_EVIDENCE+'/'+name+'.png'});}};
  await page.evaluate(s=>taskq.receive(s),{...snapshot,page:'home'});
  await shot('G06-home');
  check(await page.locator('[data-card]').count()===1,'Home must show the complete task list after panels');
  check(!await page.locator('.content').innerText().then(t=>t.includes('Attention')),'G06: empty Attention takes no row');
  await page.evaluate(s=>taskq.receive(s),{...snapshot,page:project.root});
  await shot('G05-project');
  check(await page.getByRole('button',{name:'Alpha',exact:true}).count()===0,'G05: project identity must appear only in navigation');
  check(await page.getByRole('link',{name:/Board/}).count()===1,'G05/V08: one independent Board destination');
  assert.deepEqual(failures,[]);
  for(const font of [16,20])for(const theme of ['dark','light']){
   await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme,page:'home'});
   if(!await page.locator('.dashboard-status').evaluate(e=>e.open))await page.locator('.dashboard-status>summary').click();
   assert.equal(await page.locator('textarea,.qac').count(),0);
   assert(await page.locator('.running-agents').evaluate(e=>e.compareDocumentPosition(document.querySelector('[data-card]'))&Node.DOCUMENT_POSITION_FOLLOWING));
   assert.equal(await page.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth),true);
   assert.equal(await page.locator('.tqp-task-title').evaluate(e=>e.scrollHeight<=e.clientHeight&&getComputedStyle(e).whiteSpace!=='nowrap'),true);
  }
  await page.evaluate(s=>taskq.receive(s),{...snapshot,page:'home',projects:[{...project,error:'Fixture urgent fault'}]});
  assert.match(await page.locator('.content').innerText(),/Fixture urgent fault/);
  assert(await page.locator('.alert').first().evaluate(e=>e.compareDocumentPosition(document.querySelector('[data-card]'))&Node.DOCUMENT_POSITION_FOLLOWING));
  // A stale process source never turns Doing into a running figure.
  await page.evaluate(s=>taskq.receive(s),{...snapshot,projects:[{...project,age:61}]});
  assert.match(await page.locator('.running-agents').innerText(),/unknown.*stale/i);
  assert.doesNotMatch(await page.locator('.running-agents').innerText(),/codex 1/);
  await page.evaluate(s=>taskq.receive(s),{...snapshot,projects:[{...project,tasks:[{...project.tasks[0],state:'ask',revision:9}]}]});
  await page.locator('[data-a="task-detail"]').click();await page.locator('[data-page="answer"]').click();
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='workspace').at(-1).open),{page:'answer',operation:null,root:project.root,board:project.issues,task:1,revision:9});
  assert.equal(await page.locator('[data-card]').count(),1,'question appears once');
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>!['workspace','size','ready','web-observation'].includes(m.a)&&!(m.a==='set'&&m.navigation))),[],'panels/navigation apply no policy or retry');
  console.log('PASS R6/R12 TW3 G05/G06: panels before complete tasks, one identity/Board, no empty Attention, urgent faults, unknown reason, W launch, wrapped title; dark/light 16/20 px at 320 px');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
