// #2110 A5/A6, Q-A3-Q-A5: a missing saved record never resumes following without explicit Latest.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:900,height:700}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='conversation-dialogue')queueMicrotask(()=>taskq.receive({dialogueReceipt:{...m,state:'accepted',value:m.action==='status'?{state:'idle',watch:'no watch'}:{generation:'invented-source',start:m.after!==undefined?200:0,end:300,size:300,items:[{at:200,i:0,role:'assistant',text:'Current source text'}]}}}));}};
  taskq.receive({font:20,theme:'dark',host:'fixture',projects:[],ai:{history:[],runtime:{id:'claude',available:true}},messages:{complete:true,entries:[]},conversations:{complete:true,items:[{id:'owner',revision:1,root:'fixture',title:'Saved source',text:'Independent draft',caret:[1,4],files:[],sent:{root:'fixture',runtime:'claude',text:'Original request',files:[]},request:{state:'launched',runtime:'claude'}}],navigation:{list:'recent',selected:'owner',threads:{selected:'owner',positions:{owner:{at:100,i:0,y:-7,g:'invented-source'}}}}}});
 });
 await page.getByText('Saved reading position is no longer in the transcript. Choose Latest to open the current source.',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='conversation-dialogue'&&m.action==='transcript').length),1,'source loss cannot issue an automatic tail read');
 assert.equal(await page.locator('.dialogue-item').count(),0);assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Independent draft');
 await page.locator('[data-c=latest]').click();await page.getByText('Current source text',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='transcript').length),2);assert.equal(await page.locator('[data-c=latest]').count(),0);
 assert.equal(await page.evaluate(()=>sent.some(m=>m.a==='answer'||['send','reply'].includes(m.action))),false);assert.deepEqual(errors,[]);
 for(const generation of ['invented-source','replacement-source','images-v1:invented-source']){
  const worker=await browser.newPage(),workerErrors=[];worker.on('pageerror',e=>workerErrors.push(e.message));await worker.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await worker.evaluate(generation=>{window.sent=[];window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='conversation-dialogue'&&m.action==='transcript')queueMicrotask(()=>taskq.receive({dialogueReceipt:{...m,state:'accepted',value:{generation,start:m.after!==undefined?200:0,end:300,size:300,items:[...(generation.startsWith('images-v1:')?[{at:100,i:0,role:'tool',text:'',image:{name:'Image now occupying the old text ordinal',reason:'Invented image; no bytes fetched'}}]:[]),{at:200,i:0,role:'assistant',text:'Current worker source'}]}}}));}};
   const id='task:https://example.org/issues/7';taskq.receive({font:20,theme:'dark',projects:[{root:'fixture',name:'Invented',issues:'https://example.org/issues',alive:true,complete:true,tasks:[{n:7,title:'Worker source',state:'doing',role:'worker',attempt:71,revision:71,runtime:'codex',session:'codex:invented fixture',local_session:true,host:'fixture',executions:[{attempt:71,state:'running'}]}]}],messages:{complete:true,entries:[]},conversations:{complete:true,items:[],navigation:{threads:{selected:id,positions:{[id]:{at:100,i:0,y:-7,g:'invented-source'}}}}}});
  },generation);
  await worker.getByText(/Choose Latest to open the current source/).waitFor();await worker.waitForTimeout(150);assert.equal(await worker.evaluate(()=>sent.filter(m=>m.action==='transcript').length),1,'worker source loss cannot retry/follow automatically');
  await worker.locator('[data-c=worker-latest]').click();await worker.getByText('Current worker source',{exact:true}).waitFor();assert.equal(await worker.evaluate(()=>sent.filter(m=>m.action==='transcript').length),2);assert.equal(await worker.evaluate(()=>sent.some(m=>m.a==='answer'||['send','reply'].includes(m.action))),false);assert.deepEqual(workerErrors,[]);await worker.close();
 }
 console.log('PASS #2159 parser-version same-key image fence and #2110 owner/worker missing anchor and replacement generation: explicit Latest only, exact cursor, no automatic follow/send, independent draft retained');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
