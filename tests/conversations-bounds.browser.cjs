// #2110 A5/A6/A8, CS1-CS6/Q-A3-Q-A5: actual bundle; invented source reads only.
// Risk: hidden eager bodies, unbounded pages/query caches, stale body replies or reading-context loss.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const browser=await chromium.launch(require('./browser.cjs')({args:['--js-flags=--expose-gc']}));const measured=[];try{
 for(const count of [600,10000]){
  const page=await browser.newPage({viewport:{width:1000,height:700}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  const s={font:20,theme:'dark',host:'fixture',projects:[{root:'fixture',name:'Invented',issues:'https://example.org/issues',alive:true,complete:true,tasks:[],events:[],reports:{complete:true,reports:[],history:[]}}],ai:{history:[],runtime:{id:'claude',available:true}},messages:{complete:true,entries:[]},conversations:{complete:true,items:Array.from({length:count},(_,i)=>({id:'owner-'+i,revision:1,root:'fixture',title:'Thread '+String(i).padStart(5,'0'),preview_title:'Thread '+i,lazy:true,archived:false,updated:1000000+i})),navigation:{threads:{selected:null},list:'recent',listOnly:true}}};
  const raw=JSON.stringify(s);
  const timings=await page.evaluate(raw=>{
   window.sent=[];window.forbidden=false;window.latencies=[];for(const event of ['input','click'])document.querySelector('.menu').addEventListener(event,()=>{const at=performance.now();requestAnimationFrame(()=>latencies.push(performance.now()-at));},true);window.bodies=new Map();window.delayed=[];window.holdBodies=false;
   window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(sent.length>64)sent.shift();if(m.a==='answer'||['send','reply','watch','unwatch'].includes(m.action))forbidden=true;
    if(m.a==='conversation-dialogue'&&m.action==='body'){
     const answer=()=>{const c=fixture.conversations.items.find(c=>c.id===m.conversation),body=bodies.get(c.id)||{...c,lazy:false,text:'Invented saved draft 🐈',caret:[2,4],scroll:0,files:[]};bodies.set(c.id,body);taskq.receive({dialogueReceipt:{...m,state:'accepted',value:structuredClone(body)}});};if(holdBodies)delayed.push(answer);else queueMicrotask(answer);
    }
    if(m.a==='conversation-action'&&m.action==='save'){const c=bodies.get(m.conversation);Object.assign(c,m.draft);c.revision++;queueMicrotask(()=>taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:{complete:false,items:[structuredClone(c)]}}}));}
   }};
   let t=performance.now();window.fixture=JSON.parse(raw);const parse=performance.now()-t;t=performance.now();taskq.receive(fixture);const project=performance.now()-t;t=performance.now();document.querySelector('.conversation-rows').getBoundingClientRect();return {parse,project,layout:performance.now()-t};
  },raw);
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='body').length),0,'list does not fetch unrelated bodies');
  assert.equal((await page.locator('.conversation-view>summary').innerText()).replace(/\s+/g,' '),'All · '+count,'full-scope total independent of resident pages');
  assert((await page.locator('.conversation-row').count())<=192);
  const navigate=async i=>{
   await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('Thread '+String(i).padStart(5,'0'));
   await page.locator('[data-c=select][data-id="owner-'+i+'"]').click();
   await page.waitForFunction(id=>!document.querySelector('[data-c-field=text]')?.disabled&&document.querySelector('.conversation-detail')?.dataset.threadId===id,'owner-'+i);
  };
  await navigate(0);await page.locator('[data-c-field=text]').fill('Preserved unsent independent draft');
  await page.waitForFunction(()=>sent.some(m=>m.action==='save'&&m.draft?.text==='Preserved unsent independent draft'));
  const samples=[],heaps=[],cdp=await page.context().newCDPSession(page);
  for(let tour=0;tour<5;tour++){for(let i=0;i<30;i++){
   const t=Date.now();await navigate(i);samples.push(Date.now()-t);
   const bounds=await page.evaluate(()=>taskq.bounds().conversations);assert(bounds.queryKeys<=24);assert(bounds.bodySources<=8);assert(bounds.bodyBytes<=8*1024*1024);assert(bounds.listRows<=192);
  }await cdp.send('HeapProfiler.collectGarbage');heaps.push((await cdp.send('Runtime.getHeapUsage')).usedSize);}
  await navigate(0);assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Preserved unsent independent draft','evicted draft rehydrates from its durable source');
  // An old body reply may populate its own cache, never the selected draft.
  await page.evaluate(()=>holdBodies=true);await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('Thread 00040');await page.locator('[data-c=select][data-id=owner-40]').click();
  await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('Thread 00041');await page.locator('[data-c=select][data-id=owner-41]').click();await page.evaluate(()=>{holdBodies=false;delayed.reverse().forEach(fn=>fn());delayed=[];});await page.waitForFunction(()=>!document.querySelector('[data-c-field=text]').disabled);
  assert.equal(await page.locator('.conversation-detail').getAttribute('data-thread-id'),'owner-41');
  await page.locator('[data-c=clear-search]').click();await page.locator('[data-c=list]').evaluateAll(es=>{if(innerWidth<=620)es[0]?.click();});
  for(let i=0;i<6;i++){const more=page.locator('[data-c=results-more]');if(await more.count())await more.click();assert((await page.locator('.conversation-row').count())<=192);}
  const bounds=await page.evaluate(()=>{globalThis.gc?.();return taskq.bounds();});const latency=await page.evaluate(()=>latencies.sort((a,b)=>a-b)[Math.floor(latencies.length*.95)]);const interactionBudget=latency<=100?'Pass':'Fail';
  assert.equal(await page.evaluate(()=>forbidden),false);
  assert.deepEqual(errors,[]);samples.sort((a,b)=>a-b);measured.push({count,bytes:Buffer.byteLength(raw),...timings,bounds,interactionP95:latency,interactionBudget,retainedHeapAfterTours:heaps,driverInteractionP95:samples[Math.floor(samples.length*.95)]});await page.close();
 }
 console.log(JSON.stringify({functionalAssertions:'Pass; latency budget is reported separately, never inferred from exit status',environment:'Headless Chromium, 8 logical cores, 1000x700, 20px, dark; driver timing includes Playwright auto-wait',measured},null,2));
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
