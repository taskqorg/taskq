// #1680 actual bundle, invented values in the Mac install's persisted shape: one launched request, idle dialogue, list-only
// navigation, saved navigation on Work mode Recovery. Defect class: Conversations opens as Interface error, a failing route
// hides Home and Close, and the native request status leaks onto every page.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const evidence=process.env.TASKQ_EVIDENCE_1680;
const request={id:'owner-request-1680',state:'launched',kind:'launched',next:'Reply in Conversations',reason:null,url:'https://claude.ai/code/session_fixture1680',host:'fixture-host',runtime:'claude',session:'fixture-session'};
const page_={page:'operations',operation:{operation:'recovery',root:null},project:null,parent:'messages',taskFilter:null,working:null,aiRoot:'fixture-alpha',
 answer:{board:'https://example.org/alpha/issues',task:7,revision:1,root:'fixture-alpha'},messageFilter:'all',message:null,scroll:0,open:{},focus:null,unavailable:null};
const snapshot={font:16,theme:'dark',host:'fixture-host',version:'0.0.0-fixture',projects:[{root:'fixture-alpha',name:'Alpha',alive:true,complete:true,issues:'https://example.org/alpha/issues',tasks:[],mode:'run',focus:['all']}],
 ai:{request,runtime:{id:'claude',name:'Claude Code',available:true,reason:''},root:'fixture-alpha',local:{text:'',caret:[0,0],op:null,receipt:null},error:null,draft:'',history:['Start']},
 messages:{entries:[{key:'request:'+request.id,kind:'request',title:'Start',root:'fixture-alpha',host:'fixture-host',id:request.id,state:'launched',created:1,request,pending:true,read:false,archived:false,deleting:false}],count:1,reason:'',complete:true},
 conversations:{items:[{revision:3,title:'',root:'fixture-alpha',text:'',caret:[0,0],scroll:0,archived:false,deleting:false,next:false,parent:null,files:[],
  sent:{revision:2,text:'Start',root:'fixture-alpha',runtime:'claude',files:[]},id:request.id,request,dialogue:{turn:null,watch:'no watch',state:'idle'}}],
  navigation:{list:'recent',search:'',selection:{recent:request.id,archive:null},selected:request.id,listOnly:true,scrolls:{recent:0}},complete:true,reason:'',
  limits:{files:8,file_bytes:1048576,draft_bytes:65536,store_bytes:8388608,pdf_pages:20}},
 workspace_navigation:{schema:1,current:page_,trail:[{...page_,page:'messages',operation:null,open:{'conversation-history':true},focus:{tag:'button',key:'home',index:0}}]},
 workspace_local:{}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:900,height:720}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync(process.env.TASKQ_CONVERSATION_HTML||'build/web/workspace.html','utf8'));
  await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='conversation-dialogue')taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:m.action==='status'?{turn:null,watch:'no watch',last_line:'Done',state:'idle'}:{items:[{role:'user',text:'Start'},{role:'assistant',text:'Done'}],start:0,end:2}}});}};});
  const status=()=>page.locator('[data-status=workspace]').innerText(),fence=()=>page.getByText('Interface error').count();
  // The tray's own order on open: state, open, window chrome, owner.
  await page.evaluate(s=>{taskq.receive(s);taskq.receive({open:{page:'conversations'}});taskq.receive({workspacePin:false,workspaceMaximized:false});taskq.receive({workspaceOwner:{id:'main',detached:[]}});},snapshot);
  await page.waitForTimeout(300);
  // A1/A2: the list opens with its one request; the guard did not fire.
  assert.deepEqual(errors,[]);assert.equal(await fence(),0,'Conversations opened as Interface error');
  assert.equal(await page.locator('h1').innerText(),'Conversations');
  assert(await page.locator('.conversation-list').getByText('Start').count()>0,'the launched request is listed');
  if(evidence){fs.mkdirSync(evidence,{recursive:true});await page.screenshot({path:path.join(evidence,'conversations.png')});}
  // A4: request status stays on its conversation; Recovery and Inbox carry no global request notice.
  for(const open of [{page:'operations',operation:'recovery'},{page:'operations',operation:'resources'},{page:'messages'}]){
   await page.evaluate(o=>{taskq.receive({open:o});},open);await page.evaluate(s=>taskq.receive(s),snapshot);
   assert(!/Native request/.test(await status()),open.operation||open.page);assert(!/Native request: launched/.test(await page.locator('main').innerText()),open.operation||open.page);
  }
  // A3: a route that throws keeps Home, Back and Close and reports its class and site, not page-wide Interface error.
  await page.evaluate(s=>taskq.receive({...s,messages:{complete:true,count:1,entries:{}}}),snapshot);
  assert.equal(await fence(),0,'a route failure tripped the page-wide fence');
  assert.match(await page.locator('main [role=alert]').innerText(),/could not be shown/);
  for(const name of ['Workspace home','Close workspace'])assert(await page.getByRole('button',{name}).isVisible(),name);
  const report=await page.evaluate(()=>sent.filter(m=>m.a==='route-error'));
  assert.equal(report.length,1);assert.equal(report[0].route,'conversations');assert.equal(report[0].error,'TypeError');assert.match(report[0].site,/^[\w$.]+(:\d+:\d+)?$/);
  assert(!JSON.stringify(report).includes('example.org'),'report carries no content');
  await page.getByRole('button',{name:'Back',exact:true}).click();
  await page.getByRole('region',{name:'Conversations recovery'}).waitFor();
  assert(await page.getByRole('region',{name:'Conversations recovery'}).isVisible(),'Back must leave a failed data projection for an independent safe view');
  assert.match(await page.locator('main').innerText(),/previous reader is unavailable/);
  await page.evaluate(s=>taskq.receive({...s,messages:{complete:true,count:1,entries:{}}}),snapshot);
  assert(await page.getByRole('region',{name:'Conversations recovery'}).isVisible(),'automatic status cannot reopen the failed projection');
  if(evidence)await page.screenshot({path:path.join(evidence,'safe-view.png')});
  assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','ai-send','reports','message-action','conversation-action'].includes(m.a)).length),0,'safe Back must not write or replay');
  await page.evaluate(()=>taskq.receive({open:{page:'conversations',thread:'recovery'}}));
  assert(await page.getByRole('region',{name:'Conversations recovery'}).isVisible(),'native Safe view also avoids the failed selection callback');
  await page.locator('.shell-overflow summary').click();await page.locator('.shell-overflow [data-route=conversations]').click();
  assert.equal(await page.locator('h1').innerText(),'Conversations','safe navigation leaves the failing route');
  assert.deepEqual(errors,[]);
  // #1698 A1/A3: Settings opens each operation review while the tray forwards a Completed history page and an agentq
  // reply; neither is a status snapshot, so Work mode no longer fails at operationBody on a snapshot without projects.
  for(const operation of ['resources','service','setup','admission','recovery','stop','update']){
   await page.evaluate(s=>taskq.receive(s),snapshot);
   await page.evaluate(o=>{taskq.receive({open:{page:'operations',operation:o}});taskq.receive({completed:{items:[],complete:true}});taskq.receive({agentqError:null});},operation);
   assert.deepEqual(errors,[],operation);assert.equal(await fence(),0,operation+' opened as Interface error');
   assert.equal(await page.locator('h1').innerText(),'Operation review',operation);
   assert.equal(await page.locator('main [role=alert]').count(),0,operation+' route failed: '+JSON.stringify(await page.evaluate(()=>sent.filter(m=>m.a==='route-error'))));
  }
  if(evidence)await page.screenshot({path:path.join(evidence,'work-mode.png')});
  // #2110 R03/TW1: Settings review Back carries the origin and preserves the preceding Conversations context.
  await page.evaluate(()=>{taskq.receive({open:{page:'conversations'}});taskq.receive({open:{page:'operations',operation:'resources',origin:{surface:'settings',page:'work'}}});});
  await page.getByRole('button',{name:'Back',exact:true}).click();
  assert.deepEqual(await page.evaluate(()=>sent.filter(m=>m.a==='workspace-origin').at(-1).origin),{surface:'settings',page:'work'});
  assert.equal(await page.locator('h1').innerText(),'Conversations');
  // #1698 A2: invalid labels are reached only after the local choose-labels action; focus:null is now handled as unknown. A content failure inside Operation review keeps the shell; Back, Home and Escape still act.
  await page.evaluate(s=>{taskq.receive({...s,projects:[{...s.projects[0],mode:'watch',focus:null,saved:false,labels:null}]});taskq.receive({open:{page:'operations',operation:'admission'}});},snapshot);
  await page.locator('[data-a=pick]').click();
  assert.equal(await fence(),0,'an operation route failure tripped the page-wide fence');
  assert.match(await page.locator('main [role=alert]').innerText(),/could not be shown/);
  for(const name of ['Workspace home','Close workspace'])assert(await page.getByRole('button',{name}).isVisible(),name);
  if(evidence)await page.screenshot({path:path.join(evidence,'route-failure.png')});
  await page.getByRole('button',{name:'Workspace home'}).click();assert.equal(await page.locator('h1').innerText(),'Conversations','Home leaves the failing route');
  await page.keyboard.press('Escape');assert(await page.evaluate(()=>sent.some(m=>m.a==='hide')),'Escape closes the workspace');
  assert.deepEqual(errors,[]);
  // #2140: native Safe view is installed before snapshot, checkpoint and detached owner callbacks.
  const safe=await browser.newPage({viewport:{width:900,height:720}});
  try{
   await safe.evaluate(()=>{window.taskqNativeFrame=true;window.taskqSafeRecovery=true;});
   await safe.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   await safe.evaluate(s=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};taskq.receive({...s,messages:{complete:true,count:1,entries:{}}});taskq.receive({workspaceOwner:{id:'detached-fixture',conversation:'owner-request-1680',detached:[]}});},snapshot);
   assert(await safe.getByRole('region',{name:'Conversations recovery'}).isVisible(),'native safe boot cannot resolve the failed selected thread');
   assert.equal(await safe.getByText('Interface error',{exact:false}).count(),0,'safe owner restore must not trip the page guard');
   await safe.evaluate(()=>taskq.receive({workspaceRecoveryReady:true}));
   const handoff=await safe.evaluate(()=>sent.filter(m=>['workspace-recovered','workspace-checkpoint'].includes(m.a)));
   assert.equal(handoff.at(-2)?.a,'workspace-recovered','handoff acknowledges restoration before its checkpoint');
   assert.equal(handoff.at(-1)?.a,'workspace-checkpoint');
  }finally{await safe.close();}
  console.log('PASS persisted-shape Conversations open, scoped request status, failing route keeps Home and Close');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
