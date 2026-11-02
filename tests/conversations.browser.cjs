// #1580 C-A1–10/CW1–8: actual bundle with explicit invented native receipts; no native launch/board effects.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),crypto=require('node:crypto'),{chromium}=require('playwright');
const evidence=process.env.TASKQ_EVIDENCE_1580;
const snapshot={font:16,theme:'dark',host:'fixture-host',projects:[{root:'fixture-alpha',name:'Alpha',alive:true,complete:true,issues:'https://example.org/alpha/issues',tasks:[]}],ai:{history:['History fills only'],runtime:{id:'claude',name:'Claude Code',available:true,choices:[{id:'claude',name:'Claude Code',available:true},{id:'codex',name:'Codex',available:true}]}},conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:900,height:720}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  const html=fs.readFileSync(process.env.TASKQ_CONVERSATION_HTML||'build/web/workspace.html','utf8');
  const state=structuredClone(snapshot),fileData=new Map();let seen=0,paused=false;
  // Native preference saves acknowledge asynchronously, including delayed caret/scroll saves.
  // Other effects retain explicit fixture receipts; Rust tests exercise the real persistence/launch adapter.
  await page.exposeFunction('fixtureSave',async m=>{
   let c=state.conversations.items.find(c=>c.id===m.conversation);
   if(!c){c={id:m.conversation,revision:0,text:'',title:'',root:'fixture-alpha',caret:[0,0],scroll:0,files:[],archived:false};state.conversations.items.push(c);}
   Object.assign(c,m.draft);c.revision++;
   await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:m.id,state:'accepted',snapshot:state.conversations});
  });
  async function boot(){await page.setContent(html);await page.evaluate(()=>{window.sent=[];window.fixtureSaving=0;window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='conversation-action'&&m.action==='save'){fixtureSaving++;window.fixtureSave(m).then(()=>fixtureSaving--);}}};});await page.evaluate(s=>taskq.receive(s),state);await page.evaluate(()=>taskq.receive({open:{page:'conversations'}}));}
  async function drain(stop){
   for(let i=0;i<50;i++){
    await page.waitForFunction(()=>fixtureSaving===0);
    const all=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-action'&&m.action!=='save'));if(seen===all.length)return;
    const m=all[seen++];if(paused||m.action===stop)return m;
    let c=state.conversations.items.find(c=>c.id===m.conversation);
    if(!c){c={id:m.conversation,revision:0,text:'',title:'',root:'fixture-alpha',caret:[0,0],scroll:0,files:[],archived:false};state.conversations.items.push(c);}
    if(m.action==='save')Object.assign(c,m.draft);
    if(m.action==='attach'){fileData.set(m.file.id,m.data);const data=Buffer.from(m.data,'base64');c.files.push({...m.file,hash:crypto.createHash('sha256').update(data).digest('hex')});}
    if(m.action==='remove')c.files=c.files.filter(f=>f.id!==m.file);
    if(m.action==='archive')c.archived=true;
    if(m.action==='restore')c.archived=false;
    if(m.action==='send'){c.sent={text:c.text,root:c.root,runtime:m.runtime,files:structuredClone(c.files)};c.request={id:c.id,state:'pending',kind:'starting',next:'Awaiting native launch'};}
    let value=m.action==='preview'?{file:c.files.find(f=>f.id===m.file),data:fileData.get(m.file)}:null;
    if(m.action==='fork'){const next={id:m.fork_id,revision:1,title:'',root:c.root,text:c.text,caret:c.caret,scroll:0,files:structuredClone(c.files),archived:false};state.conversations.items.push(next);c.text='';c.caret=[0,0];c.files=[];value={forked:next.id};}
    if(m.action!=='preview')c.revision++;
    await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:m.id,state:'accepted',value,snapshot:state.conversations});
   }throw Error('Fixture queue failed to settle');
  }
  async function capture(name){if(evidence){fs.mkdirSync(evidence,{recursive:true});await page.screenshot({path:path.join(evidence,name+'.png')});}}
  async function details(){const d=page.locator('.conversation-info');if(!await d.evaluate(e=>e.open))await d.locator('summary').click();}
  async function tool(action){await details();await page.locator('[data-c='+action+']').click();}
  await boot();
  assert.equal(await page.locator('[data-c=new]').count(),1,'CW1: Conversations must have direct New');
  await page.locator('.conversation-view>summary').click();await page.locator('[data-c=result-filter][data-filter=finished]').click();
  await page.locator('[data-c=new]').click();await drain();
  assert.equal(await page.locator('.conversation-view>summary').innerText().then(t=>t.includes('Finished')),false,'#2159 New clears Finished scope');
  assert.equal(await page.locator('.conversation-row[aria-current=page]').count(),1,'New remains visibly selected');
  const input=page.locator('[data-c-field=text]');assert.equal(await input.evaluate(e=>document.activeElement===e),true,'New focuses its own composer');
  await input.fill('Design draft\nFirst independent context');await input.dispatchEvent('select');await drain();
  await input.evaluate(e=>{e.focus();e.setSelectionRange(2,9);e.dispatchEvent(new Event('select',{bubbles:true}));});await drain();
  await page.evaluate(()=>{taskq.receive({workerPanelClosed:true});taskq.receive({open:{page:'conversations',thread:'centre'}});});
  assert(await input.evaluate(e=>document.activeElement===e),'#2159 native-shaped Close/reopen restores composer focus');assert.deepEqual(await input.evaluate(e=>[e.selectionStart,e.selectionEnd]),[2,9],'Close/reopen retains selected text');
  assert.match(await page.locator('.conversation-row[aria-current=page]').innerText(),/^Design draft/,'#1748 A2: an unsent draft names itself by its first line');
  // Switching before the debounce must save the previous item, even if another draft is immediately edited.
  await page.locator('[data-c=new]').click();await drain();await input.fill('Release draft');await input.evaluate(e=>e.setSelectionRange(2,5));await input.dispatchEvent('select');await drain();
  await details();await page.locator('[data-c-field=runtime]').selectOption('codex');await drain();
  const [first,second]=state.conversations.items;assert.equal(second.runtime,'codex','#2159 choice is saved per request');assert.equal(first.runtime,'claude','other draft keeps its runtime');await capture('runtime-choice-dark-16');
  if(evidence){
   const preview=structuredClone(state);preview.version='2159 isolated fixture preview';preview.conversations.navigation={selected:second.id,list:'recent'};
   const boot=`<script>document.title='TaskQ 2159 isolated fixture preview';(()=>{const state=${JSON.stringify(preview).replaceAll('<','\\u003c')};window.ipc={postMessage:text=>{const m=JSON.parse(text);if(m.a!=='conversation-action')return;queueMicrotask(()=>{if(m.action!=='save'){taskq.receive({conversationReceipt:{id:m.id,state:'unknown',reason:'Isolated preview: native and board actions are disabled'}});return;}let c=state.conversations.items.find(c=>c.id===m.conversation);if(!c){c={id:m.conversation,revision:0,files:[],archived:false};state.conversations.items.push(c);}Object.assign(c,m.draft);c.revision++;taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:state.conversations}});});}};taskq.receive(state);taskq.receive({open:{page:'conversations'}});})();</script>`;
   fs.writeFileSync(path.join(evidence,'preview.html'),html.replace('</body>',boot+'</body>'));
  }
