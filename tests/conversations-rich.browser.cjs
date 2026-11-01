// #2159 D4: actual bundle, invented transcript. Detect dropped tool images, unsafe content,
// lost literal code and unbounded mounted decode; this never qualifies native history.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium,webkit}=require('playwright');
(async()=>{for(const engine of [chromium,webkit]){
 const browser=await engine.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:900,height:760}}),errors=[],requests=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>requests.push(r.url()));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:text=>sent.push(JSON.parse(text))};});
 const board='https://example.org/fixture/issues',thread='task:'+board+'/7';
 const state={font:18,theme:'dark',host:'fixture',projects:[{root:'fixture',name:'Invented',issues:board,alive:true,complete:true,tasks:[{n:7,title:'Invented Markdown and image episode',state:'doing',role:'worker',attempt:71,runtime:'codex',local_session:true,session:'codex:invented fixture'}]}],ai:{history:[]},conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
 await page.evaluate(s=>taskq.receive(s),state);await page.evaluate(board=>taskq.receive({open:{page:'conversations',board,task:7,root:'fixture'}}),board);
 await page.waitForFunction(()=>sent.some(m=>m.action==='transcript'));
 const src=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=480;c.height=160;const x=c.getContext('2d');x.fillStyle='#246341';x.fillRect(0,0,480,160);x.fillStyle='white';x.font='24px sans-serif';x.fillText('Invented image fixture #2159',22,85);return c.toDataURL();});
 const code='const answer = "<script>"; // do not resend',text='## Current action\n\nKeep **all evidence** and *source identity*.\n\n- Read progress\n- Do not resend\n\n```javascript\n'+code+'\n```';
 const items=[{at:0,i:0,role:'assistant',text},...Array.from({length:6},(_,i)=>({at:10+i,i:0,role:'tool',text:'',image:{name:'Invented image fixture '+(i+1),src,width:480,height:160}})),{at:20,i:0,role:'tool',text:'',image:{name:'Unavailable artifact',reason:'Image bytes missing; no file fetched',url:'https://example.org/artifact.png'}}];
 await page.evaluate(({board,thread,items})=>{const m=sent.find(m=>m.action==='transcript');taskq.receive({dialogueReceipt:{id:m.id,conversation:thread,action:'transcript',state:'accepted',value:{generation:'fixture-rich',items,start:0,end:30,size:30}}});},{board,thread,items});
 const panel=page.locator('.worker-live');await page.waitForFunction(()=>document.querySelector('.worker-live .syntax-keyword'));
 assert.equal(await panel.locator('pre code').innerText(),code,'highlighted code retains exact characters');
 assert.equal(await panel.locator('script').count(),0);assert.equal(await panel.locator('img').count(),4,'reader caps mounted images');
 assert.match(await panel.innerText(),/Image reader preview limit/);assert.match(await panel.innerText(),/Image bytes missing/);
 assert.equal(await panel.locator('a[data-url="https://example.org/artifact.png"]').count(),1,'source artifact remains inspectable');
 const first=panel.locator('img').first();await first.scrollIntoViewIfNeeded();await page.waitForFunction(()=>document.querySelector('.worker-live img')?.naturalWidth===480);
 assert.equal(await first.getAttribute('loading'),'lazy');assert.equal(await first.getAttribute('decoding'),'async');
 const box=await first.boundingBox();assert(Math.abs(box.width/box.height-3)<.05,'aspect ratio retained');
 await panel.locator('summary').filter({hasText:'Exact source'}).click();assert.equal(await panel.locator('[data-key="0:0"] details pre').innerText(),text);await panel.locator('summary').filter({hasText:'Exact source'}).click();
 for(const theme of ['dark','light']){await page.evaluate(s=>taskq.receive(s),{...state,theme});assert.equal(await page.evaluate(()=>document.documentElement.style.colorScheme),theme);for(const width of [900,540]){
  await page.setViewportSize({width,height:760});await panel.locator('[data-key="0:0"]').scrollIntoViewIfNeeded();
  assert(await page.locator('.conversation-reader').evaluate(e=>e.scrollWidth<=e.clientWidth+2),'rich reader has no horizontal scroll trap');
  if(process.env.TASKQ_EVIDENCE_2159){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2159,{recursive:true});await page.screenshot({path:process.env.TASKQ_EVIDENCE_2159+'/rich-'+engine.name()+'-'+theme+'-'+width+'.png'});if(theme==='dark'&&width===900){await first.scrollIntoViewIfNeeded();await page.screenshot({path:process.env.TASKQ_EVIDENCE_2159+'/image-'+engine.name()+'.png'});}}
 }}
 assert(!requests.some(u=>/^https?:/.test(u)),'rendering never fetches remote/private artifacts');assert.deepEqual(errors,[]);
 }finally{await browser.close();}
}console.log('PASS rich Conversations: language tokens/exact source, bounded source-backed image rendering, no remote fetch, Chromium/WebKit themes/widths');})().catch(e=>{console.error(e);process.exitCode=1;});
