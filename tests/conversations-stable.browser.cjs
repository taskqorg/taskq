// #1741 Q-A2/Q-A7/Q-A8/Q-A11: actual bundle, invented native reads and receipts; no board/native effects.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const board='https://example.org/alpha/issues',root='fixture',fx=require('./fixtures/answer-card.json');
const question=structuredClone(fx.cases.find(c=>c.name==='choice').rows[0]);
question.question.problem='Retained context and alternatives. '.repeat(200);
const owner=id=>({id,title:id,revision:1,root,text:'Independent draft',caret:[2,4],scroll:0,files:[],archived:false,sent:{text:'Original',runtime:'claude',files:[]},request:{state:'launched',runtime:'claude',id}});
const state={font:20,theme:'dark',host:'fixture',projects:[{root,name:'Alpha',issues:board,alive:true,complete:true,tasks:[question,{...question,n:question.n+1}]}],ai:{history:[],runtime:{id:'claude',available:true}},conversations:{complete:true,items:[owner('owner-a'),owner('owner-b')],navigation:{selected:'owner-a',list:'recent'}},messages:{complete:true,entries:[]}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());const failures=[];try{
 const page=await browser.newPage({viewport:{width:940,height:740}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};});
 await page.evaluate(s=>taskq.receive(s),state);
 const open=async n=>page.evaluate(({board,n})=>taskq.receive({open:{page:'conversations',board,task:n}}),{board,n});
 async function check(name,run){try{await run();console.log('PASS '+name);}catch(e){failures.push(name+': '+e.message);console.error('FAIL '+name+': '+e.message);}}
 await check('Q-A2 fixed heading/Send with growing draft at 350/620/940',async()=>{
  await open(question.n);
  for(const width of [940,620,350]){
   await page.setViewportSize({width,height:740});
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   const reader=page.locator('.conversation-reader').or(page.locator('.conversation-detail').filter({hasNot:page.locator('.conversation-reader')}));
   const heading=await page.locator('.thread-head').boundingBox();
   for(const fraction of [0,.5,1]){
    await reader.evaluate((e,f)=>{e.scrollTop=(e.scrollHeight-e.clientHeight)*f;e.dispatchEvent(new Event('scroll'));},fraction);
    const send=await page.locator('[data-a=answer-send]').boundingBox(),head=await page.locator('.thread-head').boundingBox();
    assert(send.y>=0&&send.y+send.height<=740,'Send visible without scrollIntoView at '+width+' / '+fraction);
    assert.equal(head.y,heading.y,'thread heading stays stable');
    if(width===940){const field=await page.locator('[data-c-field=answer]').boundingBox();assert(field.y>=head.y+head.height&&field.y+field.height<=send.y,'short input stays at the bottom while reading');}
   }
   await page.locator('[data-c-field=answer]').fill('Long draft line.\n'.repeat(100));
   const geometry=await page.locator('[data-c-field=answer]').evaluate(e=>({client:e.clientHeight,scroll:e.scrollHeight,resize:getComputedStyle(e).resize}));
   assert(geometry.client>=geometry.scroll-1,'natural growth has no inner scroll');assert.equal(geometry.resize,'none');
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'no horizontal overflow');
   await page.locator('[data-c-field=answer]').fill('');
  }
 });
 await page.setViewportSize({width:940,height:740});
 await check('Q-A7 same-board late receipt stays on its task, one status',async()=>{
  await open(question.n);await page.locator('[data-qac=choose]').first().click();
  await page.waitForFunction(()=>sent.some(m=>m.a==='answer'));
  const op=await page.evaluate(()=>sent.find(m=>m.a==='answer').op);
  await open(question.n+1);
  await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:op.id,state:'accepted',reason:'Unique receipt for A'});
  state.projects[0].tasks[0]={...question,state:'doing',revision:question.revision+1};await page.evaluate(s=>taskq.receive(s),state);
  assert.doesNotMatch(await page.locator('.menu').textContent(),/Unique receipt for A|answer accepted/,'B cannot show A receipt or toast');
  await open(question.n);
  assert.match(await page.locator('.conversation-detail').innerText(),/Unique receipt for A/);
  assert.equal(await page.locator('.conversation-detail [role=status]').filter({hasText:'Unique receipt for A'}).count(),1);
  assert.equal(await page.locator('.toast').count(),0,'workspace has no duplicate receipt overlay');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer').length),1,'navigation and receipts never resend');
 });
 await check('Q-A11 late and failed reads preserve the selected owner draft',async()=>{
  await page.locator('[data-c=select][data-id=owner-a]').click();
  const reads=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.conversation==='owner-a'));
  await page.locator('[data-c-field=text]').fill('Owner A draft');
  await page.locator('[data-c=select][data-id=owner-b]').click();await page.locator('[data-c-field=text]').fill('Owner B draft');
  for(const m of reads)await page.evaluate(m=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,state:'accepted',value:m.action==='status'?{state:'idle',watch:'no watch'}:{items:[{role:'assistant',text:'Foreign transcript A'}],start:0,end:1}}}),m);
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Owner B draft');assert.doesNotMatch(await page.locator('.conversation-detail').innerText(),/Foreign transcript A/);
  const m=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.conversation==='owner-b'&&m.action==='transcript').at(-1));
  await page.evaluate(m=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,state:'unknown',reason:'Invented read failure'}}),m);
  assert.match(await page.locator('.dialogue-transcript').innerText(),/Invented read failure/);
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Owner B draft');
  assert.equal(await page.evaluate(()=>sent.filter(m=>['reply','send'].includes(m.action)).length),0,'read recovery never sends');
 });
 await check('Q-A8 owner refusal stays with its operation across a switch',async()=>{
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(s=>{window.sent=[];window.fixture=s;window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-action'&&m.action==='save'){const c=fixture.conversations.items.find(c=>c.id===m.conversation);Object.assign(c,m.draft);c.revision++;queueMicrotask(()=>taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:fixture.conversations}}));}}};taskq.receive(s);},state);
  await page.locator('[data-c=select][data-id=owner-a]').click();
  await page.locator('[data-c=reply]').click();await page.waitForFunction(()=>sent.some(m=>m.action==='reply'));
  const op=await page.evaluate(()=>sent.find(m=>m.action==='reply'));
  await page.locator('[data-c=select][data-id=owner-b]').click();
  await page.evaluate(id=>taskq.receive({conversationReceipt:{id,state:'unknown',reason:'open in claude; close it there first'}}),op.id);
  assert.doesNotMatch(await page.locator('.conversation-status').innerText(),/open in claude|Not sent/);
  await page.locator('[data-c=select][data-id=owner-a]').click();
  assert.match(await page.locator('.conversation-status').innerText(),/Not sent: open in claude/);
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Independent draft');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='reply').length),1);
  await page.locator('[data-c=reply]').click();
  assert.doesNotMatch(await page.locator('.conversation-status').innerText(),/Not sent|open in claude/,'a new explicit submission replaces the old operation status');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='reply').length),2);
 });
 // Q-A8/Q-A11: operations that move selection must retain feedback and recovery without replay.
 async function bootOwners(){
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(s=>{window.sent=[];window.fixture=s;window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-action'&&m.action==='save'){const c=fixture.conversations.items.find(c=>c.id===m.conversation);Object.assign(c,m.draft);c.revision++;queueMicrotask(()=>taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:fixture.conversations}}));}}};taskq.receive(s);},state);
 }
 for(const action of ['archive','delete','fork'])await check('Q-A8 '+action+' completion follows resulting selection',async()=>{
  await bootOwners();
  if(action==='delete'){
   await page.locator('[data-c=archive]').click();
   await page.evaluate(()=>{const m=sent.find(m=>m.action==='archive');fixture.conversations.items[0].archived=true;taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:fixture.conversations}});});
   await page.evaluate(()=>{fixture.conversations.items[1].archived=true;taskq.receive(fixture);});
   await page.locator('[data-c=archive-list]').click();await page.locator('[data-c=select][data-id=owner-a]').click();
   await page.locator('[data-c=delete-review]').click();await page.locator('[data-c=delete]').click();
  }else await page.locator('[data-c='+ (action==='fork'?'from-draft':action)+']').click();
  await page.waitForFunction(action=>sent.some(m=>m.action===action),action);
  await page.evaluate(action=>{
   const m=sent.find(m=>m.action===action),c=fixture.conversations.items[0];let value;
   if(action==='archive')c.archived=true;
   if(action==='delete')fixture.conversations.items.shift();
   if(action==='fork'){fixture.conversations.items.push({...c,id:m.fork_id,request:null,sent:null});value={forked:m.fork_id};}
   taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:fixture.conversations,value}});
  },action);
  assert.match(await page.locator('.conversation-status').innerText(),new RegExp({archive:'Archived',delete:'Permanently deleted',fork:'Draft and files moved'}[action]));
  assert.equal(await page.evaluate(()=>sent.filter(m=>['send','reply'].includes(m.action)).length),0,'navigation operations never send');
 });
 await check('Q-A11 foreign save failure names its source and opens recovery',async()=>{
  await bootOwners();
  await page.locator('[data-c=archive]').click();await page.waitForFunction(()=>sent.some(m=>m.action==='archive'));
  const op=await page.evaluate(()=>sent.find(m=>m.action==='archive'));
  await page.locator('[data-c=select][data-id=owner-b]').click();
  await page.evaluate(id=>taskq.receive({conversationReceipt:{id,state:'unknown',reason:'Invented missing archive receipt'}}),op.id);
  assert.match(await page.locator('.conversation-status').innerText(),/owner-a/,'blocked queue identifies the failed thread');
  for(const width of [350,620,940]){
   await page.setViewportSize({width,height:740});
   const box=await page.locator('[data-c=reply-open]').boundingBox();
   assert(box.x>=0&&box.x+box.width<=width&&box.y>=0&&box.y+box.height<=740,'recovery stays reachable at '+width);
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'recovery has no horizontal overflow');
  }
  if(process.env.TASKQ_EVIDENCE_1741){fs.mkdirSync(process.env.TASKQ_EVIDENCE_1741,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_1741+'/cross-thread-recovery.png'});}
  await page.locator('[data-c=reply-open]').click();
  assert.equal(await page.locator('.conversation-detail').getAttribute('data-thread-id'),'owner-a');
  assert.match(await page.locator('.conversation-status').innerText(),/Invented missing archive receipt/);
  assert(await page.locator('[data-c=retry-save]').isVisible());
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Independent draft');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='archive').length),1,'opening recovery never retries');
  assert.equal(await page.evaluate(()=>sent.filter(m=>['send','reply'].includes(m.action)).length),0,'opening recovery never sends');
 });
 assert.deepEqual(errors,[]);assert.deepEqual(failures,[]);
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
