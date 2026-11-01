// #2110 A5/A6, Q-A3-Q-A5: a whole Unicode record cannot turn a bounded record window into unbounded DOM.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:700,height:700}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{window.reads=[];window.record=('Invented Unicode 🐈 body '+String.fromCodePoint(0x1f642)).repeat(90000);window.timings=[];
  window.ipc={postMessage:text=>{const m=JSON.parse(text);if(m.a==='conversation-dialogue')queueMicrotask(()=>{const at=performance.now();taskq.receive({dialogueReceipt:{...m,state:'accepted',value:m.action==='status'?{state:'idle',watch:'no watch'}:m.action==='tasks'?{tasks:[],more:false}:{generation:'invented',start:0,end:100,size:100,items:[{at:0,i:0,role:'assistant',text:record}]}}});if(m.action==='transcript'){reads.push(m);document.querySelector('.conversation-reader')?.getBoundingClientRect();timings.push(performance.now()-at);}});}};
  window.fixture={font:20,theme:'dark',projects:[],conversations:{complete:true,items:[{id:'owner',revision:1,text:'Independent draft',caret:[2,4],files:[],sent:{root:'fixture',runtime:'claude',text:'Start'},request:{state:'launched'}}],navigation:{selected:'owner'}}};taskq.receive(fixture);
 });
 await page.locator('[data-native-part="0"]').waitFor();const pieces=[];
 for(;;){const value=await page.locator('.dialogue-item>.conversation-text>span.conversation-text').innerText();pieces.push(value);assert(value.length<=32770);assert((await page.locator('[data-native-part]').count())<=32);
  const next=page.getByRole('button',{name:'Next text',exact:true});if(!await next.count())break;await next.click();
 }
 assert.equal(await page.evaluate(pieces=>pieces.join('')===record,pieces),true,'every Unicode code point is retained, with no seam duplication');
 await page.getByRole('button',{name:'Previous text',exact:true}).click();const first=await page.locator('[data-native-part]').first().getAttribute('data-native-part');
 await page.locator('.conversation-reader').evaluate(e=>{e.scrollTop+=100;e.dispatchEvent(new Event('scroll',{bubbles:true}));});
 const anchor=await page.evaluate(()=>{const e=document.querySelector('.conversation-reader'),top=e.getBoundingClientRect().top,n=[...e.querySelectorAll('[data-native-part]')].find(n=>n.getBoundingClientRect().bottom>top);return {part:n.dataset.nativePart,y:n.getBoundingClientRect().top-top};});
 await page.evaluate(()=>{fixture.font=18;taskq.receive(fixture);});await page.waitForTimeout(100);
 const y=await page.locator('[data-native-part="'+anchor.part+'"]').evaluate(e=>e.getBoundingClientRect().top-e.closest('.conversation-reader').getBoundingClientRect().top);assert(Math.abs(y-anchor.y)<=2,'large-record paragraph anchor survives font reflow');
 assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Independent draft');assert.equal(await page.evaluate(()=>reads.filter(m=>m.after===undefined).length),1,'text pages reuse the already read bounded native record');assert(await page.evaluate(()=>reads.slice(1).every(m=>m.after===100)),'only the existing EOF poll may read while paging text');
 assert((await page.evaluate(()=>taskq.bounds().conversations.nativeBytes))<=32*1024*1024);assert.equal(await page.locator('[data-native-part]').first().getAttribute('data-native-part'),first);
 assert.deepEqual(errors,[]);console.log(JSON.stringify({case:'whole Unicode record, bounded DOM, exact seams, source reuse, anchor and draft retained',characters:await page.evaluate(()=>record.length),receiveAndLayoutMs:await page.evaluate(()=>timings),bounds:await page.evaluate(()=>taskq.bounds().conversations)}));
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
