// Chain: #2365 A4/A5 adoption offer -> Conversations keeps draft, files, caret and selection, and replays nothing; menu edits hold their page.
// Risk: adoption while typing loses text or focus; pending work replays after the page changes; a refused bundle hides its reason.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const board='https://example.org/invented/issues',key=board+'#112',root='invented-root';
const files=[{id:'invented-file',name:'notes.md',mime:'text/markdown',bytes:2,hash:'0'.repeat(64)}];
const state={font:20,theme:'dark',workspace_local:{[key]:{draft:'Old disk text',caret:[0,0],files,projectRoot:root}},projects:[{root,name:'Invented',issues:board,alive:true,complete:true,tasks:[{n:112,title:'Question',revision:71,state:'ask',role:'worker',attempt:71,runtime:'codex',host:'invented-host',local_session:true,body_lazy:false,body_source_revision:71,history_count:0,executions:[{attempt:71,state:'running'}],question:{text:'Problem: Choose a path',choices:[]}}]}],ai:{history:[]},messages:{complete:true,entries:[]},conversations:{complete:true,items:[],navigation:{}}};
const passTenSeconds=page=>page.evaluate(()=>{const real=performance.now.bind(performance);performance.now=()=>real()+20000;});
async function conversations(browser){
 const page=await browser.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 const mount=async recovery=>{
  await page.goto('about:blank');await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(({state,recovery})=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='workspace-checkpoint')window.checkpoint=m.value;}};taskq.receive(state);if(recovery)taskq.receive({workspaceRecovery:recovery});},{state,recovery});
 };
 const reloads=()=>page.evaluate(()=>sent.filter(m=>m.a==='workspace-reload'&&m.adopt===true).length);
 const offer=()=>page.evaluate(()=>taskq.receive({bundleAdopt:true}));
 try{
  await mount();await page.evaluate(({board,root})=>taskq.receive({open:{page:'conversations',board,task:112,root}}),{board,root});
  const field=page.locator('[data-c-field=answer]');await field.click();await field.pressSequentially('Unsaved answer');
  await offer();
  assert.equal(await reloads(),0,'a reader who typed within 10 s keeps the page');
  await passTenSeconds(page);
  await field.evaluate(e=>{e.setSelectionRange(3,9);e.dispatchEvent(new Event('select',{bubbles:true}));});
  await offer();
  assert.equal(await reloads(),1,'an idle reader adopts on the next offer');
  const recovery=await page.evaluate(()=>window.checkpoint);assert(recovery,'adoption checkpoints before the page goes');
  await mount(recovery);await field.waitFor();
  assert.equal(await field.inputValue(),'Old disk textUnsaved answer','draft text survives');
  assert.deepEqual(await field.evaluate(e=>[e.selectionStart,e.selectionEnd]),[3,9],'caret survives');
  assert.match(await page.locator('.answer-composer').innerText(),/notes.md/,'attached file survives');
  assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','conversation-action','reports','message-action'].includes(m.a)).length),0,'adoption replays no operation');
  recovery.answers[key]={...recovery.answers[key],op:{id:'invented-pending',task:112,revision:71,text:'Immutable',files},receipt:{state:'sending'}};
  await mount(recovery);await field.waitFor();
  await passTenSeconds(page);
  await offer();
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer').length),0,'a pending answer is not sent again by an offer');
  assert.deepEqual(errors,[]);
 }finally{await page.close();}
}
// #2365 A4: the native catalog sends draft bodies lazily and transcripts by page, so the saved position outlives the first draw.
async function reading(browser){
 const text=Array.from({length:300},(_,i)=>'Reading line '+(i+1)).join('\n');
 const draft={id:'invented-draft',revision:1,root,title:'Draft',text,caret:[3,9],scroll:0,files};
 const launched={id:'invented-launched',revision:1,title:'Launched',text:'',root,caret:[0,0],scroll:0,files:[],archived:false,sent:{text:'Start',root,runtime:'codex',files:[]},request:{id:'invented-launched',state:'launched',kind:'launched',url:'https://example.org/invented',next:'Reply'},dialogue:{state:'idle',watch:'no watch',turn:null}};
 const items=Array.from({length:150},(_,i)=>({role:i%2?'assistant':'user',text:'Message '+i,at:i*100,i:0}));
 const page=await browser.newPage({viewport:{width:800,height:600}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 const mount=async(selected,recovery,navigation)=>{
  await page.goto('about:blank');await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(({draft,launched,items,selected,recovery,navigation,root,board})=>{
   window.sent=[];
   window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);
    if(m.a==='workspace-checkpoint')window.checkpoint=m.value;
    if(m.a==='conversation-dialogue'){const value=m.action==='body'?{...draft,lazy:false}:m.action==='status'?{state:'idle',watch:'no watch',turn:null,last_line:''}:{items,start:0,end:15000,size:15000};setTimeout(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value}}),10);}
   }};
   const {text,files,...row}=draft;
   taskq.receive({font:16,theme:'dark',projects:[{root,name:'Invented',issues:board,alive:true,complete:true,tasks:[]}],ai:{history:[]},messages:{complete:true,entries:[]},conversations:{complete:true,items:[{...row,lazy:true},launched],navigation:navigation||{selected,list:'recent'}}});
   taskq.receive({open:{page:'conversations'}});
   if(recovery){taskq.receive({workspaceRecovery:recovery});taskq.receive({workspaceRecoveryReady:true});}
  },{draft,launched,items,selected,recovery,navigation,root,board});
 };
 const adopt=()=>page.evaluate(()=>{const real=performance.now.bind(performance);performance.now=()=>real()+20000;taskq.receive({bundleAdopt:true});return {recovery:window.checkpoint,navigation:sent.filter(m=>m.a==='conversation-navigation').at(-1)?.navigation};});
 const reader=page.locator('.conversation-reader');
 try{
  await mount(draft.id);await page.locator('[data-c-field=text]').waitFor();await page.waitForFunction(t=>document.querySelector('[data-c-field=text]').value===t,text);
  await reader.hover();await page.mouse.wheel(0,120);await page.waitForFunction(()=>document.querySelector('.conversation-reader').scrollTop>0);await page.waitForTimeout(300);
  const position=await reader.evaluate(e=>e.scrollTop),first=await adopt();
  await mount(draft.id,first.recovery,first.navigation);await page.waitForFunction(t=>document.querySelector('[data-c-field=text]')?.value===t,text);await page.waitForTimeout(200);
  assert.equal(await reader.evaluate(e=>e.scrollTop),position,'a lazy draft returns to its reading position');
  await mount(launched.id);await page.locator('.dialogue-transcript>[data-key]').first().waitFor();await page.waitForTimeout(200);
  await reader.hover();await page.mouse.wheel(0,-400);await page.waitForTimeout(400);
  const place=()=>page.evaluate(()=>{const el=document.querySelector('.conversation-reader'),top=el.getBoundingClientRect().top,node=[...el.querySelectorAll('.dialogue-transcript>[data-key]')].find(n=>n.getBoundingClientRect().bottom>top);return {key:node.dataset.key,y:Math.round(node.getBoundingClientRect().top-top)};});
  const before=await place(),again=await adopt();
  assert(again.navigation?.threads?.positions?.[launched.id],'the position is saved before the page goes');
  await mount(launched.id,again.recovery,again.navigation);await page.locator('.dialogue-transcript>[data-key]').first().waitFor();await page.waitForTimeout(400);
  const after=await place();assert.equal(after.key,before.key,'a transcript returns to the same record');assert(Math.abs(after.y-before.y)<=2,'a transcript returns to the same offset');
  assert.deepEqual(errors,[]);
 }finally{await page.close();}
}
async function menu(browser){
 const page=await browser.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 try{
  await page.goto('about:blank');await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  const S={font:20,theme:'dark',page:'about',version:'9.9.9',host:'invented-host',now:1000,projects:[{root,name:'Invented',issues:board,alive:true,complete:true,tasks:[]}],ai:{draft:''},notify:{projects:{}},ui:{bundle:'1800000000-abcdef012345',source:'adopted',bridge:1,reason:null,check_error:'download failed'}};
  await page.evaluate(S=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};taskq.receive(S);},S);
  const reloads=()=>page.evaluate(()=>sent.filter(m=>m.a==='bundle-reload').length),offer=()=>page.evaluate(()=>taskq.receive({bundleAdopt:true}));
  await offer();
  assert.equal(await reloads(),1,'a menu without an edit adopts on the first offer');
  await page.evaluate(()=>{const field=document.createElement('input');field.id='invented-edit';field.value='unsaved';document.body.append(field);});
  await offer();
  assert.equal(await reloads(),1,'a field holding text keeps the page');
  await page.evaluate(()=>document.getElementById('invented-edit').remove());
  await offer();
  assert.equal(await reloads(),2,'the next offer adopts once the edit is gone');
  await page.evaluate(S=>taskq.receive({...S,ui:{...S.ui,source:'embedded',reason:'UI bundle 1800000000-abcdef012345 refused: bridge 2, binary needs 1; using 1700000000-abcdef012345'}}),S);
  const shown=await page.evaluate(()=>document.body.textContent);
  assert.match(shown,/refused: bridge 2, binary needs 1/,'a refused bundle shows its reason in the errors list');
  assert.match(shown,/Last check failed: download failed/,'a failed check stays in About');
  assert.deepEqual(errors,[]);
 }finally{await page.close();}
}
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  await conversations(browser);
  await reading(browser);
  await menu(browser);
  console.log('PASS adoption keeps draft, files and caret, replays nothing, waits for idle input and open edits');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