assert.equal(first.text,'Design draft\nFirst independent context');assert.equal(second.text,'Release draft');
  await page.locator('[data-c=select][data-id="'+first.id+'"]').click();await drain();assert.equal(await input.inputValue(),first.text);assert(await input.evaluate(e=>document.activeElement===e));
  await details();await page.locator('[data-c-field=title]').fill('Explicit name');await input.focus();await drain();await input.fill('Changed temporary title');await input.dispatchEvent('select');await drain();
  assert.match(await page.locator('[data-c=select][data-id="'+first.id+'"]').innerText(),/Explicit name/,'explicit title wins');
  // #1775 C5: sidebar search is an icon until opened or a query is active; clear closes it.
  await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('no match');await page.locator('[data-c=clear-search]').click();assert.equal(await page.locator('[data-c-field=search]').count(),0,'cleared search closes');assert.equal(await input.inputValue(),'Changed temporary title','clear search keeps selection');
  await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('hidden');await page.locator('[data-c=new]').click();await drain();assert.equal(await page.locator('[data-c-field=search]').count(),0,'New clears and closes search');assert(await input.evaluate(e=>document.activeElement===e));
  await input.fill('Third draft');await input.dispatchEvent('select');await drain();
  const third=state.conversations.items[2];await tool('archive');await drain();assert.equal(await input.inputValue(),first.text,'Archive selects another item without switching list');assert.equal(await page.locator('[data-c=recent]').getAttribute('aria-pressed'),'true');
  await page.locator('[data-c=undo]').click();await drain();assert.equal(await input.inputValue(),'Third draft');assert(await input.evaluate(e=>document.activeElement===e));
  await tool('archive');await drain();await page.locator('[data-c=archive-list]').click();await page.locator('[data-c=select][data-id="'+third.id+'"]').click();
  // #1775 R43: Archive and Restore draw the catalog archive glyphs (visual-style control icons), not download/undo
  assert.equal(await page.locator('[data-c=archive-list] .lucide-archive').count(),1);assert.equal(await page.locator('[data-c=restore] .lucide-archive-restore').count(),1);
  await tool('restore');await drain();assert.equal(await input.inputValue(),'Third draft');
  await page.locator('[data-c=recent]').click();await page.locator('[data-c=select][data-id="'+second.id+'"]').click();await drain();assert.deepEqual(await input.evaluate(e=>[e.selectionStart,e.selectionEnd]),[2,5],'separate caret retained');
  // C-A3/F3: list scroll is independent per Recent/Archive; a new row is visible even in a dense list.
  state.conversations.items.push(...Array.from({length:60},(_,n)=>({id:'dense-'+n,revision:1,title:'Dense saved request '+n,root:'fixture-alpha',text:'Saved draft '+n,caret:[0,0],scroll:0,files:[],archived:false})));
  await page.evaluate(s=>taskq.receive(s),state);
  const rows=page.locator('.conversation-rows');await rows.evaluate(e=>{e.scrollTop=240;e.dispatchEvent(new Event('scroll',{bubbles:true}));});
  await page.locator('[data-c=archive-list]').click();await page.locator('[data-c=recent]').click();assert.equal(await rows.evaluate(e=>e.scrollTop),240,'Recent restores its own list scroll');
  await page.locator('[data-c=new]').click();await drain();
  const selectedRow=page.locator('.conversation-row[aria-current=page]');const selectedBox=await selectedRow.boundingBox(),listBox=await rows.boundingBox();
  assert(selectedBox.y>=listBox.y&&selectedBox.y+selectedBox.height<=listBox.y+listBox.height+1,'New reveals its own row in a dense list');
  await page.locator('[data-c=select][data-id="'+second.id+'"]').click();await drain();
  // History is not a submission; IME and Shift+Enter are never sends.
  await page.locator('[data-view-key=conversation-history] summary').click();await page.locator('[data-c=history]').click();await drain();assert.equal(await input.inputValue(),'History fills only');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),0);
  await input.dispatchEvent('compositionstart');await input.dispatchEvent('keydown',{key:'Enter',isComposing:true});await input.dispatchEvent('compositionend');await input.press('Shift+Enter');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),0,'IME/Shift+Enter cannot send');
  await input.fill('Review the attached text');await input.dispatchEvent('select');await drain();
  await page.locator('[data-c-field=files]').setInputFiles({name:'fixture.txt',mimeType:'text/plain',buffer:Buffer.from('invented attachment bytes')});
  await page.waitForFunction(()=>sent.some(m=>m.action==='attach'));await drain();assert.match(await page.locator('.conversation-files').innerText(),/fixture.txt/);
  const file=second.files[0];assert.equal(file.bytes,25);assert.equal(file.hash,crypto.createHash('sha256').update('invented attachment bytes').digest('hex'));
  await page.locator('[data-c=preview]').click();await drain();assert.equal(await page.locator('[aria-label="Local file preview"] pre').innerText(),'invented attachment bytes');await page.locator('[data-c=preview-close]').click();
  await page.locator('[data-c-field=files]').setInputFiles({name:'broken.png',mimeType:'image/png',buffer:Buffer.from('\x89PNG\r\n\x1a\n','latin1')});
  await page.waitForFunction(()=>document.querySelector('.conversation-status').textContent.includes('Image unreadable'));
  assert.equal(await input.inputValue(),'Review the attached text');assert.equal(second.files.length,1,'bad image keeps other files and cannot fault the whole interface');
  await page.locator('[data-c-field=files]').setInputFiles({name:'bad.pdf',mimeType:'application/pdf',buffer:Buffer.from('%PDF-invented')});assert.match(await page.locator('.conversation-status').innerText(),/PDF unavailable/);assert.equal(await input.inputValue(),'Review the attached text');
  // Exact rendered adverse fixture: lost native readback keeps edit/files and original request identity.
  await capture('ready-dark-16');
  paused=true;await input.press('Enter');
  // First save, then Send: acknowledge the former, withhold the latter.
  paused=false;await drain();
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),1);assert.equal(await page.evaluate(()=>sent.find(m=>m.action==='send').runtime),'codex','Send freezes the explicit choice');
  await input.fill('Next draft while launch waits');await input.dispatchEvent('select');await drain();assert.equal(second.sent.text,'Review the attached text');assert.equal(second.sent.files[0].hash,file.hash);
  // #1748 A1: the next draft never renames a sent thread in its heading or sidebar row.
  assert.match(await page.locator('[data-c=select][data-id="'+second.id+'"]').innerText(),/^Review the attached text/);assert.equal(await page.locator('[data-c-field=title]').getAttribute('placeholder'),'Review the attached text');
  assert.match(await page.locator('.conversation-record').innerText(),/Awaiting native launch/);assert.equal(await page.locator('[data-c=cancel]').count(),1);
  await input.press('Enter');await drain();assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),1,'one immutable launch per item');
  // A receipt from another item/operation cannot change text or claim cancellation.
  await page.evaluate(()=>taskq.receive({conversationReceipt:{id:'foreign',state:'accepted'}}));assert.equal(await input.inputValue(),'Next draft while launch waits');
  await page.locator('[data-c=reconcile]').click();await drain();const retries=await page.evaluate(()=>sent.filter(m=>m.action==='reconcile'));assert(retries.every(m=>m.conversation===second.id));
  const launched={id:second.id,state:'launched',kind:'launched',runtime:'codex',url:'codex://threads/invented-1580',next:'Read and answer in Codex'};second.request=launched;await page.evaluate(s=>taskq.receive(s),state);
  // #1699 A5: the handoff sits under Launch details
  await details();await page.locator('.native-session').click();assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='open').at(-1).url),launched.url);assert.equal(await page.locator('[data-c=cancel]').count(),0,'no runtime Stop adapter is invented');
  await capture('launched-dark-16');
  // Native snapshot round trip (not browser localStorage); chat-first reload never requires opening the tray first.
  await input.evaluate(e=>e.setSelectionRange(3,8));await input.dispatchEvent('select');await drain();
  const nav=await page.evaluate(()=>sent.filter(m=>m.a==='conversation-navigation').at(-1)?.navigation);state.conversations.navigation=nav||{selected:second.id,list:'recent'};
  state.conversations.navigation.selected=second.id;
  const sendsBefore=await page.evaluate(()=>sent.filter(m=>m.action==='send').length);await boot();seen=0;
  assert.equal(await input.inputValue(),'Next draft while launch waits');assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),0,'restart never auto-replays');assert.equal(sendsBefore,1);
  await page.locator('[data-c=select][data-id="'+first.id+'"]').click();await drain();assert.equal(await input.inputValue(),first.text);
  // Ownership transfer is explicit; a main view cannot edit a detached item or resend it.
  await page.evaluate(id=>taskq.receive({workspaceOwner:{id:'main',detached:[id]}}),first.id);assert(await input.isDisabled());
  await page.locator('[data-c=select][data-id="'+first.id+'"]').click();assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-window').at(-1).action),'raise');
  await page.evaluate(id=>taskq.receive({workspaceOwner:{id:'main',detached:[],select:id}}),first.id);assert.equal(await input.isDisabled(),false);
  // A5/A6: a valid image previews locally; New from draft moves the selected bytes to a separate unsent identity.
  await page.locator('[data-c=select][data-id="'+second.id+'"]').click();await drain();
  const png=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=c.height=2;c.getContext('2d').fillRect(0,0,2,2);return c.toDataURL('image/png').split(',')[1];});
  const attachedBefore=await page.evaluate(()=>sent.filter(m=>m.action==='attach').length);
  await page.locator('[data-c-field=files]').setInputFiles({name:'local.png',mimeType:'image/png',buffer:Buffer.from(png,'base64')});
  await page.waitForFunction(n=>sent.filter(m=>m.action==='attach').length>n,attachedBefore);await drain();
  await page.locator('[data-c=preview]').click();await drain();await page.locator('[aria-label="Local file preview"] img').waitFor({state:'visible'});
  assert.equal(await page.locator('[aria-label="Local file preview"] img').evaluate(e=>e.naturalWidth),2);await page.locator('[data-c=preview-close]').click();
  const nextFiles=structuredClone(second.files);await tool('from-draft');await drain();
  const forked=state.conversations.items.at(-1);assert.notEqual(forked.id,second.id);assert.equal(await input.inputValue(),'Next draft while launch waits');assert.deepEqual(forked.files,nextFiles);assert.equal(forked.sent,undefined);
  assert.equal(second.sent.text,'Review the attached text');assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),0,'draft transfer never launches');
  // The selected fork can live on the next bounded page; revisit the original through normal paging.
  if(!await page.locator('[data-c=select][data-id="'+second.id+'"]').count())await page.getByRole('button',{name:'Previous threads',exact:true}).click();
  await page.locator('[data-c=select][data-id="'+second.id+'"]').click();await input.fill('Next draft while launch waits');await input.dispatchEvent('select');await drain();
  // #1665 D3: a launched item replies in place; a refused turn shows its reason and keeps the next draft.
  await page.locator('[data-c=select][data-id="'+second.id+'"]').click();await drain();await page.locator('[data-c=reply]').click();
  const pending=await drain('reply');
  await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:pending.id,state:'unknown',reason:'working',snapshot:state.conversations});
  assert.match(await page.locator('.conversation-status').innerText(),/Not sent: working\. Draft kept/);assert.equal(await input.inputValue(),'Next draft while launch waits');await capture('partial-dark-16');
  await page.setViewportSize({width:350,height:560});state.font=20;state.theme='light';await page.evaluate(s=>taskq.receive(s),state);
  assert(await page.locator('[data-c=new]').isVisible());assert(await page.locator('[data-c=list]').isVisible());assert(await input.isVisible());
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'no horizontal page scrolling');
  await capture('narrow-light-20');
  await page.locator('[data-c=list]').click();assert(await page.locator('[data-c=search-open]').isVisible());await page.locator('[data-c=select][data-id="'+first.id+'"]').click();assert(await input.isVisible());
  await page.getByRole('button',{name:'Close workspace',exact:true}).click();assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='hide').length),1);assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),0,'close never sends');
  await page.setViewportSize({width:800,height:650});state.font=20;await page.evaluate(s=>taskq.receive(s),state);
  await page.locator('.conversation-view>summary').click();await page.locator('[data-c=result-filter][data-filter=activity]').click();
  const label=await page.locator('.conversation-view>summary .view-label').boundingBox(),count=await page.locator('.conversation-view>summary .view-count').boundingBox();
  assert(Math.abs(label.y-count.y)<=2,'#2159 sidebar mode and exact count stay on one line at native-sized width and 20px');
  await capture('sidebar-count-light-20');
  assert.deepEqual(errors,[]);
  console.log('PASS #1580: CW1–8, independent native drafts/caret/files, archive/Undo/Restore, explicit names/search, history fills only, IME/keyboard, immutable send IDs, native-only links, partial delivery, restart/close and narrow layout');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
