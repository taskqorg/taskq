// Chain: P-W3/TW2/C2/C7/D4 recovery -> same sources/drafts/receipts, no replay or acknowledgement.
// Risk: native-shaped lazy local worker source faults the whole reader; rebuilding loses unsaved edits or replays input.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const html=fs.readFileSync(process.env.TASKQ_CONVERSATION_HTML||'build/web/workspace.html','utf8');
const board='https://example.org/invented/issues',thread='task:'+board+'/112';
const draft={id:'invented-owner-2140',revision:1,root:'invented-root',title:'Independent draft',text:'Saved draft',caret:[2,4],scroll:0,files:[{id:'invented-file',name:'notes.md',mime:'text/markdown',bytes:2,hash:'0'.repeat(64)}]};
const browserTask=runtime=>({n:112,title:'Existing local worker',revision:71,state:'doing',role:'worker',attempt:71,runtime,host:'invented-host',local_session:true,body_lazy:true,body_source_revision:71,history_count:0,executions:[{attempt:71,state:'running'}]});
// A4/C2/C7: refused saves retain task edits and caret even when saved navigation mounts the field before recovery.
async function taskRecovery(browser,kind,savedNavigation=false){
 const page=await browser.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 try{
  const question=kind==='answer',task={...browserTask('codex'),body_lazy:false,...(question?{state:'ask',question:{text:'Problem: Choose a path',choices:[]}}:{})};
  const key=board+'#112',local={draft:'Old disk text',caret:[0,0],files:draft.files,projectRoot:'invented-root',steerAttempt:71};
  const state={font:20,theme:'dark',workspace_local:{[key]:local},projects:[{root:'invented-root',name:'Invented',issues:board,alive:true,complete:true,tasks:[task]}],ai:{history:[]},messages:{complete:true,entries:[]},conversations:{complete:true,items:[],navigation:{}}};
  async function mount(recovery){
   await page.goto('about:blank');await page.setContent(html);
   await page.evaluate(({state,recovery})=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='workspace-checkpoint')window.checkpoint=m.value;if(m.a==='answer-local')queueMicrotask(()=>taskq.receive({workspaceLocalError:'Invented disk write refused'}));}};taskq.receive(state);if(recovery)taskq.receive({workspaceRecovery:recovery});},{state,recovery});
  }
  await mount();await page.evaluate(board=>taskq.receive({open:{page:'conversations',board,task:112,root:'invented-root'}}),board);
  const field=page.locator('[data-c-field='+kind+']');await field.fill('Unsaved '+kind+' draft');
  await field.evaluate(e=>{e.focus();e.setSelectionRange(3,9);e.dispatchEvent(new Event('select',{bubbles:true}));});
  await page.getByText('Invented disk write refused',{exact:true}).waitFor();
  const recovery=await page.evaluate(()=>window.checkpoint);
  if(savedNavigation){
   state.workspace_navigation={schema:1,current:{page:'conversations',answer:{board,task:112,root:'invented-root'}},trail:[]};
   state.conversations.navigation={selected:thread,list:'recent',threads:{selected:thread}};
  }
  await mount(recovery);await field.waitFor();
  assert.equal(await field.inputValue(),'Unsaved '+kind+' draft','refused answer-local save must retain '+kind+' text');
  assert.deepEqual(await field.evaluate(e=>[e.selectionStart,e.selectionEnd]),[3,9],kind+' caret'+(savedNavigation?' with saved navigation':''));
  assert(await field.evaluate(e=>document.activeElement===e),kind+' focus');
  assert.match(await page.locator('.answer-composer').innerText(),/notes.md/);
  assert(await page.getByText('Invented disk write refused',{exact:true}).isVisible(),'save refusal remains explicit after recovery');
  assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','answer-local','conversation-action','reports','message-action'].includes(m.a)).length),0,'recovery is inert, including local saves');
  if(process.env.TASKQ_EVIDENCE_2140&&savedNavigation&&question){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2140,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_2140+'/recovered-answer-selection.png'});}
  const op={id:'invented-pending-'+kind,task:112,revision:71,text:'Immutable '+kind,files:draft.files,...(!question?{action:'steer'}:{})};
  recovery.answers[key]={...recovery.answers[key],op,receipt:{state:'sending'}};
  await mount(recovery);await field.waitFor();
  assert.equal(await field.inputValue(),'Unsaved '+kind+' draft');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer').length),0,'pending '+kind+' is never replayed');
  await page.getByRole('button',{name:'Check delivery',exact:true}).first().click();
  const request=await page.evaluate(()=>sent.find(m=>m.a==='answer'));
  assert.deepEqual([request.root,request.board,request.op],['invented-root',board,op],'explicit check retains exact root/board/operation');
  assert.deepEqual(errors,[]);
 }finally{await page.close();}
}
// A4: native ready may deliver recovery before a lazy catalog/body; retain it across another recreation.
async function loadingRecovery(browser){
 const page=await browser.newPage({viewport:{width:380,height:740}});try{
  const state={font:20,theme:'light',projects:[{root:draft.root,name:'Invented',issues:board,alive:true,complete:true,tasks:[]}],ai:{history:[]},messages:{complete:true,entries:[]},conversations:{items:[],loading:true,complete:false}};
  let recovery={conversation:{drafts:[{...draft,text:'Recovered while loading',version:3}],localQueue:[],context:{selected:draft.id,list:'recent',selection:{recent:draft.id},scrolls:{},positions:{},focuses:{},opened:[],listOnly:false,search:'',filter:'all'}},navigation:{page:'conversations'}};
  for(let fault=0;fault<2;fault++){
   await page.goto('about:blank');await page.setContent(html);
   await page.evaluate(({state,recovery})=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='workspace-checkpoint')window.checkpoint=m.value;}};taskq.receive(state);taskq.receive({workspaceRecovery:recovery});taskq.receive({workspaceRecoveryReady:true});},{state,recovery});
   recovery=await page.evaluate(()=>window.checkpoint);
   assert.equal(recovery.conversation.drafts[0]?.text,'Recovered while loading','loading must not discard unbound recovered edits');
   assert.match(await page.locator('main').innerText(),/Recovered draft source unavailable/);
   if(process.env.TASKQ_EVIDENCE_2140&&fault===1){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2140,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_2140+'/unresolved-draft.png'});}
  }
  await page.evaluate(({state,draft})=>taskq.receive({...state,conversations:{complete:true,items:[{...draft,lazy:true}],navigation:{selected:draft.id}}}),{state,draft});
  await page.waitForFunction(()=>sent.some(m=>m.a==='conversation-dialogue'&&m.action==='body'));
  await page.evaluate(draft=>{const m=sent.find(m=>m.a==='conversation-dialogue'&&m.action==='body');taskq.receive({dialogueReceipt:{...m,state:'accepted',value:{...draft,lazy:false}}});},draft);
  const field=page.locator('[data-c-field=text]');await field.waitFor();
  assert.equal(await field.inputValue(),'Recovered while loading','exact lazy source hydration must overlay recovered text');
  assert.deepEqual(await field.evaluate(e=>[e.selectionStart,e.selectionEnd]),draft.caret);
  assert.match(await page.locator('.conversation-files').innerText(),/notes.md/);
  assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','answer-local','conversation-action','reports','message-action'].includes(m.a)).length),0,'late hydration cannot send or save automatically');
  recovery=await page.evaluate(()=>window.checkpoint);
  for(let fault=0;fault<2;fault++){
   await page.goto('about:blank');await page.setContent(html);
   await page.evaluate(({state,draft,recovery})=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='workspace-checkpoint')window.checkpoint=m.value;}};taskq.receive({...state,conversations:{complete:true,items:[{...draft,sent:{revision:2,root:draft.root,text:'Different source',files:[]}}]}});taskq.receive({workspaceRecovery:recovery});},{state,draft,recovery});
   assert.match(await page.locator('main').innerText(),/Recovered draft source changed/,'source mismatch must remain explicit across successive recoveries');
   assert.equal(await field.inputValue(),'Recovered while loading');
   assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','answer-local','conversation-action','reports','message-action'].includes(m.a)).length),0);
   recovery=await page.evaluate(()=>window.checkpoint);
  }

 }finally{await page.close();}
}
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const regressions=await Promise.allSettled(['answer','steer'].flatMap(kind=>[false,true].map(saved=>taskRecovery(browser,kind,saved))).concat(loadingRecovery(browser)));
  assert.deepEqual(regressions.filter(r=>r.status==='rejected').map(r=>r.reason.message),[]);
  const page=await browser.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));let cells=0;
  for(const runtime of ['codex','claude'])for(const theme of ['dark','light'])for(const font of [18,20])for(const width of [380,900]){
   await page.setViewportSize({width,height:740});const task=browserTask(runtime);
   const state={font,theme,workspace_local:{[board+'#112']:{draft:'Do not resend',projectRoot:'invented-root',op:{id:'invented-steer-2140',task:112,revision:71,action:'steer',text:'Same immutable input'},receipt:{state:'unknown'}}},projects:[{root:'invented-root',name:'Invented',issues:board,alive:true,complete:true,tasks:[task]}],ai:{history:[]},messages:{complete:true,entries:[]},conversations:{complete:true,items:[draft],navigation:{selected:draft.id,list:'recent'}}};
   async function mount(recovery){
    await page.goto('about:blank');await page.evaluate(()=>window.taskqNativeFrame=true);
    await page.setContent(html);
    await page.evaluate(({state,recovery})=>{
     window.sent=[];window.fixture=state;window.invalidWorkerPage=!recovery;
     window.ipc={postMessage:text=>{
      const m=JSON.parse(text);sent.push(m);
      if(m.a==='workspace-checkpoint')window.checkpoint=m.value;
      if(m.a==='conversation-dialogue')queueMicrotask(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:m.action==='transcript'&&window.invalidWorkerPage?(window.invalidWorkerPage=false,{}):m.action==='task-body'?{...state.projects[0].tasks[0],body_lazy:false,history:[]}:m.action==='transcript'?{generation:'invented-source-a',start:0,end:10,size:10,items:[{at:0,i:0,role:'assistant',text:'Reading the original worker source.',updated:Date.now()-1000}]}:{state:'idle',turn:null}}}));
     }};
     taskq.receive(state);if(recovery)taskq.receive({workspaceRecovery:recovery});
    },{state,recovery});
   }
   await mount();
   await page.evaluate(({board})=>taskq.receive({open:{page:'conversations',board,task:112,root:'invented-root'}}),{board});
   await page.waitForFunction(()=>document.querySelector('.worker-live')?.textContent.includes('response invalid')||getComputedStyle(document.querySelector('.workspace')).display==='none');
   assert(await page.locator('.worker-live').isVisible(),'an invalid accepted local-source page must not replace the whole workspace');
   assert.match(await page.locator('.worker-live').innerText(),/response invalid/);
   await page.locator('.worker-live .dialogue-item').waitFor();
   if(process.env.TASKQ_EVIDENCE_2140&&runtime==='codex'&&theme==='dark'&&font===20&&width===900){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2140,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_2140+'/worker-document.png'});}
   assert.equal(await page.getByText('Interface error',{exact:false}).count(),0);
   const request=await page.evaluate(()=>sent.find(m=>m.action==='transcript'));
   assert.deepEqual([request.root,request.attempt,request.conversation,request.session],['invented-root',71,thread,undefined]);
   // Explicit refusal retains the current source and gives a reason.
   await page.evaluate(()=>{const m=sent.filter(m=>m.action==='transcript').at(-1);taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:'transcript',state:'unknown',reason:'Invented source unavailable'}});});
   await page.evaluate(()=>taskq.receive({open:{page:'conversations'}}));
   if(width<620){const list=page.locator('[data-c=list]');if(await list.count())await list.click();}
   await page.locator('[data-c=select][data-id="'+draft.id+'"]').click();
   const field=page.locator('[data-c-field=text]');await field.fill('Unsaved local recovery draft');
   await field.evaluate(e=>{e.focus();e.setSelectionRange(3,9);e.dispatchEvent(new Event('select',{bubbles:true}));});
   await page.waitForFunction(()=>checkpoint?.conversation.drafts.some(d=>d.text==='Unsaved local recovery draft'&&d.caret[0]===3));
   const checkpoint=await page.evaluate(()=>window.checkpoint),originalSave=await page.evaluate(()=>sent.find(m=>m.a==='conversation-action'&&m.action==='save'));
   assert.equal(checkpoint.conversation.pending?.id,originalSave.id,'checkpoint must retain the original pending native-save ID');
   checkpoint.conversation.localQueue.push({conversation:draft.id,action:'remove',file:'invented-file'});
   for(let fault=0;fault<3;fault++){
    await page.evaluate(()=>{window.sent=[];});
    // Independent guard still works after product taskq itself has failed.
    await page.evaluate(()=>{window.taskqWebFault('script',{error:'TypeError',site:'invented:1:1'});window.taskq=null;});
    assert.equal(await page.locator('.workspace').evaluate(e=>getComputedStyle(e).display),'none');
    for(const label of ['Reload safely','Back to safe view','Close'])assert(await page.getByRole('button',{name:label,exact:true}).isVisible(),label);
    await page.getByRole('button',{name:'Close',exact:true}).click();
    await page.getByRole('button',{name:'Reload safely',exact:true}).click();
    assert(await page.evaluate(()=>sent.some(m=>m.a==='hide')&&sent.some(m=>m.a==='workspace-reload')));
    assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','ai-send','reports','message-action','conversation-action'].includes(m.a)).length),0,'recovery must not send, decide or acknowledge');
    await mount(checkpoint);await field.waitFor();assert.equal(await field.inputValue(),'Unsaved local recovery draft');
    assert.deepEqual(await field.evaluate(e=>[e.selectionStart,e.selectionEnd]),[3,9]);
    assert.match(await page.locator('.conversation-files').innerText(),/notes.md/);
    assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action').length),0,'mount/recovery must not replay even local storage actions');
    assert.equal((await page.evaluate(()=>window.checkpoint)).conversation.pending?.id,checkpoint.conversation.pending?.id,'retain the original pending operation ID');
   }
   await page.locator('[data-c=retry-save]').click();
   const retried=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action'));
   assert.equal(retried.length,1,'one explicit retry submits only its interrupted operation');assert.equal(retried[0].action,'save');
   await page.evaluate(m=>taskq.receive({conversationReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:null}}),retried[0]);
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action').length),1,'successful retry must not drain recovered attach/remove/save work');
   const paused=checkpoint.conversation.localQueue;
   for(const op of paused){
    const label=op.action==='remove'?'Remove recovered attachment':'Save recovered draft';
    await page.getByRole('button',{name:label,exact:true}).waitFor();
    const before=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action').length);
    await page.getByRole('button',{name:label,exact:true}).click();
    const request=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action').at(-1));
    assert.equal(request.action,op.action,'each queued mutation retains its action');
    assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action').length),before+1,'one click submits one recovered mutation');
    await page.evaluate(m=>taskq.receive({conversationReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:null}}),request);
    assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action').length),before+1,'receipt cannot dispatch the next recovered mutation');
   }
   await page.evaluate(({draft,saved})=>{const s={...window.fixture,conversations:{...window.fixture.conversations,items:[{...draft,...saved,revision:2}]}};taskq.receive(s);},{draft,saved:retried[0].draft});
   assert.equal(await field.inputValue(),'Unsaved local recovery draft','an old save receipt cannot replace recovered edits');
   assert.deepEqual(await field.evaluate(e=>[e.selectionStart,e.selectionEnd]),[3,9],'an old save/snapshot cannot replace recovered caret');
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'ordinary text must not create horizontal scroll');
   if(process.env.TASKQ_EVIDENCE_2140&&runtime==='codex'&&theme==='dark'&&font===20&&width===900){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2140,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_2140+'/recovered-draft.png'});}
   cells++;
  }
  assert.deepEqual(errors,[]);console.log('PASS '+cells+' Codex/Claude × theme × 18/20px × narrow/wide cells; three faults each; exact lazy source, answer/steer and delayed owner drafts/files/caret/focus, immutable checks and no replay');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
