// #1665 D2-D7 workspace dialogue: actual bundle with invented native receipts. Rust tests exercise the real turn, reader and watch.
// Defect class: a launched conversation without a reply path, a transcript shown empty when unreadable, a refusal that drops the draft.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const evidence=process.env.TASKQ_EVIDENCE_1665;
const draft=(id,runtime,state)=>({id,revision:1,title:runtime+' session',text:'',root:'fixture-alpha',caret:[0,0],scroll:0,files:[],archived:false,
 sent:{text:'Start',root:'fixture-alpha',runtime,files:[]},request:{id,state:'launched',kind:'launched',url:'https://example.org/'+runtime+'/'+id,next:'Reply here'},dialogue:{state,watch:'no watch',turn:null}});
const snapshot={font:16,theme:'dark',host:'fixture-host',projects:[{root:'fixture-alpha',name:'Alpha',alive:true,complete:true,issues:'https://example.org/alpha/issues',tasks:[]}],ai:{history:[],runtime:{id:'claude',name:'Claude Code',available:true}},
 conversations:{complete:true,items:[draft('claude-1665','claude','idle'),draft('codex-1665','codex','unknown')],navigation:{selected:'claude-1665',list:'recent'}},messages:{complete:true,entries:[]}};
// 60 invented records; the reader returns chunks of 20 from the end like D4.
const records=Array.from({length:60},(_,i)=>i%3===2?{role:'tool',text:'Bash: taskq status --line '+i}:{role:i%3?'assistant':'user',text:(i%3?'Answer ':'Question ')+i});
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:900,height:720}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  const html=fs.readFileSync(process.env.TASKQ_CONVERSATION_HTML||'build/web/workspace.html','utf8');
  const state=structuredClone(snapshot),log=structuredClone(records),status={'claude-1665':{state:'idle',watch:'no watch',turn:{state:'done',at:Date.UTC(2026,9,23,9),watch:false,outcome:'done'},last_line:'Answer 58'}};
  let unreadable=new Set(['codex-1665']);
  await page.exposeFunction('fixtureRead',async m=>{
   let r;
   if(unreadable.has(m.conversation))r={state:'unknown',reason:m.action==='status'?'native holder unavailable':'transcript unreadable'};
   else if(m.action==='status')r={state:'accepted',value:status[m.conversation]};
   else {const end=m.after!==undefined?log.length:m.before??log.length,start=m.after!==undefined?m.after:Math.max(0,end-20);r={state:'accepted',value:{items:log.slice(start,end),start,end}};}
   await page.evaluate(r=>taskq.receive({dialogueReceipt:r}),{id:m.id,conversation:m.conversation,action:m.action,...r});
  });
  await page.exposeFunction('fixtureSave',async m=>{const c=state.conversations.items.find(c=>c.id===m.conversation);Object.assign(c,m.draft);c.revision++;await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:m.id,state:'accepted',snapshot:state.conversations});});
  await page.setContent(html);
  await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='conversation-dialogue')window.fixtureRead(m);if(m.a==='conversation-action'&&m.action==='save')window.fixtureSave(m);}};});
  await page.evaluate(s=>taskq.receive(s),state);await page.evaluate(()=>taskq.receive({open:{page:'conversations'}}));
  async function action(name){await page.waitForFunction(n=>sent.some(m=>m.action===n),name);return page.evaluate(n=>{const m=sent.filter(m=>m.action===n).at(-1);sent.splice(sent.indexOf(m),1);return m;},name);}
  async function receipt(m,r){const c=state.conversations.items.find(c=>c.id===m.conversation);c.revision++;await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:m.id,snapshot:state.conversations,...r});}
  async function capture(name){if(evidence){fs.mkdirSync(evidence,{recursive:true});await page.screenshot({path:path.join(evidence,name+'.png')});}}
  const view=page.locator('.dialogue-transcript'),detail=page.locator('.conversation-detail'),input=page.locator('[data-c-field=text]'),texts=()=>view.locator('.dialogue-item .conversation-text, .dialogue-tool').allInnerTexts();

  // A2/D4: the newest chunk in order, tool calls as one line, newest at the bottom.
  await page.waitForFunction(()=>document.querySelectorAll('.dialogue-transcript .dialogue-item').length>0);
  assert.deepEqual(await texts(),records.slice(40).map(r=>r.role==='tool'?'Tool · '+r.text:r.text),'last chunk in file order');
  assert.equal(await view.locator('.dialogue-tool').first().evaluate(e=>getComputedStyle(e).whiteSpace),'nowrap','a tool call is one line');
  assert(await detail.evaluate(e=>e.scrollHeight-e.scrollTop-e.clientHeight<2),'opens at the newest record');
  assert.equal(await page.locator('.conversation-record').count(),0,'a launched conversation shows its transcript, not the request record');
  // Scrolling up loads the previous chunk without jumping away from the reading position; Load earlier reaches the first record.
  const top=await view.locator('.dialogue-item').first().innerText();await detail.evaluate(e=>{e.scrollTop=0;e.dispatchEvent(new Event('scroll',{bubbles:true}));});
  await page.waitForFunction(()=>document.querySelectorAll('.dialogue-transcript .dialogue-item, .dialogue-transcript .dialogue-tool').length===40);
  assert(await detail.evaluate(e=>e.scrollTop>0),'prepend keeps the reading anchor');assert.match(top,/Answer 40/);
  await page.locator('[data-c=earlier]').click();await page.waitForFunction(()=>document.querySelectorAll('.dialogue-transcript .dialogue-item, .dialogue-transcript .dialogue-tool').length===60);
  assert.equal(await page.locator('[data-c=earlier]').count(),0,'first record reached');assert.deepEqual((await texts())[0],'Question 0');
  // New records append while the view is open (2 s poll).
  await detail.evaluate(e=>{e.scrollTop=e.scrollHeight;e.dispatchEvent(new Event('scroll',{bubbles:true}));});
  log.push({role:'user',text:'Question 60'},{role:'assistant',text:'Answer 61'});
  await page.waitForFunction(()=>[...document.querySelectorAll('.dialogue-transcript .conversation-text')].at(-1)?.innerText==='Answer 61',null,{timeout:8000});
  assert(await detail.evaluate(e=>e.scrollHeight-e.scrollTop-e.clientHeight<2),'a reader at the bottom follows new records');
  // A3/D5: state comes from the live status read.
  assert.match(await page.locator('.dialogue-state').innerText(),/State: idle/);
  await capture('transcript-dark-16');

  // A4/D7: no watch reads `no watch`; set, edit and remove it from Status.
  const details=page.locator('.dialogue-status');assert.match(await details.locator('summary').innerText(),/Status · no watch/);
  await details.locator('summary').click();assert.match(await details.innerText(),/Watch\s+no watch/);assert.match(await details.innerText(),/Last assistant line\s+Answer 58/);
  assert(await page.locator('[data-c=watch]').isDisabled(),'a watch needs a prompt');
  await page.locator('[data-w-field=prompt]').fill('Check the board as the project rules say');await page.locator('[data-w-field=every]').fill('3');
  assert(await page.locator('[data-c=watch]').isDisabled(),'every is at least 5 minutes');
  await page.locator('[data-w-field=every]').fill('5');await page.locator('[data-w-field=event][value=ask]').check();await page.locator('[data-w-field=event][value=close]').check();
  await page.locator('[data-c=watch]').click();let m=await action('watch');
  assert.deepEqual([m.prompt,m.every,m.events],['Check the board as the project rules say',5,['close','ask']],'events keep D6 order');
  const watch={prompt:m.prompt,every:5,events:['close','ask'],last_turn:null,last_outcome:null,next_due:Date.UTC(2026,9,23,10,5),seen:{}};
  status['claude-1665']={...status['claude-1665'],watch};await receipt(m,{state:'accepted',value:watch});
  for(const text of [/Status · watch every 5 min/,/Prompt\s+Check the board/,/Every\s+5 minutes/,/Events\s+close, ask/,/Last turn\s+none/,/Next turn\s+\S/,/Last outcome\s+none/])assert.match(await details.innerText(),text);
  await page.locator('[data-w-field=every]').fill('15');await page.locator('[data-c=watch]').click();m=await action('watch');assert.equal(m.every,15,'Save watch edits the same watch');
  await receipt(m,{state:'unknown',reason:'every must be at least 5 minutes'});assert.match(await page.locator('.conversation-status').innerText(),/every must be at least 5 minutes/);
  await capture('status-watch-dark-16');
  await page.locator('[data-c=unwatch]').click();m=await action('unwatch');status['claude-1665']={...status['claude-1665'],watch:'no watch'};await receipt(m,{state:'accepted'});
  assert.match(await details.locator('summary').innerText(),/no watch/);assert.equal(await page.locator('[data-c=unwatch]').count(),0);

  // A1/D2/D3: Send continues the session; working and open-in refusals show their text and keep the draft.
  await input.fill('Is the build green?');await input.press('Enter');m=await action('reply');
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='send').length),0,'a launched item never starts a new launch');
  await receipt(m,{state:'unknown',reason:'working'});assert.match(await page.locator('.conversation-status').innerText(),/Not sent: working\. Draft kept/);assert.equal(await input.inputValue(),'Is the build green?');
  await page.locator('[data-c=reply]').click();m=await action('reply');
  await receipt(m,{state:'unknown',reason:'open in claude; close it there first'});
  assert.match(await page.locator('.conversation-status').innerText(),/Not sent: open in claude; close it there first\. Draft kept/);assert.equal(await input.inputValue(),'Is the build green?');
  status['claude-1665']={...status['claude-1665'],state:'open in claude'};await page.locator('[data-c=status-refresh]').click();
  await page.waitForFunction(()=>/open in claude/.test(document.querySelector('.dialogue-state').innerText));assert.equal(await page.locator('.dialogue-state').innerText(),'State: open in claude');
  await capture('refused-dark-16');
  status['claude-1665']={...status['claude-1665'],state:'idle'};await page.locator('[data-c=reply]').click();m=await action('reply');
  assert(await page.locator('[data-c=reply]').isDisabled(),'one turn at a time from the workspace');await input.press('Enter');assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='reply').length),0,'Enter cannot send a second turn');
  status['claude-1665']={...status['claude-1665'],state:'working'};await receipt(m,{state:'accepted'});
  await page.waitForFunction(()=>document.querySelector('[data-c-field=text]').value==='');assert.match(await page.locator('.conversation-status').innerText(),/Sent as one native turn/);
  await page.waitForFunction(()=>/State: working/.test(document.querySelector('.dialogue-state').innerText));
  // The reply itself comes only from the transcript.
  log.push({role:'user',text:'Is the build green?'},{role:'assistant',text:'Yes, all checks pass.'});
  await page.waitForFunction(()=>[...document.querySelectorAll('.dialogue-transcript .conversation-text')].at(-1)?.innerText==='Yes, all checks pass.',null,{timeout:8000});

  // A3/D5 unknown: an unreadable Codex session never shows as empty or done.
  state.theme='light';await page.evaluate(s=>taskq.receive(s),state);
  await page.locator('[data-c=select][data-id=codex-1665]').click();
  await page.waitForFunction(()=>/Transcript unknown/.test(document.querySelector('.dialogue-transcript')?.innerText||''));
  assert.match(await view.innerText(),/Transcript unknown: transcript unreadable/);assert.doesNotMatch(await view.innerText(),/No messages/);
  await page.waitForFunction(()=>/Status unknown/.test(document.querySelector('.dialogue-status summary').innerText));
  assert.equal(await page.locator('.dialogue-state').innerText(),'State: unknown');assert.match(await page.locator('[data-c=select][data-id=codex-1665]').innerText(),/unknown/);
  await capture('codex-unknown-light-16');
  unreadable.delete('codex-1665');status['codex-1665']={state:'idle',watch:'no watch',turn:null,last_line:'Codex answer'};
  await page.waitForFunction(()=>document.querySelectorAll('.dialogue-transcript .dialogue-item').length>0,null,{timeout:8000});
  // The same D2/D3/D7 path for a Codex session.
  await input.fill('Codex reply');await page.locator('[data-c=reply]').click();m=await action('reply');assert.equal(m.conversation,'codex-1665');
  await receipt(m,{state:'unknown',reason:'open in codex; close it there first'});
  assert.match(await page.locator('.conversation-status').innerText(),/Not sent: open in codex; close it there first\. Draft kept/);assert.equal(await input.inputValue(),'Codex reply');
  await page.locator('[data-c=reply]').click();m=await action('reply');await receipt(m,{state:'accepted'});await page.waitForFunction(()=>document.querySelector('[data-c-field=text]').value==='');
  if(!await details.evaluate(e=>e.open))await details.locator('summary').click();
  await page.locator('[data-w-field=prompt]').fill('Report the board');await page.locator('[data-w-field=every]').fill('5');await page.locator('[data-w-field=event][value=ask]').check();
  await page.locator('[data-c=watch]').click();m=await action('watch');assert.deepEqual([m.conversation,m.prompt,m.every,m.events],['codex-1665','Report the board',5,['ask']]);
  status['codex-1665']={...status['codex-1665'],watch:{prompt:m.prompt,every:5,events:['ask'],last_turn:null,last_outcome:null,next_due:null,seen:{}}};await receipt(m,{state:'accepted',value:status['codex-1665'].watch});
  assert.match(await details.locator('summary').innerText(),/watch every 5 min/);
  await page.setViewportSize({width:350,height:560});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'no horizontal page scrolling');
  assert(await page.locator('[data-c=reply]').isVisible());assert(await view.isVisible());
  await capture('narrow-light-16');
  // #1699 A5/A6: diagnostics filled the detail and the composer read 'Session runtime unknown' for a claude handoff.
  // Risk kept: at 18–20 px the transcript keeps the reading area, Send stays reachable with a long reply, copy names the recorded runtime.
  for(const font of [18,20]){
   await page.setViewportSize({width:900,height:720});await page.evaluate(s=>taskq.receive(s),{...state,font});
   await page.locator('[data-c=select][data-id=claude-1665]').click();await page.waitForFunction(()=>document.querySelector('.dialogue-transcript .dialogue-item'));
   assert.equal(await page.locator('.dialogue-launch').evaluate(d=>d.open),false,'launch diagnostics start collapsed');
   const composer=await page.locator('.conversation-composer').innerText();
   assert.match(composer,/Session claude/);assert.doesNotMatch(composer,/runtime unknown|not recorded/);
   assert.equal(await input.getAttribute('placeholder'),'Reply to the claude session');
   await input.fill('Long reply line. '.repeat(200));assert.match(await page.locator('.conversation-composer').innerText(),/Send goes to this claude session from this window/);
   const [t,b,r]=await Promise.all([view,page.locator('.dialogue-bar'),page.locator('[data-c=reply]')].map(l=>l.boundingBox()));
   assert(t.height>b.height,'the transcript takes more height than the status bar at '+font+' px');
   await page.locator('[data-c=reply]').scrollIntoViewIfNeeded();
   const send=await page.locator('[data-c=reply]').boundingBox();assert(send.y>=0&&send.y+send.height<=720,'Send is reachable in the one scroll at '+font+' px');
   await input.fill('');
  }
  assert.deepEqual(errors,[]);
  console.log('PASS #1665: D2/D3 reply and refusals keep the draft, D4 ordered transcript with infinite scroll and live append, D5 states incl. unknown, D7 watch set/edit/remove, narrow layout');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
