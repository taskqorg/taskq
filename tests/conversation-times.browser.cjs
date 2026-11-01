// #1930 UX30-17..19 / A1–A6: actual production bundles, invented clocks/receipts. No live effects.
'use strict';
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium,webkit}=require('playwright');
const html=fs.readFileSync(process.env.TASKQ_TIMES_HTML||'build/web/workspace.html','utf8'),out=process.env.TASKQ_TIMES_EVIDENCE;
const now=Date.parse('2026-10-31T12:00:59Z'),iso='2026-10-31T12:00:30.123Z',board='https://example.org/alpha/issues',root='fixture';
const records=[{role:'user',text:'Owner request',updated:iso},{role:'assistant',text:'Worker reply',updated:iso},{role:'tool',tool_name:'Read',text:'Read source.rs',updated:iso},{role:'tool',tool_name:'Bash',text:'Bash cargo test',updated:null},{role:'brief',text:'Native brief',updated:iso},{role:'assistant',text:'Zone-less record',updated:'2026-10-31T12:00:00'},...Array.from({length:25},(_,i)=>({role:'assistant',text:'Reading anchor '+i+'\nbody\nbody\nbody',updated:iso}))].map((r,i)=>({...r,at:1000000+i*100,i:0}));
const owner=(id,runtime,archived=false)=>({id,title:runtime+' '+id,revision:1,root,text:'',caret:[0,0],scroll:0,files:[],archived,updated:iso,sent:{root,runtime,text:'Work request',files:[]},request:{state:'launched',runtime,created:now-3600000},dialogue:{state:'idle'}});
const question=require('./fixtures/answer-card.json').cases.find(c=>c.name==='choice').rows[0];
const task={...question,since:(now-60000)/1000,updated:(now-60000)/1000,role:'worker',attempt:20,host:'remote-host',runtime:'codex',executions:[{attempt:20,state:'running'}],conversation:{id:'owner',brief:'Recorded task brief',verdict:'Recorded result'},history:[{question:question.question,answered:{by:'owner',text:'Earlier answer'}}]};
const state={font:16,theme:'dark',host:'fixture',projects:[{root,name:'Alpha',issues:board,alive:true,complete:true,tasks:[task]}],ai:{history:[],runtime:{id:'claude',available:true}},conversations:{complete:true,items:[owner('owner','claude'),owner('codex-owner','codex'),owner('archive','claude',true)],navigation:{selected:'owner',list:'recent'}},messages:{complete:true,entries:[{kind:'request',key:'a'.repeat(64),id:'event-only',title:'Request event',root,created:now-86400000,state:'failed',request:{reason:'Invented failure'}}]}};
(async()=>{if(out)fs.mkdirSync(out,{recursive:true});for(const [engine,type]of [['chromium',chromium],['webkit',webkit]]){
 const browser=await type.launch({...require('./browser.cjs')(),...(engine==='webkit'&&process.env.TASKQ_WEBKIT_PATH?{executablePath:process.env.TASKQ_WEBKIT_PATH}:{})});try{
  const page=await browser.newPage({viewport:{width:940,height:740},timezoneId:'Asia/Bangkok'}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.clock.install({time:now});await page.clock.pauseAt(now+1);await page.setContent(html);
  await page.evaluate(({state,records,now,iso})=>{
   window.state=state;window.records=records;window.sent=[];window.ipc={postMessage:text=>{
    const m=JSON.parse(text);sent.push(m);if(m.a!=='conversation-dialogue')return;
    if(m.action==='status'&&window.failStatus){queueMicrotask(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'unknown',reason:'Invented native source unavailable'}}));return;}
    // A source reread after eviction starts at a real byte cursor, independently of source timestamps.
    const items=m.after!==undefined?records.filter(r=>r.at>=m.after):m.before!==undefined?records.filter(r=>r.at<m.before):records,size=records.at(-1).at+100;
    const value=m.action==='status'?{state:'idle',updated:iso,watch:{prompt:'Check board',every:5,events:['ask'],last_turn:now-3600000,next_due:now+3600001},turn:{at:now-60000,state:'done',outcome:'unknown'}}:m.action==='activity'?{updated:iso}:m.action==='tasks'?{tasks:[],more:false}:{generation:'invented',items,start:m.after??items[0]?.at??0,end:m.before??size,size};
    queueMicrotask(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value}}));
   }};taskq.receive(state);
  },{state,records,now,iso});
  const log=page.locator('.dialogue-transcript');await log.locator('.dialogue-item').first().waitFor();assert(await log.locator('time').count()>0,'native messages must expose relative timestamps');
  assert.equal(await log.locator('.dialogue-item.user time').textContent(),'<1m','owner message has a relative label');
  const ownerTime=log.locator('.dialogue-item.user time');assert.equal(await ownerTime.getAttribute('data-time-source'),iso);assert.equal(await ownerTime.getAttribute('datetime'),iso);
  assert.match(await ownerTime.getAttribute('title'),/31 Oct 2026.*19:00:30.*GMT\+07:00.*Asia\/Bangkok/);
  await ownerTime.focus();assert.match(await page.locator('.time-tooltip').innerText(),/You message.*19:00:30/);await page.keyboard.press('Escape');assert(await page.locator('.time-tooltip').isHidden());
  const steps=log.locator('.dialogue-steps');assert.equal(await steps.locator('summary .time-unknown').textContent(),'Time unknown','latest group member cannot borrow earlier time');
  await steps.locator('summary').click();assert.equal(await steps.locator('p .relative-time').count(),2);assert.match(await steps.locator('p .time-unknown').getAttribute('title'),/Tool recorded.*unavailable/);
  assert.equal(await log.locator('.dialogue-item').filter({hasText:'Zone-less record'}).locator('.time-unknown').count(),1);
  assert.equal(await log.locator('.dialogue-brief time').count(),1);
  await page.locator('.conversation-info>summary').click();
  assert.equal(await page.locator('.dialogue-status time[aria-label^="Next scheduled turn"]').textContent(),'in 1h');
  assert.match(await page.locator('.dialogue-status time[aria-label^="Turn submitted"]').getAttribute('title'),/Turn submitted/);
  const reader=page.locator('.conversation-reader'),anchor=log.locator('.dialogue-item').filter({hasText:'Reading anchor 10'});
  await anchor.evaluate(e=>{const r=e.closest('.conversation-reader');r.scrollTop+=e.getBoundingClientRect().top-r.getBoundingClientRect().top;r.dispatchEvent(new Event('scroll'));});
  await page.mouse.wheel(0,-1);await anchor.locator('time').focus();
  const before=await page.evaluate(()=>{
   const r=document.querySelector('.conversation-reader'),a=[...r.querySelectorAll('.dialogue-item')].find(e=>e.textContent.includes('Reading anchor 10'));
   window.oldTimes=[...document.querySelectorAll('.relative-time')];window.oldRecords=[...document.querySelectorAll('[data-key]')];window.oldRows=[...document.querySelectorAll('.conversation-row')];window.oldFocus=document.activeElement;
   return {y:a.getBoundingClientRect().top-r.getBoundingClientRect().top,scroll:r.scrollTop,source:a.querySelector('time').dataset.timeSource};
  });
  await page.clock.runFor(1002);
  assert.equal(await ownerTime.textContent(),'<1m');
  await page.clock.fastForward(60000);
  const after=await page.evaluate(()=>{
   const r=document.querySelector('.conversation-reader'),a=[...r.querySelectorAll('.dialogue-item')].find(e=>e.textContent.includes('Reading anchor 10'));
   const same=(old,selector)=>{const current=[...document.querySelectorAll(selector)];return old.length===current.length&&old.every((e,i)=>e===current[i]);};
   return {same:same(oldTimes,'.relative-time')&&same(oldRecords,'[data-key]')&&same(oldRows,'.conversation-row'),focus:oldFocus===document.activeElement,y:a.getBoundingClientRect().top-r.getBoundingClientRect().top,scroll:r.scrollTop,source:a.querySelector('time').dataset.timeSource};
  });
  assert(after.same&&after.focus,engine+' minute refresh preserves all nodes/order/focus');assert(Math.abs(before.y-after.y)<=2&&Math.abs(before.scroll-after.scroll)<=2,engine+' minute refresh keeps reading anchor');assert.equal(before.source,after.source);assert.equal(await ownerTime.textContent(),'1m');
  // Actual worker read path, including remote-host row, consumes the same normalized native timestamps.
  await page.evaluate(({board,n})=>{state.projects[0].tasks[0].state='doing';taskq.receive(state);taskq.receive({open:{page:'conversations',board,task:n}});},{board,n:task.n});
  await page.clock.runFor(1);await page.locator('.worker-live time').first().waitFor();assert.match(await page.locator('.worker-live').innerText(),/on remote-host/);
  assert.equal(await page.locator('.worker-live time[data-time-source="'+iso+'"]').count()>0,true);
  await page.evaluate(()=>{state.projects[0].tasks[0].state='ask';taskq.receive(state);});assert.equal(await page.locator('.journey-heading time[aria-label^="Question opened"]').count(),1);
  await page.locator('.thread-events>summary').click();await page.locator('.asked').waitFor();assert.equal(await page.locator('.asked .time-unknown').count(),1);assert.equal(await page.locator('.answered .time-unknown').count(),1);
  await page.evaluate(()=>taskq.receive({open:{page:'conversations',conversation:'owner'}}));
  // Explicitly select owner via the sidebar, independent of optional native open schema.
  if(!await page.locator('[data-c=select][data-id=owner]').isVisible())await page.locator('[data-c=list]').click();
  await page.locator('[data-c=select][data-id=owner]').click();
  assert.equal(await page.locator('.linked-task details summary .time-unknown').count(),1);assert.equal(await page.locator('[data-reading-key^="result:"] .time-unknown').count(),1);
  let cells=0;
  for(const width of [350,620,940])for(const font of [14,18,20])for(const theme of ['dark','light']){
   await page.setViewportSize({width,height:740});await page.evaluate(({font,theme})=>taskq.receive({...state,font,theme}),{font,theme});
   if(!await page.locator('.conversation-info').evaluate(e=>e.open))await page.locator('.conversation-info>summary').click();
   const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1);assert(!overflow,engine+' '+width+'/'+font+'/'+theme+' horizontal overflow');
   const labels=page.locator('.dialogue-transcript time');assert(await labels.count()>0,engine+' '+width+'/'+font+'/'+theme+' '+await log.innerText()+' '+JSON.stringify(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.action==='transcript').slice(-4)))) ;assert(await labels.first().evaluate(e=>getComputedStyle(e).fontSize)!=='0px');
   assert(await page.locator('.dialogue-status').evaluate(e=>e.scrollWidth<=e.clientWidth+1),'expanded time facts fit '+width+'/'+font+'/'+theme);
   const first=page.locator('.dialogue-transcript .dialogue-item').first();await first.scrollIntoViewIfNeeded();
   assert(await first.locator('time').evaluate(e=>{const r=e.getBoundingClientRect();return r.right<=innerWidth+1&&r.width>0;}),'timestamp fits narrow reading pane');cells++;
  }
  if(out){await page.setViewportSize({width:940,height:740});await page.evaluate(()=>taskq.receive({...state,font:20,theme:'dark'}));await page.locator('.dialogue-transcript .dialogue-item').first().scrollIntoViewIfNeeded();await page.screenshot({path:path.join(out,engine+'-owner.png')});await page.locator('.dialogue-item.user time').focus();await page.screenshot({path:path.join(out,engine+'-exact-focus.png')});}
  await page.evaluate(()=>window.failStatus=true);await page.locator('[data-c=status-refresh]').click();await page.locator('[data-c=select][data-id=owner] .time-unknown').waitFor();assert.match(await page.locator('[data-c=select][data-id=owner] .time-unknown').getAttribute('title'),/Invented native source unavailable/);assert.match(await page.locator('.dialogue-status .time-unknown[aria-label^="Turn submitted"]').getAttribute('title'),/Invented native source unavailable/);
  if(await page.locator('[data-c=list]').isVisible())await page.locator('[data-c=list]').click();await page.locator('[data-c=archive-list]').click();await page.locator('[data-c=select][data-id=archive]').click();await page.locator('.dialogue-transcript time').first().waitFor();assert.equal(await page.locator('[data-c-field=text]').count(),0);
  if(await page.locator('[data-c=list]').isVisible())await page.locator('[data-c=list]').click();await page.locator('[data-c=recent]').click();await page.locator('[data-thread="event:'+'a'.repeat(64)+'"]').click();assert.equal(await page.locator('.conversation-reader>time[aria-label^="Request created"]').count(),1);
  assert.deepEqual(errors,[]);assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer'||m.a==='conversation-action').length),0,'reading/refresh never writes board or session');
  console.log('PASS '+engine+' UX30-17..19: owner/remote-worker/tool/brief/question/history/event/list/archive, exact keyboard tooltip, minute nodes/focus/anchor; '+cells+' width/font/theme cells');
 }finally{await browser.close();}
}})().catch(e=>{console.error(e);process.exitCode=1;});
