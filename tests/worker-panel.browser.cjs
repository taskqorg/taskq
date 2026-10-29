// #1831 A1-A5: real bundled view, invented native receipts. Rust checks own session selection and tool privacy.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:900,height:720}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  for(const runtime of ['claude','codex']){
   const board='https://example.org/alpha/issues',thread='task:'+board+'/7';
   const task={n:7,title:'Live worker',state:'doing',role:'worker',attempt:71,runtime,local_session:true,session_id:'invented-native',session:runtime+':invented-native fixture-host'};
   const state={font:16,theme:'dark',host:'fixture-host',projects:[{root:'fixture-alpha',name:'Alpha',issues:board,alive:true,complete:true,tasks:[task]}],ai:{history:[]},conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
   await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:text=>sent.push(JSON.parse(text))};});
   await page.evaluate(s=>taskq.receive(s),state);
   await page.evaluate(board=>taskq.receive({open:{page:'conversations',board,task:7,root:'fixture-alpha'}}),board);
   async function read(){await page.waitForFunction(()=>sent.some(m=>m.a==='conversation-dialogue'&&m.action==='transcript'));return page.evaluate(()=>{const i=sent.findIndex(m=>m.a==='conversation-dialogue'&&m.action==='transcript');return sent.splice(i,1)[0];});}
   async function reply(m,value){await page.evaluate(r=>taskq.receive({dialogueReceipt:r}),{id:m.id,conversation:thread,action:'transcript',state:'accepted',value});}
   const panel=page.locator('.worker-live');
   let m=await read();
   assert.equal(m.session,undefined,'A2: browser must not select a native session');
   assert.equal(m.attempt,71);assert.equal(m.root,'fixture-alpha');
   await reply(m,{items:[{at:0,i:0,role:'assistant',text:'First worker message'},{at:10,i:0,role:'tool',text:'Bash'}],end:20});
   await page.waitForFunction(()=>document.querySelector('.worker-live')?.textContent.includes('First worker message'));
   assert.match(await panel.innerText(),/Tool · Bash/,'A1: first render includes tool lines');
   const began=Date.now();m=await read();assert.equal(m.after,20);
   await reply(m,{items:[{at:20,i:0,role:'owner',text:'Please keep the draft'},{at:30,i:0,role:'assistant',text:'Appended reply'}],end:40});
   await page.waitForFunction(()=>document.querySelector('.worker-live')?.textContent.includes('Appended reply'));
   assert(Date.now()-began<2000,'A1: appended owner and worker turns arrive within 2 s');
   assert.match(await panel.innerText(),/You · via the board\s+Please keep the draft/);
   m=await read();await reply(m,{items:Array.from({length:25},(_,i)=>({at:40+i*10,i:0,role:'assistant',text:'Progress '+i+' '+('reading context '.repeat(12))})),end:300});
   const reader=page.locator('.conversation-reader');
   await page.waitForFunction(()=>document.querySelector('.worker-live')?.textContent.includes('Progress 24'));
   assert(await reader.evaluate(e=>e.scrollHeight-e.scrollTop-e.clientHeight<2),'D4: follows while at the end');
   await panel.locator('[data-key="90:0"]').scrollIntoViewIfNeeded();
   await page.waitForFunction(()=>!!document.querySelector('[data-c=worker-latest]'));
   const anchor=await panel.locator('[data-key="90:0"]').boundingBox();m=await read();await reply(m,{items:[{at:300,i:0,role:'assistant',text:'New while reading above'}],end:310});
   await page.waitForFunction(()=>document.querySelector('.worker-live')?.textContent.includes('New while reading above'));
   assert(Math.abs((await panel.locator('[data-key="90:0"]').boundingBox()).y-anchor.y)<=2,'D4: append preserves the reading anchor');
   await page.locator('[data-c=worker-latest]').click();assert(await reader.evaluate(e=>e.scrollHeight-e.scrollTop-e.clientHeight<2),'D4: explicit latest resumes following');
   m=await read();await reply(m,{ended:true,items:[]});
   await page.waitForFunction(()=>document.querySelector('.worker-live')?.textContent.includes('Worker attempt ended'));
   await page.waitForTimeout(1200);
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.action==='transcript').length),0,'A3: ended response stops polling');
   // A replaced attempt starts with a new cursor; a late old reply cannot pollute it.
   task.attempt=72;await page.evaluate(s=>taskq.receive(s),state);m=await read();assert.equal(m.attempt,72);assert.equal(m.after,undefined);
   await page.evaluate(()=>taskq.receive({open:{page:'sessions'}}));
   await reply(m,{items:[{at:40,i:0,role:'assistant',text:'Late old reply'}],end:50});
   await page.waitForTimeout(1200);
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.action==='transcript').length),0,'A3: leaving the thread cancels the read');
   assert.equal(await panel.count(),0);
   task.state='review';await page.evaluate(s=>taskq.receive(s),state);await page.evaluate(board=>taskq.receive({open:{page:'conversations',board,task:7,root:'fixture-alpha'}}),board);
   assert.equal(await panel.count(),0,'A3: review is not a live worker panel');
   task.state='doing';task.attempt=73;await page.evaluate(s=>taskq.receive(s),state);m=await read();
   await page.locator('[data-a=hide]').click();
   await reply(m,{items:[{at:0,i:0,role:'assistant',text:'Hidden reply'}],end:1});
   await page.evaluate(s=>taskq.receive(s),state);await page.waitForTimeout(1200);
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.action==='transcript').length),0,'A3: closing the window cancels reads through subsequent snapshots');
   await page.evaluate(board=>taskq.receive({open:{page:'conversations',board,task:7,root:'fixture-alpha'}}),board);m=await read();
   await page.evaluate(()=>taskq.receive({workerPanelClosed:true}));await reply(m,{items:[],end:0});await page.waitForTimeout(1200);
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.action==='transcript').length),0,'A3: native close cancels polling too');
  }
  assert.deepEqual(errors,[]);console.log('#1831 live worker panel: first render, append, attempt fence and teardown passed for Claude/Codex');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
