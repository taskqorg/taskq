// #1723 A2-A6: mixed selections, archived transcript and one reading scroll; invented native receipts, no live writes.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.TASKQ_EVIDENCE_1723;if(out)fs.mkdirSync(out,{recursive:true});
const board='https://example.org/alpha/issues',event='e'.repeat(64),fx=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json'));
const question=process.env.TASKQ_QUESTION_1723?JSON.parse(fs.readFileSync(process.env.TASKQ_QUESTION_1723)):structuredClone(fx.cases.find(c=>c.name==='choice').rows[0]);
// Optional local source evidence is never committed; the persistent fixture stresses long geometry with invented text.
if(!process.env.TASKQ_QUESTION_1723)question.question.problem=('A long question with alternatives, compatibility and retained owner context. ').repeat(45);
if(!process.env.TASKQ_QUESTION_1723)question.title='One identity for a long question';
const owner=(id,archived)=>({id,archived,title:id,revision:1,root:'fixture',text:'Independent '+id+' draft',caret:[2,4],scroll:0,files:[],sent:{runtime:'claude',text:'Original request',files:[]},request:{state:'launched',kind:'launched',runtime:'claude',id,url:'https://claude.ai/code/session_invented1723'}});
const state={font:20,theme:'dark',host:'fixture',projects:[{root:'fixture',name:'Alpha',issues:board,alive:true,complete:true,tasks:[question]}],ai:{history:[],runtime:{id:'claude',available:true}},conversations:{complete:true,items:[owner('active-owner',false),owner('archived-owner',true)],navigation:{selected:'active-owner',list:'recent'}},messages:{complete:true,entries:[{key:event,id:'orphan',kind:'request',title:'Archived event',archived:true,read:true,state:'launched',request:{next:'Original event'}}]}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:940,height:740}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.exposeFunction('read',async m=>page.evaluate(r=>taskq.receive({dialogueReceipt:r}),{id:m.id,conversation:m.conversation,state:'accepted',value:m.action==='status'?{state:'idle',watch:{prompt:'Long watch prompt. '.repeat(250),every:30,events:[]},last_line:'Last native answer'}:{items:[{role:'user',text:'Transcript question'},{role:'assistant',text:'Native transcript answer'}],start:0,end:80}}));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-dialogue')window.read(m);}};});
 await page.evaluate(s=>taskq.receive(s),state);
 const current=()=>page.locator('.conversation-row[aria-current=page]').innerText(),detail=page.locator('.conversation-detail');
 await page.locator('.dialogue-item').first().waitFor();
 await page.locator('[data-c=archive-list]').click();assert.equal(await page.locator('.dialogue-transcript').count(),0,'Archive starts empty instead of showing a Recent owner');
 await page.locator('[data-c=select][data-id=archived-owner]').click();await page.locator('.dialogue-item').first().waitFor();
 assert.match(await detail.innerText(),/Native transcript answer/);assert.match(await detail.innerText(),/Archived in TaskQ only[^]*Restore before reply/);
 assert.equal(await page.locator('[data-c-field=text]').count(),0,'archived transcript has no reply composer');assert.equal(await page.locator('[data-c=restore]').isEnabled(),true);
 assert.equal(await page.locator('.dialogue-launch').evaluate(e=>e.open),false,'native identity starts in Details');
 assert.doesNotMatch(await detail.innerText(),/session_invented1723|Replies open here after|archive it in Claude/);
 if(out)await page.screenshot({path:out+'/archived-dark-20.png'});
 await page.locator('[data-c=recent]').click();assert.match(await current(),/active-owner/);assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Independent active-owner draft');
 await page.locator('[data-c=archive-list]').click();assert.match(await current(),/archived-owner/,'Archive restores its owner selection');
 await page.locator('[data-thread="event:'+event+'"]').click();await page.locator('[data-c=recent]').click();assert.match(await current(),/active-owner/);
 await page.locator('[data-c=archive-list]').click();assert.match(await current(),/Archived event/,'Archive also restores event selection');
 await page.evaluate(b=>taskq.receive({open:{page:'conversations',board:b,task:1}}),board);
 await page.locator('[data-c=archive-list]').click();assert.match(await current(),/Archived event/);await page.locator('[data-c=recent]').click();assert.match(await current(),/#1/,'Recent restores a task');
 // A4: source IDs/revision remain, one visible identity heading, choices and composer reached through the same scroll.
 await page.evaluate(({board,n})=>taskq.receive({open:{page:'conversations',board,task:n}}),{board,n:question.n});
 assert.equal(await page.locator('.thread-title').count(),1);assert.equal(await page.locator('.qac-head').isVisible(),false);
 assert.deepEqual(await page.locator('[data-qac=choose]').evaluateAll(es=>es.map(e=>e.textContent.trim().match(/^\d+/)[0])),question.question.choices.map(c=>String(c.id)));
 for(const font of [18,20]){await page.evaluate(({s,font})=>taskq.receive({...s,font}),{s:state,font});await page.locator('[data-c-field=answer]').scrollIntoViewIfNeeded();const box=await page.locator('[data-c-field=answer]').boundingBox();assert(box.y>=0&&box.y+box.height<=740,'long question composer reached at '+font);}
 if(out)await page.screenshot({path:out+'/question-dark-20.png'});
 await page.locator('[data-c-field=answer]').fill('Retained answer draft');
 state.projects[0].tasks[0]={...question,revision:question.revision+2};await page.evaluate(s=>taskq.receive(s),state);
 assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'stale','fresh revision cannot silently approve a retained draft');
 assert.equal(await page.locator('[data-c-field=answer]').inputValue(),'Retained answer draft');assert(await page.locator('[data-a=answer-send]').isDisabled());assert(await page.locator('[data-qac=choose]').first().isDisabled());
 await page.locator('[data-c=select][data-id=active-owner]').click();await page.locator('.dialogue-status summary').click();
 for(const font of [18,20]){
  await page.evaluate(({s,font})=>taskq.receive({...s,font}),{s:state,font});
  if(!await page.locator('.dialogue-status').evaluate(e=>e.open))await page.locator('.dialogue-status summary').click();
  const nested=await detail.evaluate(e=>[...e.querySelectorAll('.dialogue-bar,.dialogue-transcript,.conversation-composer')].filter(n=>['auto','scroll'].includes(getComputedStyle(n).overflowY)).length);assert.equal(nested,0,'no nested thread-content scroll at '+font);
  await page.locator('[data-c=reply]').scrollIntoViewIfNeeded();const send=await page.locator('[data-c=reply]').boundingBox();assert(send.y>=0&&send.y+send.height<=740,'composer reached at '+font);
  const before=await detail.evaluate(e=>e.scrollTop);await page.evaluate(({s,font})=>taskq.receive({...s,font}),{s:state,font});assert(Math.abs(await detail.evaluate(e=>e.scrollTop)-before)<2,'snapshot keeps reading position '+JSON.stringify({before,after:await detail.evaluate(e=>({top:e.scrollTop,height:e.scrollHeight,client:e.clientHeight})),status:await page.locator('.dialogue-status').evaluate(e=>e.open)}));
  if(out)await page.screenshot({path:out+'/expanded-status-dark-'+font+'.png'});
 }
 assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='message-action'||m.a==='conversation-action').length),0,'navigation/reading never sends, acknowledges or saves an owner record');
 // An explicit project root must survive unified selection when the board has two registered roots.
 await page.evaluate(({s,n,board})=>{taskq.receive({...s,projects:[{...s.projects[0],root:'other-root',name:'Other',alive:false},s.projects[0]]});taskq.receive({open:{page:'conversations',board,root:s.projects[0].root,task:n}});},{s:state,n:question.n,board});
 assert.match(await page.locator('.thread-title').innerText(),/^Alpha/,'task navigation retains the requested project root');
 // A2: the actual menu detail dispatcher exposes one entry for each task state, without lifecycle actions.
 const menu=await browser.newPage({viewport:{width:800,height:740}});menu.on('pageerror',e=>errors.push(e.message));
 await menu.setContent(fs.readFileSync('build/web/tray.html','utf8'));await menu.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};});
 await menu.evaluate(s=>taskq.receive(s),{...state,page:'home',platform:'macos',display:require('./fixtures/display-model.json').display,projects:[{...state.projects[0],board_kind:'github',tasks:['later','doing','review','ask'].map((state,i)=>({n:i+1,state,title:'Menu task '+(i+1),revision:i+10,controls:[]}))}]});
 for(const n of [1,2,3,4]){
  await menu.locator('[data-a=task-detail][data-key="'+board+'/'+n+'"]').click();
  const entry=menu.locator('#taskq-popover [data-a=workspace]');assert.equal(await entry.count(),1,'one thread entry per task');
  await entry.click();const open=await menu.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open);assert.deepEqual([open.board,open.task,open.revision],[board,n,n+9]);await menu.keyboard.press('Escape');
 }
 assert.equal(await menu.evaluate(()=>sent.filter(m=>['answer','control','start','ai-send'].includes(m.a)).length),0,'opening a menu task has no lifecycle effect');await menu.close();
 assert.deepEqual(errors,[]);console.log('PASS #1723: mixed Recent/Archive selections both directions, read-only archived D4 transcript, one heading/source choices, one status/transcript/composer scroll at 18/20 px, position and drafts preserved; no task or owner writes');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
