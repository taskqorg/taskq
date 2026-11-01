// Chain: #1928 A1–A8 / M3 / MP13 / UX30-07..10. Risk: floating details, wrong task destinations,
// lost operation fences, history or reading context. Invented snapshots; no board/native side effects.
const assert=require('node:assert/strict'),fs=require('node:fs'),{chromium}=require('playwright');
const states=['doing','review','ask','failed','ready','waiting','later','held'],controls=require('./fixtures/task-controls.json').states;
const board='https://forge.example/team/project/-/issues',root='/fixture/alpha',out=process.env.TASK_NAV_EVIDENCE;
const tasks=states.map((state,i)=>({n:i+1,state,title:'Preserve task '+(i+1),rank:i,revision:100+i,attempt:200+i,role:'worker',runtime:'codex',host:'fixture',local_session:true,session:'codex:invented-'+i+' fixture',age:'3m',state_name:state,controls:controls[state],native_session_url:'codex://threads/invented-'+i}));
tasks[1]={...tasks[1],enqueued:'invented-queue-receipt',state_name:'merge queue · CI',publication:{reason:'CI: Linux passed; macOS pending'}};
tasks[2]={...tasks[2],card:'Current problem without repeated identity',question:{parsed:true,done:'Compared fixture clients',text:'Current problem without repeated identity',evidence:[],limits:{chars:240},problem:'Current problem without repeated identity',choices:[{id:'3',text:'Keep all context',label:'Keep all context',recommended:true}]},history:[{question:{parsed:true,problem:'Historical question: preserve this exact wording',choices:[{id:'7',label:'Earlier choice'}]},answered:{by:'fixture owner',text:'Earlier answer',revision:80,option:'7'}}]};
const snapshot={...require('./fixtures/display-model.json'),platform:'macos',font:16,theme:'dark',host:'fixture',now:1000,palette:'--work:light-dark(#087f5b,#38e8a0);--ask:light-dark(#995200,#ffc247);--failed:light-dark(#c92641,#ff6378);--review:light-dark(#8250df,#bca3eb)',projects:[{root,name:'Alpha',board_kind:'gitlab',issues:board,alive:true,complete:true,age:1,tasks,focus:['all'],labels:[]}],ai:{draft:'',history:[]},notify:{projects:{}},conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 const open=async(file,s,viewport={width:360,height:560})=>{const p=await browser.newPage({viewport,colorScheme:s.theme});p.on('pageerror',e=>{throw e;});await p.setContent(fs.readFileSync('build/web/'+file+'.html','utf8'));await p.evaluate(()=>{window.sent=[];window.ipc={postMessage:m=>sent.push(JSON.parse(m))};});await p.evaluate(s=>taskq.receive(s),s);return p;};
 try{
  if(out)fs.mkdirSync(out,{recursive:true});
  // Eight states × 2 widths × 2 themes × 2 fonts: all disclosures, direct targets and retained operation payloads.
  for(const width of [320,360])for(const theme of ['light','dark'])for(const font of [16,20]){
   const page=await open('tray',{...snapshot,theme,font},{width,height:560});
   for(const t of tasks){
    const row=page.locator('[data-card="'+board+'/'+t.n+'"]'),trigger=row.locator('[data-a=task-detail]');
    await trigger.hover();await trigger.focus();assert.equal(await trigger.getAttribute('aria-expanded'),'false','hover/focus never opens');
    await page.keyboard.press(t.n%2?'Enter':'Space');
    assert.equal(await page.locator('#taskq-popover').count(),0,t.state+': task inspection never floats');
    const panel=row.locator('.task-inline-details');assert.equal(await panel.count(),1,t.state+': inline panel');
    const expectedEdge=({doing:['rgb(8, 127, 91)','rgb(56, 232, 160)'],review:['rgb(130, 80, 223)','rgb(188, 163, 235)'],ask:['rgb(153, 82, 0)','rgb(255, 194, 71)'],failed:['rgb(201, 38, 65)','rgb(255, 99, 120)']})[t.state]||['rgb(89, 99, 110)','rgb(185, 195, 205)'];assert.equal(await panel.evaluate(e=>getComputedStyle(e).borderLeftColor),expectedEdge[theme==='dark'?1:0],t.state+': restrained state edge');assert.equal(await panel.evaluate(e=>getComputedStyle(e).color),theme==='dark'?'rgb(238, 242, 245)':'rgb(36, 48, 57)','only the edge/actions carry the state accent');
    assert.equal(await panel.locator('.detail-title,.tqp-age,.task-stage,h2,strong').count(),0,'parent identity/state/age is not repeated');
    await page.locator('h1').click();assert.equal(await panel.count(),1,'outside click retains inline context');
    assert.equal(await row.locator('.task-board').getAttribute('data-url'),board+'/'+t.n,'exact self-hosted GitLab forge target');
    assert.equal(await row.locator('.tqp-task-title a').getAttribute('data-url'),board+'/'+t.n);
    assert.equal(await panel.locator('[title="Close #'+t.n+' on the board"] svg title').textContent(),'GitLab','Close uses recorded provider identity');
    assert.equal(await panel.locator('[data-destination=native]').getAttribute('data-url'),t.native_session_url,'native target remains explicit');
    if(t.enqueued){assert.match(await row.locator('.task-publication').innerText(),/merge queue · CI/);assert.match(await panel.innerText(),/Linux passed; macOS pending/);}
    if(t.state==='ask')assert.equal(await row.locator('.question-shortcut').count(),1,'direct one-click question entry retained');
    const expected={page:'answer',operation:null,root,board,task:t.n,revision:t.revision};
    await row.locator('.tqp-mark').click();assert.deepEqual(await page.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open),expected,'runtime goes to the exact TaskQ thread');
    assert.equal(await panel.count(),1,'agent navigation keeps the menu context');
    if(width===360&&theme==='dark'&&font===16){
     // The same canonical operation renderer/dispatcher still owns every state action and revision fence.
     const offered=await panel.locator('[data-a=control]').evaluateAll(es=>es.map(e=>e.dataset.control));
     assert.deepEqual(offered,t.controls.filter(a=>a!=='start'),t.state+': operation parity');
     for(const action of t.controls){
      await panel.locator(action==='start'?'[data-a=start]':'[data-control='+action+']').click();
      const m=await page.evaluate(()=>sent.filter(m=>m.a==='answer').at(-1));
      assert.equal(m.board,board);assert.equal(m.root,root);assert.deepEqual(m.op,{id:'tray.'+action+'.'+t.n+'.'+t.revision,task:t.n,revision:t.revision,action});
      await page.evaluate(m=>taskq.receive({answerReceipt:{root:m.root,id:m.op.id,state:'rejected',reason:'Invented refusal; no board write'}}),m);
      const dismiss=page.locator('[data-a=answer-dismiss]');if(await dismiss.count())await dismiss.first().click();
     }
    }
    assert.deepEqual(await panel.evaluate(e=>[e,...e.querySelectorAll('*')].filter(n=>['auto','scroll'].includes(getComputedStyle(n).overflowY)&&n.scrollHeight>n.clientHeight).map(n=>n.className)),[],'no nested scroll authority');
    assert(await page.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth+1),'narrow/theme/font reflows');
    if(out&&width===360&&theme==='dark'&&font===16&&t.state==='review'){await trigger.scrollIntoViewIfNeeded();await page.screenshot({path:out+'/inline-review.png'});}
    await panel.getByRole('button',{name:'Close task details',exact:true}).click();assert(await trigger.evaluate(e=>e===document.activeElement),'Close returns focus');
    await trigger.click();await page.keyboard.press('Escape');assert(await trigger.evaluate(e=>e===document.activeElement),'Escape returns focus');
   }
   assert.equal(await page.evaluate(()=>sent.filter(m=>['conversation-change','message-action','ai-send'].includes(m.a)).length),0,'opening and reading never sends or marks read');
   await page.close();
  }
  console.log('PASS #1928 64 inline cells, eight-state payload parity, forge/runtime/question routing, focus and outside-click retention');
  // Real wheel input over text, controls and both panel edges must always move the primary list.
  for(const t of tasks){
   const middle={...t,card:'Missing context. '.repeat(120),blocked:Array.from({length:4},(_,i)=>({ref:'#'+(90+i),url:board+'/'+(90+i),reason:'Recorded blocker context'}))};
   const padding=(offset)=>Array.from({length:12},(_,i)=>({...tasks[4],n:offset+i,rank:offset+i,title:'Surrounding task '+i}));
   const ps=[...padding(100),middle,...padding(120)].map((r,rank)=>({...r,rank}));
   const wheel=await open('tray',{...snapshot,projects:[{...snapshot.projects[0],tasks:ps}]});
   await wheel.locator('[data-a=task-detail][data-key="'+board+'/'+t.n+'"]').click();
   const panel=wheel.locator('.task-inline-details');
   for(const target of [panel.locator('.popover-heading'),panel.locator('.task-card-text'),panel.locator('.row-actions'),panel]){
    for(const edge of target===panel?['top','bottom']:['center']){
     await target.evaluate((e,edge)=>{const c=document.querySelector('.content'),r=e.getBoundingClientRect();c.scrollTop+=(edge==='bottom'?r.bottom:r.top)-c.getBoundingClientRect().top-180;},edge);
     const b=await target.boundingBox();await wheel.mouse.move(b.x+b.width/2,edge==='bottom'?b.y+b.height-1:edge==='top'?b.y+1:b.y+Math.min(16,b.height/2));
     for(const delta of [12,90,-12,-90]){const before=await wheel.locator('.content').evaluate(e=>e.scrollTop);await wheel.mouse.wheel(0,delta);await wheel.waitForFunction(({before,delta})=>(document.querySelector('.content').scrollTop-before)*Math.sign(delta)>1,{before,delta},{timeout:2000});}
    }
   }
   assert.equal(await wheel.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='workspace').length),0,'wheel never triggers an operation');await wheel.close();
  }
  console.log('PASS #1928 eight-state wheel/trackpad deltas over text, controls and panel edges');
  // RC1/MP14: close-pin opening preserves exact scope while retiring the independent menu reader.
  // Full transcript/cursor/outcome/PR/return protection is in conversations-centre and closed-pins instruments.
  const ref='gitlab://forge.example/team/project#40',pin={ref,title:'Closed task',url:board+'/40'},s={...snapshot,navigation:{schema:1,dashboard:{pins:[pin]}}},page=await open('tray',s);
  await page.waitForFunction(()=>sent.some(m=>m.a==='completed'&&m.task===40));const read=await page.evaluate(()=>sent.find(m=>m.a==='completed'&&m.task===40));
  await page.evaluate(read=>taskq.receive({completed:{...read,tasks:[{n:40,state:'closed',url:read.root+'/40',title:'Closed task',closed:990,outcome:'success'}]}}),read);
  await page.locator('[data-a=pin-transcript]').click();assert.equal(await page.locator('.task-pins').count(),0,'first open retains the closed-pin policy');assert.deepEqual(await page.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open),{page:'conversations',root,board,task:40});assert.equal(await page.locator('.task-transcript-page').count(),0);await page.close();
  // Task activity is a read-only route to the same exact thread, including archived Q&A and independent owner drafts.
  const owner={id:'independent-draft',title:'Independent draft',root,text:'Keep this unsent draft',caret:[2,4],files:[],revision:1},workspace=await open('workspace',{...snapshot,conversations:{complete:true,items:[owner]}},{width:940,height:740});
  await workspace.evaluate(()=>taskq.receive({open:{page:'sessions'}}));
  const activity=workspace.locator('.conversation-row[data-thread="task:'+board+'/3"]');assert.equal(await workspace.locator('[data-c=result-filter][data-filter=activity]').getAttribute('aria-pressed'),'true');
  await activity.click();assert.equal(await workspace.locator('.thread-title').innerText(),'Alpha · #3 Preserve task 3');assert.match(await workspace.locator('.conversation-info').textContent(),/attempt 202.*worker.*fixture/s);
  assert.match(await workspace.locator('.conversation-reader').innerText(),/Historical question: preserve this exact wording[\s\S]*Earlier choice[\s\S]*Earlier answer[\s\S]*Current problem/);
  await workspace.locator('[data-c-field=answer]').fill('Retain task answer draft');
  await workspace.locator('.conversation-row[data-thread="task:'+board+'/4"]').click();await activity.click();assert.equal(await workspace.locator('[data-c-field=answer]').inputValue(),'Retain task answer draft');
  await workspace.locator('[data-a=back]').click();assert.equal(await workspace.locator('h1').innerText(),'Conversations','Back restores the invoking common view');
  await workspace.locator('[data-c=select][data-id=independent-draft]').click();assert.equal(await workspace.locator('[data-c-field=text]').inputValue(),owner.text,'independent owner draft survives navigation');
  assert.equal(await workspace.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='message-action'||m.a==='conversation-change'&&['send','reply','create'].includes(m.action)).length),0,'reading never creates, sends or marks read');
  await workspace.close();console.log('PASS #1928 closed reading/auto-unpin, history/PR/outcome, exact task activity and draft/Back preservation');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
