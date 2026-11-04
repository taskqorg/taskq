// Chain: #1928 A1–A8 / M3 / MP13 / UX30-07..10. Risk: competing readers, wrong task destinations,
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
  // F13/F15: the same eight states open exact Conversations objects. Task operations live in its sticky head.
  for(const width of [320,360])for(const theme of ['light','dark'])for(const font of [16,20]){
   const page=await open('tray',{...snapshot,theme,font},{width,height:560});
   const workspace=await open('workspace',{...snapshot,theme,font},{width,height:740});
   for(const t of tasks){
    const row=page.locator('[data-card="'+board+'/'+t.n+'"]'),trigger=row.locator('.task-target');
    const before=await page.evaluate(()=>sent.filter(m=>m.a==='workspace').length);
    await trigger.hover();await trigger.focus();assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='workspace').length),before,'hover/focus never opens');
    assert.equal(await row.locator('button,a,input,[tabindex]').count(),2,'row navigation and Copy are independent keyboard targets');
    await page.keyboard.press(t.n%2?'Enter':'Space');
    assert.equal(await page.locator('#taskq-popover,.task-inline-details').count(),0,t.state+': no competing inline reader');
    const destination=await page.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open);
    assert.deepEqual(destination,{page:'conversations',thread:'task:'+board+'/'+t.n,operation:null,root,board,task:t.n,revision:t.revision,...(t.state==='ask'?{target:{kind:'question',revision:t.revision}}:{})},'exact board/task/revision and question target');
    await workspace.evaluate(open=>taskq.receive({open}),destination);
    const head=workspace.locator('.sticky-object-head'),metadata=workspace.locator('.object-metadata');
    assert.equal((await head.locator('h2').innerText()).replace(/\s+/g,' '),'Alpha#'+t.n+' '+t.title);
    assert.equal(await head.locator('[data-url="'+board+'/'+t.n+'"]').getAttribute('href'),board+'/'+t.n,'exact self-hosted GitLab forge target');
    assert.equal(await head.locator('[data-url="'+board+'/'+t.n+'"] svg title').textContent(),'GitLab','board action uses recorded provider identity');
    assert.equal(await metadata.locator('[data-url="'+t.native_session_url+'"]').getAttribute('href'),t.native_session_url,'native target remains explicit');
    assert.match(await metadata.innerText(),new RegExp('Attempt\\s*'+t.attempt+'[\\s\\S]*Revision\\s*'+t.revision));
    if(t.enqueued)assert.match(await metadata.innerText(),/Linux passed; macOS pending/,'publication reason remains visible');
    const offered=await head.locator('[data-a=control]').evaluateAll(es=>es.map(e=>e.dataset.control));assert.deepEqual(offered,t.controls,t.state+': operation parity');
    if(width===360&&theme==='dark'&&font===16){
     for(const action of t.controls){
      await head.locator('[data-control='+action+']').click();
      const m=await workspace.evaluate(()=>sent.filter(m=>m.a==='answer').at(-1));
      assert.equal(m.board,board);assert.equal(m.root,root);assert.deepEqual(m.op,{id:'conversation.'+action+'.'+t.n+'.'+t.revision,task:t.n,revision:t.revision,action});
      await workspace.evaluate(m=>taskq.receive({answerReceipt:{root:m.root,id:m.op.id,state:'rejected',reason:'Invented refusal; no board write'}}),m);
      const dismiss=workspace.locator('[data-a=answer-dismiss]');if(await dismiss.count())await dismiss.first().click();
     }
     await head.getByRole('button',{name:'Pin task',exact:true}).click();assert.deepEqual(await workspace.evaluate(()=>(({a,root,task,pinned})=>({a,root,task,pinned}))(sent.filter(m=>m.a==='task-pin').at(-1))),{a:'task-pin',root,task:t.n,pinned:true});
     await workspace.evaluate(t=>taskq.receive({dashboard:{pins:[{ref:'gitlab://forge.example/team/project#'+t.n,title:t.title,url:'https://forge.example/team/project/-/issues/'+t.n}]}}),t);
     await head.getByRole('button',{name:'Unpin task',exact:true}).click();assert.deepEqual(await workspace.evaluate(()=>(({a,root,task,pinned})=>({a,root,task,pinned}))(sent.filter(m=>m.a==='task-pin').at(-1))),{a:'task-pin',root,task:t.n,pinned:false});
     await workspace.evaluate(()=>taskq.receive({dashboard:{pins:[]}}));
    }
    assert.deepEqual(await metadata.evaluate(e=>[e,...e.querySelectorAll('*')].filter(n=>['auto','scroll'].includes(getComputedStyle(n).overflowY)&&n.scrollHeight>n.clientHeight).map(n=>n.className)),[],'metadata has no nested scroll authority');
    assert(await page.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth+1),'narrow menu reflows '+JSON.stringify({width,theme,font,state:t.state,size:await page.locator('.content').evaluate(e=>[e.clientWidth,e.scrollWidth])}));
    assert(await workspace.locator('.conversation-reader').evaluate(e=>e.scrollWidth<=e.clientWidth+1),'narrow reader reflows');
    if(out&&width===360&&theme==='dark'&&font===16&&t.state==='review')await workspace.screenshot({path:out+'/conversation-review.png'});
   }
   assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','conversation-change','message-action','ai-send'].includes(m.a)).length),0,'menu navigation never sends or marks read');
   await page.close();await workspace.close();
  }
  console.log('PASS #2343 eight-state whole-row routes, Conversations operation/revision parity, forge/native targets, pin IPC and narrow metadata');
  // Real wheel input over full task metadata must move the sole reader scroll without moving its sticky header.
  for(const t of tasks){
   const middle={...t,brief:'Missing context. '.repeat(120),blocked:Array.from({length:30},(_,i)=>({ref:'#'+(90+i),url:board+'/'+(90+i),reason:'Recorded blocker context'}))};
   const wheel=await open('workspace',{...snapshot,projects:[{...snapshot.projects[0],tasks:[middle]}]},{width:360,height:740});
   await wheel.evaluate(open=>taskq.receive({open}),{page:'conversations',root,board,task:t.n});
   const panel=wheel.locator('.object-metadata'),reader=wheel.locator('.conversation-reader'),head=wheel.locator('.sticky-object-head'),headTop=(await head.boundingBox()).y;
   await reader.evaluate(e=>e.scrollTop=150);
   for(const delta of [12,90,-12,-90]){
    const r=await reader.boundingBox();await wheel.mouse.move(r.x+r.width/2,r.y+r.height/2);
    const before=await reader.evaluate(e=>e.scrollTop);await wheel.mouse.wheel(0,delta);
    await wheel.waitForFunction(({before,delta})=>(document.querySelector('.conversation-reader').scrollTop-before)*Math.sign(delta)>1,{before,delta},{timeout:2000});
    assert.equal((await head.boundingBox()).y,headTop,'compact head remains stationary');
   }
   assert.deepEqual(await panel.evaluate(e=>[e,...e.querySelectorAll('*')].filter(n=>['auto','scroll'].includes(getComputedStyle(n).overflowY)&&n.scrollHeight>n.clientHeight).map(n=>n.className)),[]);
   assert.equal(await wheel.evaluate(()=>sent.filter(m=>m.a==='answer').length),0,'wheel never triggers an operation');await wheel.close();
  }
  console.log('PASS #2343 eight-state real wheel/small deltas move metadata under the sticky head');
  // RC1/MP14: close-pin opening preserves exact scope while retiring the independent menu reader.
  // Full transcript/cursor/outcome/PR/return protection is in conversations-centre and closed-pins instruments.
  const ref='gitlab://forge.example/team/project#40',pin={ref,title:'Closed task',url:board+'/40'},s={...snapshot,navigation:{schema:1,dashboard:{pins:[pin]}}},page=await open('tray',s);
  await page.waitForFunction(()=>sent.some(m=>m.a==='completed'&&m.task===40));const read=await page.evaluate(()=>sent.find(m=>m.a==='completed'&&m.task===40));
  await page.evaluate(read=>taskq.receive({completed:{...read,tasks:[{n:40,state:'closed',url:read.root+'/40',title:'Closed task',closed:990,outcome:'success'}]}}),read);
  await page.locator('.task-pins .task-target').click();await page.waitForFunction(()=>!document.querySelector('.task-pins'));assert.equal(await page.locator('.task-pins').count(),0,'first open retains the closed-pin policy');assert.deepEqual(await page.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open),{page:'conversations',thread:'task:'+board+'/40',operation:null,root,board,task:40,revision:null});assert.equal(await page.locator('.task-transcript-page').count(),0);await page.close();
  // Task activity is a read-only route to the same exact thread, including archived Q&A and independent owner drafts.
  const owner={id:'independent-draft',title:'Independent draft',root,text:'Keep this unsent draft',caret:[2,4],files:[],revision:1},workspace=await open('workspace',{...snapshot,conversations:{complete:true,items:[owner]}},{width:940,height:740});
  await workspace.evaluate(()=>taskq.receive({open:{page:'sessions'}}));
  const activity=workspace.locator('.conversation-row[data-thread="task:'+board+'/3"]');assert.match(await workspace.locator('.conversation-view summary').innerText(),/In progress/);
  await activity.click();assert.equal(((await workspace.locator('.thread-title').innerText()).replace(/\s+/g,' ')).replace(/\s+/g,' '),'Alpha#3 Preserve task 3');assert.match(await workspace.locator('.object-metadata').textContent(),/fixture.*worker.*202/s);
  assert.match(await workspace.locator('.conversation-reader').innerText(),/Historical question: preserve this exact wording[\s\S]*Earlier choice[\s\S]*Earlier answer[\s\S]*Current problem/);
  await workspace.locator('[data-c-field=answer]').fill('Retain task answer draft');
  await workspace.locator('.conversation-row[data-thread="task:'+board+'/4"]').click();await activity.click();assert.equal(await workspace.locator('[data-c-field=answer]').inputValue(),'Retain task answer draft');
  await workspace.locator('[data-a=back]').click();assert.equal(await workspace.locator('h1').innerText(),'Conversations','Back restores the invoking common view');
  assert.match(await workspace.locator('.conversation-view summary').innerText(),/In progress/,'Back preserves the invoking section');
  await workspace.locator('.conversation-view summary').click();await workspace.locator('[data-c=result-filter][data-filter=all]').click();
  await workspace.locator('[data-c=select][data-id=independent-draft]').click();assert.equal(await workspace.locator('[data-c-field=text]').inputValue(),owner.text,'independent owner draft survives navigation');
  assert.equal(await workspace.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='message-action'||m.a==='conversation-change'&&['send','reply','create'].includes(m.action)).length),0,'reading never creates, sends or marks read');
  await workspace.close();console.log('PASS #1928 closed reading/auto-unpin, history/PR/outcome, exact task activity and draft/Back preservation');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
