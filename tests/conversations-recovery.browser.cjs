// Chain: P-W3/TW2/C2/C7/D4 recovery -> same sources/drafts/receipts, no replay or acknowledgement.
// Risk: native-shaped lazy local worker source faults the whole reader; rebuilding loses unsaved edits or replays input.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const html=fs.readFileSync(process.env.TASKQ_CONVERSATION_HTML||'build/web/workspace.html','utf8');
const board='https://example.org/invented/issues',thread='task:'+board+'/112';
const draft={id:'invented-owner-2140',revision:1,root:'invented-root',title:'Independent draft',text:'Saved draft',caret:[2,4],scroll:0,files:[{id:'invented-file',name:'notes.md',mime:'text/markdown',bytes:2,hash:'0'.repeat(64)}]};
const browserTask=runtime=>({n:112,title:'Existing local worker',revision:71,state:'doing',role:'worker',attempt:71,runtime,host:'invented-host',local_session:true,body_lazy:true,body_source_revision:71,history_count:0,executions:[{attempt:71,state:'running'}]});
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
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
   await page.evaluate(()=>taskq.receive({open:{page:'conversations',thread:'recovery'}}));
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
    for(const label of ['Reload safely','Back to list','Close'])assert(await page.getByRole('button',{name:label,exact:true}).isVisible(),label);
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
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'ordinary text must not create horizontal scroll');
   if(process.env.TASKQ_EVIDENCE_2140&&runtime==='codex'&&theme==='dark'&&font===20&&width===900){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2140,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_2140+'/recovered-draft.png'});}
   cells++;
  }
  assert.deepEqual(errors,[]);console.log('PASS '+cells+' Codex/Claude × theme × 18/20px × narrow/wide cells; three faults each; exact lazy source, draft/files/caret and no replay');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
