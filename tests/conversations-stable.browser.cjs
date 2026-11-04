// #1741 Q-A2/Q-A7/Q-A8/Q-A11: actual bundle, invented native reads and receipts; no board/native effects.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium,webkit}=require('playwright');
const board='https://example.org/alpha/issues',root='fixture',fx=require('./fixtures/answer-card.json');
const question=structuredClone(fx.cases.find(c=>c.name==='choice').rows[0]);
question.question.problem='Retained context and alternatives. '.repeat(200);
const owner=id=>({id,title:id,revision:1,root,text:'Independent draft',caret:[2,4],scroll:0,files:[],archived:false,sent:{text:'Original',runtime:'claude',files:[]},request:{state:'launched',runtime:'claude',id}});
const initialState={font:20,theme:'dark',host:'fixture',projects:[{root,name:'Alpha',issues:board,alive:true,complete:true,tasks:[question,{...question,n:question.n+1}]}],ai:{history:[],runtime:{id:'claude',available:true}},conversations:{complete:true,items:[owner('owner-a'),owner('owner-b')],navigation:{selected:'owner-a',list:'recent'}},messages:{complete:true,entries:[]}};
(async()=>{for(const engine of [chromium,webkit]){const state=structuredClone(initialState),browser=await engine.launch(require('./browser.cjs')());const failures=[];console.log('Browser '+engine.name());try{
 const page=await browser.newPage({viewport:{width:940,height:740}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};});
 await page.evaluate(s=>taskq.receive(s),state);
 async function tool(action){const d=page.locator('.conversation-info');if(!await d.evaluate(e=>e.open))await d.locator('summary').click();await page.locator('[data-c='+action+']').click();}
 const open=async n=>page.evaluate(({board,n})=>taskq.receive({open:{page:'conversations',board,task:n}}),{board,n});
 async function check(name,run){try{await run();console.log('PASS '+name);}catch(e){failures.push(name+': '+e.message);console.error('FAIL '+name+': '+e.message);}finally{await page.evaluate(()=>{if(window.nativeFocus){HTMLElement.prototype.focus=nativeFocus;delete window.nativeFocus;}});}}
 await check('F15/F16 fixed heading and reachable historical answer at 350/620/940',async()=>{
  await open(question.n);
  for(const width of [940,620,350]){
   await page.setViewportSize({width,height:740});
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   const reader=page.locator('.conversation-reader').or(page.locator('.conversation-detail').filter({hasNot:page.locator('.conversation-reader')}));
   const heading=await page.locator('.thread-head').boundingBox();
   for(const fraction of [0,.5,1]){
    await reader.evaluate((e,f)=>{e.scrollTop=(e.scrollHeight-e.clientHeight)*f;e.dispatchEvent(new Event('scroll'));},fraction);
    const head=await page.locator('.thread-head').boundingBox();
    assert.equal(head.y,heading.y,'thread heading stays stable');
    assert.equal(await page.locator('[data-c-field=answer]').count(),1,'one answer stays at its historical question');
   }
   await page.locator('[data-a=answer-send]').scrollIntoViewIfNeeded();
   const send=await page.locator('[data-a=answer-send]').boundingBox(),head=await page.locator('.thread-head').boundingBox();
   assert(send.y>=head.y+head.height&&send.y+send.height<=740,'historical Send stays reachable below the sticky header at '+width);
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
 async function bootOwners(snapshot=state){
  await page.goto('about:blank');await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(s=>{window.sent=[];window.fixture=s;window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-action'&&m.action==='save'){const c=fixture.conversations.items.find(c=>c.id===m.conversation);Object.assign(c,m.draft);c.revision++;queueMicrotask(()=>taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:fixture.conversations}}));}}};taskq.receive(s);},snapshot);
 }
 // #2168 A1: receipt text must not resize the reader or move its pinned input. Native/board effects are substituted.
 await check('A1 status changes keep header, reader and composer geometry',async()=>{
  await bootOwners();
  for(const width of [940,350]){
   await page.setViewportSize({width,height:740});
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   const geometry=()=>page.evaluate(()=>Object.fromEntries(['.thread-head','.conversation-reader','.conversation-composer'].map(s=>{const r=document.querySelector(s).getBoundingClientRect();return [s,[r.x,r.y,r.width,r.height]];})));
   const before=await geometry();
   assert.doesNotMatch(await page.locator('.feedback-message').getAttribute('title'),/\[object Object\]/,'stale source diagnostics remain text');
   const out=(engine===chromium&&process.env.TASKQ_EVIDENCE_2168);if(out&&width===940){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:out+'/bundle-idle.png'});}
   await page.locator('[data-c=reply]').click();await page.waitForFunction(()=>sent.some(m=>m.action==='reply'));
   const op=await page.evaluate(()=>sent.filter(m=>m.action==='reply').at(-1));
   await page.evaluate(id=>taskq.receive({conversationReceipt:{id,state:'unknown',reason:'Invented delivery unavailable. Preserve the original operation and draft. '.repeat(6)}}),op.id);
   assert.deepEqual(await geometry(),before,'long receipt cannot displace the reading viewport at '+width);
   assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Independent draft');
   assert(await page.locator('[data-c=retry-save]').isVisible(),'full recovery remains reachable');
   assert(await page.locator('[data-c-field=text]').evaluate(e=>{const r=e.getBoundingClientRect();return e===document.elementFromPoint(r.x+r.width/2,r.y+r.height/2); }),'notice never overlays the input');
   await page.locator('.feedback-message').focus();
   assert.match(await page.locator('.time-tooltip').innerText(),/Invented delivery unavailable.*Preserve the original operation and draft/,'keyboard focus exposes the full receipt');
   await page.keyboard.press('Escape');
   if(out&&width===940)await page.screenshot({path:out+'/bundle-unknown.png'});
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='reply').length),1,'feedback never replays input');
   await bootOwners();
   const idle=await geometry();
   await page.evaluate(()=>taskq.receive({...fixture,ai:{...fixture.ai,error:'Invented workspace failure'}}));
   await page.evaluate(()=>taskq.receive({workspaceLocalError:'Invented draft save failure'}));
   assert.deepEqual(await geometry(),idle,'workspace and local errors cannot resize the reader');
   assert.match(await page.locator('.errors-entry [role=status]').innerText(),/Invented workspace failure/);
   assert.match(await page.locator('.errors-entry [role=status]').innerText(),/Invented draft save failure/,'a second failure remains visible and announced');
   await page.evaluate(()=>taskq.receive({workspaceLocalError:{reason:'Invented object-shaped failure'}}));
   assert.match(await page.locator('.errors-entry [role=status]').innerText(),/Invented object-shaped failure/,'structured failures retain their readable reason');
   assert.doesNotMatch(await page.locator('.feedback-message').innerText(),/\[object Object\]/,'status diagnostics never expose object coercion');
   await bootOwners();
  }
 });
 await page.setViewportSize({width:940,height:740});
 // #2168 D4: background notices must leave the native editor/IME owner untouched, including repeated focus calls.
 for(const kind of ['text','answer','steer'])await check('A4 '+kind+' typing survives notice appearance/update/dismissal',async()=>{
  const tasks=[structuredClone(question),{...structuredClone(question),n:question.n+1}];if(kind==='steer')Object.assign(tasks[0],{state:'doing',role:'worker',attempt:71,executions:[{attempt:71,state:'running'}]});
  await bootOwners({...state,projects:[{...state.projects[0],tasks}]});
  await open(question.n+1);await page.locator('[data-qac=choose]').first().click();const foreign=await page.evaluate(()=>sent.find(m=>m.a==='answer').op);
  if(kind!=='text')await open(question.n);else await page.locator('[data-c=select][data-id=owner-a]').click();
  const input=page.locator('[data-c-field='+kind+']');await input.fill('Keep original caret');await input.evaluate(e=>{e.setSelectionRange(5,13);window.retainedEditor=e;window.editorFocusCalls=0;window.editorBlur=0;window.compositionEnds=0;window.nativeFocus=HTMLElement.prototype.focus;HTMLElement.prototype.focus=function(...args){editorFocusCalls++;return nativeFocus.apply(this,args);};e.addEventListener('blur',()=>editorBlur++);e.addEventListener('compositionend',()=>compositionEnds++);e.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:'仮'}));});
  const reader=page.locator('.conversation-reader');await reader.evaluate(e=>{e.scrollTop=Math.min(120,e.scrollHeight-e.clientHeight);e.dispatchEvent(new Event('scroll'));});let anchor=await reader.evaluate(e=>e.scrollTop);
  const notices=[{workspaceLocalError:'Invented transient draft save failure'},{workspaceLocalError:'Invented updated draft save failure'},{workspaceLocalError:null},{answerReceipt:{root,id:foreign.id,state:'unknown',reason:'Invented pending receipt; check the original operation'}},{answerReceipt:{root,id:foreign.id,state:'accepted',reason:'Invented accepted foreign receipt'}},{...state,projects:[{...state.projects[0],tasks}],ai:{...state.ai,error:'Invented background workspace failure'}},{...state,projects:[{...state.projects[0],tasks}]}];
  let expected='Keep original caret',start=5,end=13;
  for(let i=0;i<notices.length;i++){
   await page.evaluate(s=>taskq.receive(s),notices[i]);
   const reason=notices[i].workspaceLocalError||notices[i].ai?.error;if(reason)assert.match(await page.locator('.errors-entry [role=status]').innerText(),new RegExp(reason),'genuine notice stays visible and announced');
   assert(await input.evaluate(e=>e===retainedEditor&&e===document.activeElement),kind+' retains its actual editor node and focus');
   assert.deepEqual(await input.evaluate(e=>[e.selectionStart,e.selectionEnd]),[start,end],kind+' retains caret/selection through notice '+i);
   assert.equal(await page.evaluate(()=>editorFocusCalls),0,kind+' background update must not issue a native focus request');
   assert.equal(await page.evaluate(()=>editorBlur+compositionEnds),0,kind+' composition is not interrupted');
   assert(Math.abs(await reader.evaluate(e=>e.scrollTop)-anchor)<=2,kind+' reading anchor stays put through notice '+i+' (expected '+anchor+', actual '+await reader.evaluate(e=>e.scrollTop)+')');
   const text=String(i);await page.keyboard.type(text);expected=expected.slice(0,start)+text+expected.slice(end);start+=text.length;end=start;
   assert.equal(await input.inputValue(),expected,kind+' next characters stay at the original caret without another click');
   // F16: typing in a historical answer may reveal its caret; background notices must preserve that new position.
   await page.evaluate(()=>new Promise(requestAnimationFrame));anchor=await reader.evaluate(e=>e.scrollTop);
  }
  await input.evaluate(e=>{HTMLElement.prototype.focus=nativeFocus;e.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'仮'}));});
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='conversation-action'&&['send','reply'].includes(m.action)).length),1,'notice typing never sends or replays beyond the explicitly initiated foreign operation');
 });
 await check('A1 catalog polling is quiet; stale failure has one stable recovery location',async()=>{
  await bootOwners();const input=page.locator('[data-c-field=text]');
  await input.focus();await input.evaluate(e=>e.setSelectionRange(2,6));
  const before=await input.boundingBox(),observed=Date.now()-10000;
  const catalog=async patch=>page.evaluate(patch=>taskq.receive({...fixture,conversations:{...fixture.conversations,...patch}}),patch);
  await catalog({complete:true,refreshing:true,observed_at:observed});
  assert.doesNotMatch(await page.locator('.feedback-message').innerText(),/refresh|catalog/i,'ordinary background refresh is quiet');
  await catalog({complete:false,loading:true,reason:'Reading conversation catalog'});
  assert.doesNotMatch(await page.locator('.conversations').innerText(),/Reading conversation catalog/,'initial in-flight read is not a warning');
  assert(await page.locator('[data-c=reply]').isDisabled(),'unknown catalog still fences Send');
  const reason='Conversation catalog observation is stale; refresh pending';
  await catalog({complete:false,loading:false,refreshing:true,reason,observed_at:observed});
  assert.equal(await page.getByText(reason,{exact:true}).count(),1,'stale diagnosis appears once');
  assert(await page.getByRole('button',{name:'Refresh conversation catalog',exact:true}).isVisible());
  assert.match(await page.locator('.catalog-observed time').getAttribute('title'),/Conversation catalog read completed/);
  await page.evaluate(()=>{window.feedbackMutations=0;new MutationObserver(rows=>feedbackMutations+=rows.length).observe(document.querySelector('.feedback-message'),{childList:true,characterData:true,subtree:true});});
  await catalog({complete:false,loading:false,refreshing:true,reason,observed_at:observed});
  assert.equal(await page.evaluate(()=>feedbackMutations),0,'unchanged polling does not repeat the live announcement');
  assert.deepEqual(await input.boundingBox(),before,'poll transitions never displace the input');
  assert.equal(await input.inputValue(),'Independent draft');assert.deepEqual(await input.evaluate(e=>[e.selectionStart,e.selectionEnd]),[2,6]);
  await catalog({complete:false,loading:false,reason:'Invented permanent read failure',observed_at:observed});
  assert.equal(await page.getByText('Invented permanent read failure',{exact:true}).count(),1,'a genuine failure stays visible during refresh');
  await catalog({complete:true,loading:false,refreshing:false,reason:''});
  assert.equal(await page.getByRole('button',{name:'Refresh conversation catalog',exact:true}).count(),0);
  assert.doesNotMatch(await page.locator('.feedback-message').innerText(),/stale|permanent read failure/);
  await page.evaluate(()=>taskq.receive({...fixture,now:1000,projects:fixture.projects.map(p=>({...p,age:1,tasks_age_seconds:40,poll:5}))}));
  assert.match(await page.locator('.conversation-status [role=status]').innerText(),/Data from \d\d:\d\d/,'board source age remains visible in the stable feedback row');
  assert.deepEqual(await input.boundingBox(),before,'board staleness never displaces the input');
 });
 await check('A1 superseded snapshots stay quiet without losing errors or source fences',async()=>{
  await bootOwners({...state,conversations:{...state.conversations,catalog_epoch:7,observed_at:Date.now(),observation_age_seconds:0}});
  await page.evaluate(()=>fixture.conversations.catalog_epoch=8);
  const input=page.locator('[data-c-field=text]');await input.fill('Keep exact local typing');await input.evaluate(e=>e.setSelectionRange(2,6));
  const catalog=async patch=>page.evaluate(patch=>taskq.receive({...fixture,conversations:{...fixture.conversations,catalog_epoch:7,items:[],...patch}}),patch);
  await catalog({complete:true,absence_complete:true});
  assert.doesNotMatch(await page.locator('.feedback-message').innerText(),/Superseded|catalog/i,'ordinary superseded reads cannot announce a warning');
  assert(await page.locator('[data-c=reply]').isDisabled(),'a quiet superseded read still fences Send');
  assert.equal(await input.inputValue(),'Keep exact local typing');assert.deepEqual(await input.evaluate(e=>[e.selectionStart,e.selectionEnd]),[2,6]);
  await page.evaluate(()=>{window.supersededAnnouncements=0;new MutationObserver(rows=>supersededAnnouncements+=rows.length).observe(document.querySelector('.feedback-message'),{childList:true,characterData:true,subtree:true});});
  await catalog({complete:true,absence_complete:true});
  assert.equal(await page.evaluate(()=>supersededAnnouncements),0,'repeated superseded snapshots do not repeat a live announcement');
  await catalog({complete:false,loading:false,reason:'Invented permanent catalog read failure',observation_age_seconds:5});
  assert.equal(await page.getByText('Invented permanent catalog read failure',{exact:true}).count(),1,'epoch replacement must not hide the real failure');
  await catalog({complete:false,loading:false,reason:'Conversation catalog observation is stale; refresh pending',observation_age_seconds:5});
  assert.equal(await page.getByRole('button',{name:'Refresh conversation catalog',exact:true}).count(),1,'aged supersession retains explicit recovery');
  await page.evaluate(()=>taskq.receive(fixture));
  assert.doesNotMatch(await page.locator('.feedback-message').innerText(),/catalog|Superseded/i);
  assert.equal(await input.inputValue(),'Keep exact local typing');assert.equal(await page.evaluate(()=>sent.filter(m=>['reply','send'].includes(m.action)).length),0);
 });
 await check('A4 selected sidebar row is discoverable after editing and explicit Refresh',async()=>{
  const rows=Array.from({length:40},(_,i)=>({...owner('owner-'+i),title:'Retained draft '+i})),selected='owner-20';
  await bootOwners({...state,conversations:{complete:true,items:rows,navigation:{selected,list:'recent'}}});
  const row=page.locator('[data-c=select][data-id='+selected+']'),input=page.locator('[data-c-field=text]');
  const clip=()=>row.evaluate(e=>{const pane=e.closest('.conversation-rows');pane.scrollTop+=e.getBoundingClientRect().bottom-pane.getBoundingClientRect().top-2;pane.dispatchEvent(new Event('scroll'));});
  const fits=()=>row.evaluate(e=>{const r=e.getBoundingClientRect(),b=e.closest('.conversation-rows').getBoundingClientRect();return r.top>=b.top-1&&r.bottom<=b.bottom+1;});
  await clip();assert.equal(await fits(),false);await input.fill('Local edit keeps this selection');
  assert(await fits(),'editing must reveal the entire selected row, not a thin strip');
  assert(await input.evaluate(e=>document.activeElement===e),'revealing the row must not steal editor focus');
  const evidence=(engine===chromium&&process.env.TASKQ_EVIDENCE_2168);if(evidence)await page.screenshot({path:evidence+'/sidebar-after-edit.png'});
  await clip();await page.getByRole('button',{name:'Refresh workspace status',exact:true}).focus();await page.getByRole('button',{name:'Refresh workspace status',exact:true}).press('Enter');
  assert(await fits(),'explicit Refresh keeps the selected row discoverable');
  assert(await page.getByRole('button',{name:'Refresh workspace status',exact:true}).evaluate(e=>document.activeElement===e),'recovery keeps the initiating control focus');
  await row.evaluate(e=>{const pane=e.closest('.conversation-rows');pane.scrollTop=pane.scrollHeight;pane.dispatchEvent(new Event('scroll'));});
  const anchor=()=>page.locator('.conversation-rows').evaluate(p=>{const top=p.getBoundingClientRect().top,n=[...p.querySelectorAll('.conversation-row')].find(e=>e.getBoundingClientRect().bottom>top);return {id:n.dataset.id,y:n.getBoundingClientRect().top-top};});
  const before=await anchor();await page.evaluate(()=>{fixture.conversations.items[0].title='Older draft label that now wraps across three lines at this narrow sidebar width';taskq.receive(fixture);});const after=await anchor();
  assert.equal(after.id,before.id);assert(Math.abs(after.y-before.y)<=2,'unrelated updates preserve deliberate sidebar reading');
  const persisted=await page.evaluate(()=>sent.filter(m=>m.a==='workspace-checkpoint').at(-1).value.conversation.context.scrolls.recent);assert(Math.abs(persisted-await page.locator('.conversation-rows').evaluate(e=>e.scrollTop))<=2,'immediate recovery retains the corrected sidebar scroll');
  assert.equal(await fits(),false,'background refresh cannot steal the manually scrolled sidebar');assert.equal(await input.inputValue(),'Local edit keeps this selection');
 });
 await check('A4 Reload and reopen reveal saved selection in a collapsed project',async()=>{
  const rows=Array.from({length:80},(_,i)=>({...owner('saved-'+i),title:'Saved draft '+i})),selected='saved-70',group=JSON.stringify([root,board]);
  const snapshot={...state,conversations:{complete:true,items:rows,sidebar:{pins:[],collapsed:[group],archived:{}},navigation:{selected,list:'recent',scrolls:{recent:0}}}};
  await bootOwners(snapshot);
  const row=page.locator('[data-c=select][data-id='+selected+']');
  const fits=()=>row.evaluate(e=>{const r=e.getBoundingClientRect(),b=e.closest('.conversation-rows').getBoundingClientRect();return r.top>=b.top-1&&r.bottom<=b.bottom+1;});
  assert(await row.isVisible(),'restored selection temporarily opens its saved collapsed group');assert(await fits(),'restored selected page fits the whole row');
  const recovery=await page.evaluate(()=>sent.filter(m=>m.a==='workspace-checkpoint').at(-1).value);
  await bootOwners(snapshot);await page.evaluate(recovery=>{taskq.receive({workspaceRecovery:recovery});taskq.receive({workspaceRecoveryReady:true});},recovery);await row.waitFor({state:'visible'});
  assert(await row.isVisible(),'safe Reload reveals the saved selected group');assert(await fits(),'safe Reload reveals its selected page');
  await page.evaluate(()=>{taskq.receive({workerPanelClosed:true});taskq.receive({open:{page:'conversations',thread:'centre'}});});
  assert(await row.isVisible());assert(await fits(),'Close/reopen keeps selection discoverable');
  if(engine===chromium&&process.env.TASKQ_EVIDENCE_2168)await page.screenshot({path:process.env.TASKQ_EVIDENCE_2168+'/sidebar-after-recovery.png'});
  assert.deepEqual(await page.evaluate(()=>fixture.conversations.sidebar.collapsed),[group],'temporary reveal keeps the saved collapse choice');
 });
 await check('A4 oversized historical disclosures retain current draft and bounded navigation',async()=>{
  const open=Object.fromEntries(Array.from({length:1800},(_,i)=>['old-thread:'+i+':'+ '界'.repeat(50),true]));
  await bootOwners({...state,workspace_navigation:{schema:1,current:{page:'conversations',open},trail:[]}});
  const input=page.locator('[data-c-field=text]');await input.focus();await input.evaluate(e=>e.setSelectionRange(2,6));
  const before=await input.boundingBox();await page.evaluate(()=>taskq.receive(fixture));
  const navigation=await page.evaluate(()=>sent.filter(m=>m.a==='workspace-set').at(-1).navigation);
  assert(new TextEncoder().encode(JSON.stringify(navigation)).length<=65536,'current navigation fits native storage');
  assert.doesNotMatch(await page.locator('.feedback-message').innerText(),/exceeds 64 KiB/);
  assert.equal(await input.inputValue(),'Independent draft');assert.deepEqual(await input.evaluate(e=>[e.selectionStart,e.selectionEnd]),[2,6]);
  assert(await input.evaluate(e=>e===document.activeElement),'refresh retains input focus');
  assert.deepEqual(await input.boundingBox(),before);assert.equal(await page.locator('[data-c=select][data-id=owner-a][aria-current=page]').count(),1);
  assert.equal(await page.evaluate(()=>sent.filter(m=>['reply','send'].includes(m.action)).length),0,'metadata trimming never launches or replays input');
 });
 for(const action of ['archive','delete','fork'])await check('Q-A8 '+action+' completion follows resulting selection',async()=>{
  await bootOwners();
  if(action==='delete'){
   await tool('archive');
   await page.evaluate(()=>{const m=sent.find(m=>m.action==='archive');fixture.conversations.items[0].archived=true;taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:fixture.conversations}});});
   await page.evaluate(()=>{fixture.conversations.items[1].archived=true;taskq.receive(fixture);});
   await page.locator('[data-c=archive-list]').click();await page.locator('[data-c=select][data-id=owner-a]').click();
   await tool('delete-review');await page.locator('[data-c=delete]').click();
  }else await tool(action==='fork'?'from-draft':action);
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
  await tool('archive');await page.waitForFunction(()=>sent.some(m=>m.action==='archive'));
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
}finally{await browser.close();}}})().catch(e=>{console.error(e);process.exitCode=1;});
