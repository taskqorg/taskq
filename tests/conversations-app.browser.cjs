// #2159 D2/C4: actual bundle, invented App receipts. The UI must retain uncertain input,
// show the typed request and keep setup/provider metadata inspectable without a second Send.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:1000,height:800}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(()=>{
  window.sent=[];window.fixtureStatus={state:'working',turn:null,watch:'no watch'};
  window.fixtureOwner={id:'invented-owner',revision:1,root:'fixture',runtime:'codex',title:'Invented App owner episode',text:'Retained refused reply',caret:[2,8],files:[],sent:{root:'fixture',runtime:'codex',text:'Read real progress, ask one clarification and relay the reviewed result.',files:[]},request:{state:'launched',runtime:'codex'}};
  window.ipc={postMessage:raw=>{const m=JSON.parse(raw);sent.push(m);
   if(m.a==='conversation-dialogue')queueMicrotask(()=>taskq.receive({dialogueReceipt:{...m,state:'accepted',value:m.action==='status'?fixtureStatus:{generation:'fixture-app',start:0,end:50,size:50,items:[
    {at:0,i:0,role:'transport',text:'taskq handshake: reply OK and do nothing else. The task follows in this thread.'},
    {at:10,i:0,role:'transport',text:'OK'},
    {at:20,i:0,role:'brief',text:'Before any work, use Read to read the owned brief in full.'},
    {at:30,i:0,role:'assistant',text:'Which reviewed result should I relay?\n<oai-mem-citation>\nInvented provider bookkeeping\n</oai-mem-citation>'}
   ]}}}));
   if(m.a==='conversation-action'&&m.action==='save'){Object.assign(fixtureOwner,m.draft);fixtureOwner.revision++;queueMicrotask(()=>taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:{complete:false,items:[structuredClone(fixtureOwner)]}}}));}
  }};
  taskq.receive({font:18,theme:'dark',host:'fixture',projects:[{root:'fixture',name:'Invented',issues:'https://example.org/issues',alive:true,complete:true,tasks:[]}],ai:{history:[],runtime:{id:'codex',name:'Codex',available:true}},messages:{complete:true,entries:[]},conversations:{complete:true,items:[fixtureOwner],navigation:{list:'recent',selected:fixtureOwner.id,threads:{selected:fixtureOwner.id}}}});
 });
 await page.waitForFunction(()=>document.querySelector('.dialogue-transcript')?.textContent.includes('Which reviewed result'));
 const transcript=page.locator('.dialogue-transcript'),reader=page.locator('.conversation-reader');
 assert.match(await reader.innerText(),/Read real progress, ask one clarification/);
 assert(!/taskq handshake|Invented provider bookkeeping|Before any work/.test(await transcript.innerText()),'transport/provider evidence is secondary');
 assert.equal(await transcript.locator('.dialogue-setup').count(),1,'one secondary setup disclosure');assert.equal(await transcript.locator('.dialogue-setup article').count(),3,'all original setup records remain inspectable');
 assert.equal(await transcript.getByText('Provider citation metadata',{exact:true}).count(),1);
 assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Retained refused reply');
 await page.locator('[data-c=status-refresh]').evaluate(b=>b.closest('details').open=true);
 await page.evaluate(()=>fixtureStatus={state:'idle',watch:'no watch',turn:{state:'done',op_id:'refused-original-op',input:'Retained refused reply',outcome:'open in codex; close it there first',retry_refused:true,at:1}});
 await page.locator('[data-c=status-refresh]').click();await page.locator('[data-c=retry-turn]').waitFor();
 await page.locator('[data-c=retry-turn]').click();
 await page.waitForFunction(()=>sent.some(m=>m.action==='retry-turn'));
 const retry=await page.evaluate(()=>sent.find(m=>m.action==='retry-turn'));assert.equal(retry.op_id,'refused-original-op');assert.equal(retry.conversation,'invented-owner');
 assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Retained refused reply');
 assert(!await page.evaluate(()=>sent.some(m=>['send','reply'].includes(m.action))),'retry cannot invent another message or session');
 if(process.env.TASKQ_EVIDENCE_2159){fs.mkdirSync(process.env.TASKQ_EVIDENCE_2159,{recursive:true});await page.locator('[data-c=status-refresh]').evaluate(b=>b.closest('details').open=false);await page.screenshot({path:process.env.TASKQ_EVIDENCE_2159+'/app-document.png'});}
 assert.deepEqual(errors,[]);console.log('PASS App owner document: typed request, secondary exact transport/provider source, retained draft and explicit same-op retry without another Send');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
