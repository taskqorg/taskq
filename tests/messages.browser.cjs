// #1699 A2/A3/A6/A7: one Conversations thread screen; actual bundled renderer, invented snapshots and receipts; no live effects.
// #1691 kept Inbox as a section beside the thread list, so a task question and its notification lived apart from the conversation.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const evidence=process.env.TASKQ_EVIDENCE_1699;if(evidence)fs.mkdirSync(evidence,{recursive:true});
const fx=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json','utf8')),row=fx.cases.find(c=>c.name==='choice').rows[0];
const q='1'.repeat(64),r='2'.repeat(64),board='https://github.com/o/r/issues';
const entries=[
 {key:q,kind:'question',title:row.title,project:'Fixture',root:'fixture',board,task:row.n,revision:row.revision,state:'ask',pending:true,read:false,archived:false},
 {key:r,kind:'request',title:'Setup request · launched',root:'fixture',host:'fixture-host',id:'native-live',state:'launched',pending:true,read:false,archived:false,request:{next:'Read replies in the native runtime.',url:'https://claude.ai/code/session_Invented123'}}
];
const owner={id:'owner-1',revision:1,title:'Owner plan',text:'owner draft text',root:'fixture',caret:[3,3],scroll:0,files:[],archived:false};
const snapshot={font:16,theme:'dark',host:'fixture-host',now:1000,projects:[{root:'fixture',name:'Fixture',issues:board,alive:true,complete:true,age:1,tasks_age_seconds:1,focus:[],labels:[],tasks:[row,{n:900,title:'Plain running task',state:'doing',revision:1}]}],
 messages:{entries,count:2,complete:true,reason:''},conversations:{complete:true,items:[owner],navigation:{selected:'owner-1',list:'recent'}},ai:{draft:'',history:[],runtime:{available:true,name:'Claude Code',id:'claude'}},workspace_local:{}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try {
  const page=await browser.newPage({viewport:{width:900,height:680}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  const state=structuredClone(snapshot);
  await page.exposeFunction('fixtureSave',async m=>{const c=state.conversations.items.find(c=>c.id===m.conversation);if(c){Object.assign(c,m.draft);c.revision++;}await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:m.id,state:'accepted',snapshot:state.conversations});});
  async function boot(extra={}){
   await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-action'&&m.action==='save')window.fixtureSave(m);}};});
   await page.evaluate(s=>taskq.receive(s),{...state,...extra});
  }
  const effects=()=>page.evaluate(()=>sent.filter(m=>['message-action','answer','ai-send'].includes(m.a)||m.a==='conversation-action'&&m.action==='send').length);
  const current=()=>page.locator('.conversation-row[aria-current=page]').innerText();
  await boot();
  // Menu badge: the list stays visible, the open question's thread is selected, nothing is acknowledged
  await page.evaluate(()=>taskq.receive({open:{page:'conversations',thread:'unread'}}));
  assert.equal(await page.locator('h1').innerText(),'Conversations');
  assert(await page.locator('.conversation-list').isVisible(),'the thread list stays visible');
  assert.match(await current(),new RegExp('#'+row.n+' [^]*Needs reply'),'the badge selects the task thread that needs a reply');
  assert.equal(await page.locator('[data-section]').count(),0,'no Inbox section');
  assert.equal(await effects(),0,'opening acknowledges and sends nothing');
  // Numbered answer: the question card sits in its task thread with its choice ids, order and recommendation
  const card=page.locator('.thread-detail [data-answer]');await card.waitFor();
  const choices=await card.locator('[data-qac=choose]').allInnerTexts();
  assert.equal(choices.length,2);assert.match(choices[0],/^1\.\s+Keep compatibility/);assert.match(choices[1],/^2\.\s+Remove the old API/);
  assert.match(await card.innerText(),/★|Recommended/,'the recommendation shows');
  // Ids leave the row and stay in Details
  assert.doesNotMatch(await current(),/revision|attempt/);assert.equal(await page.locator('.thread-detail details.ids').evaluate(d=>d.open),false);
  // Switch: the task answer draft and the owner draft each survive switching threads
  const answer=page.locator('[data-c-field=answer]');await answer.fill('Typed answer draft');
  await page.locator('[data-c=select][data-id=owner-1]').click();
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),'owner draft text');
  await page.locator('[data-c-field=text]').fill('owner draft text, edited');
  await page.locator('[data-a=thread][data-thread="task:'+board+'/'+row.n+'"]').click();
  assert.equal(await answer.inputValue(),'Typed answer draft','the task draft survives a switch');
  await page.locator('[data-c=select][data-id=owner-1]').click();
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),'owner draft text, edited','the owner draft survives a switch');
  // Task without notification: its own thread, no duplicate row, no launch
  const rows=await page.locator('.conversation-row').count();
  await page.evaluate(b=>taskq.receive({open:{page:'conversations',board:b,task:900}}),board);
  assert.match(await current(),/#900 Plain running task/);assert.equal(await page.locator('.conversation-row').count(),rows+1);
  await page.evaluate(b=>taskq.receive({open:{page:'conversations',board:b,task:900}}),board);
  assert.equal(await page.locator('.conversation-row').count(),rows+1,'reopening adds no duplicate thread');
  assert.match(await page.locator('.thread-detail').innerText(),/There is no open question/);
  // Event thread: Mark read is explicit and its receipt stays in that thread only
  await page.locator('[data-a=thread][data-thread="event:'+r+'"]').click();
  assert.match(await current(),/Setup request[^]*Unread/);assert.equal(await effects(),0,'selecting a thread acknowledges nothing');
  await page.locator('.thread-detail [data-a=message-read]').click();
  const read=await page.evaluate(()=>sent.filter(m=>m.a==='message-action').at(-1));assert.equal(read.action,'read');assert.deepEqual(read.keys,[r]);
  await page.evaluate(({id,key})=>taskq.receive({messageReceipt:{id,outcomes:[{key,action:'read',state:'accepted'}]}}),{id:read.id,key:r});
  assert.match(await page.locator('.thread-detail .thread-notice').innerText(),/Message marked read/);
  assert.doesNotMatch(await page.locator('[data-status=workspace]').innerText(),/marked read/,'the shared status line never carries it');
  await page.locator('[data-c=select][data-id=owner-1]').click();assert.doesNotMatch(await page.locator('.content').innerText(),/Message marked read/,'other threads never show it');
  // Archive, Undo and Restore: the list stays, Undo restores and opens the thread
  await page.locator('[data-a=thread][data-thread="event:'+r+'"]').click();
  await page.locator('.thread-detail [data-a=message-archive]').click();
  const archive=await page.evaluate(()=>sent.filter(m=>m.a==='message-action').at(-1));assert.equal(archive.action,'archive');
  state.messages.entries[1]={...state.messages.entries[1],read:true,archived:true};state.messages.count=1;
  await page.evaluate(({id,key})=>taskq.receive({messageReceipt:{id,outcomes:[{key,action:'archive',state:'accepted'}]}}),{id:archive.id,key:r});
  await page.evaluate(s=>taskq.receive(s),state);
  assert(await page.locator('.conversation-list').isVisible());assert.equal(await page.locator('[data-thread="event:'+r+'"]').count(),0,'Recent drops the archived thread');
  assert.match(await page.locator('.content').innerText(),/Archived/);
  await page.locator('[data-a=message-restore]').filter({hasText:'Undo archive'}).click();
  const undo=await page.evaluate(()=>sent.filter(m=>m.a==='message-action').at(-1));assert.equal(undo.action,'restore');assert.deepEqual(undo.keys,[r]);
  state.messages.entries[1]={...state.messages.entries[1],archived:false};
  await page.evaluate(s=>taskq.receive(s),state);
  await page.evaluate(({id,key})=>taskq.receive({messageReceipt:{id,outcomes:[{key,action:'restore',state:'accepted'}]}}),{id:undo.id,key:r});
  assert.match(await current(),/Setup request/,'Undo opens the restored thread');
  if(evidence)await page.screenshot({path:evidence+'/thread-dark-16.png'});
  // Restart: the selected thread and the task draft come back; nothing is replayed
  await page.locator('[data-a=thread][data-thread="task:'+board+'/'+row.n+'"]').click();
  const saved=await page.evaluate(()=>({navigation:sent.filter(m=>m.a==='workspace-set').at(-1).navigation,local:Object.fromEntries(sent.filter(m=>m.a==='answer-local').map(m=>[m.key,m.local]))}));
  await boot({workspace_navigation:saved.navigation,workspace_local:saved.local});
  assert.match(await current(),new RegExp('#'+row.n),'restart keeps the selected thread');
  assert.equal(await page.locator('[data-c-field=answer]').inputValue(),'Typed answer draft','restart keeps the task draft');
  assert.equal(await effects(),0,'rehydration replays no effects');
  // Narrow, large text: one column, no horizontal page scroll
  await page.setViewportSize({width:360,height:640});await page.evaluate(s=>taskq.receive({...s,theme:'light',font:20}),state);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'narrow view has no horizontal clipping');
  if(evidence)await page.screenshot({path:evidence+'/thread-light-20-360.png'});
  assert.deepEqual(errors,[]);
  console.log('PASS #1699: badge opens the unread task thread without acknowledging, numbered card in thread, per-thread drafts across switches and restart, task without notification, in-thread Mark read receipt, Archive/Undo/Restore, narrow layout');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
