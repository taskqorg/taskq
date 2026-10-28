// #1771 UX07: secondary controls stay reachable without competing with reading or answering.
// Real bundle, invented IPC; native providers and installed windows are separate qualification.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.TASKQ_EVIDENCE_1771,phase=process.env.TASKQ_PHASE_1771||'after';
const board='https://github.com/o/r/issues',root='fixture',fx=require('./fixtures/answer-card.json');
const question=structuredClone(fx.cases.find(c=>c.name==='choice').rows[0]);
const state={font:20,theme:'dark',host:'fixture-host',version:'fixture',projects:[{root,name:'Alpha',issues:board,alive:true,complete:true,tasks:[question]}],ai:{history:['Review the release'],runtime:{id:'claude',name:'Claude Code',available:true}},messages:{complete:true,entries:[]},conversations:{complete:true,items:[{id:'owner',title:'Release plan',revision:1,root,text:'Keep compatibility for this release.',caret:[0,0],files:[],archived:false,sent:{runtime:'claude',root,text:'Review the release plan.',files:[]},request:{id:'owner',state:'launched',kind:'launched',runtime:'claude',host:'fixture-host',url:'https://claude.ai/code/session_invented1771'}}],navigation:{selected:'owner',list:'recent'}}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:940,height:740}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 const shot=async name=>{if(out){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:path.join(out,name+'.png')});}};
 async function boot(width,theme){
  await page.setViewportSize({width,height:740});await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await page.evaluate(s=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-dialogue')queueMicrotask(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,state:'accepted',value:m.action==='status'?{state:'idle',watch:'no watch',last_line:'We can keep compatibility for one release.'}:{start:0,end:2,size:2,items:[{at:0,i:0,role:'user',text:'Review the release plan.'},{at:1,i:0,role:'assistant',text:'We can keep compatibility for one release. Which clients still need the old API?'}]}}}));}};taskq.receive(s);}, {...state,theme});
  await page.locator('.dialogue-item').first().waitFor();
 }
 for(const width of [350,620,940])for(const theme of ['dark','light']){
  await boot(width,theme);
  // Three baseline screens and six candidate screens form the bounded review set.
  if(phase==='after'||theme===(width===620?'light':'dark'))await shot(phase+'-'+width+'-'+theme);
  if(phase==='before')continue;
  const info=page.locator('.conversation-info');assert.equal(await info.evaluate(e=>e.open),false);
  assert((await info.locator('summary').boundingBox()).height>=32,'Details keeps the fine-pointer target');
  assert.equal(await page.locator('.dialogue-state').innerText(),'PM is ready','#1773: the idle state reads as the PM status line; activity remains visible with the narrow list hidden');
  for(const selector of ['[data-c-field=root]','[data-c-field=title]','[data-c=from-draft]','[data-w-field=prompt]','.pm-handoff'])assert.equal(await page.locator(selector).isVisible(),false,selector+' stays in Details');
  assert.doesNotMatch(await page.locator('.conversation-composer').innerText(),/native turn|New from draft|PDF|Host|Session/,'composer copy stays short');
  const send=await page.locator('[data-c=reply]').boundingBox();assert(send.y>=0&&send.y+send.height<=740,'Send stays reachable');
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  // Keyboard opens the same disclosure; context, watch, rename and all original actions survive.
  await info.locator('summary').focus();await page.keyboard.press('Enter');
  for(const selector of ['[data-c-field=root]','[data-c-field=title]','[data-c=from-draft]','[data-w-field=prompt]','.native-session','[data-c=detach]','[data-c=archive]'])assert(await page.locator(selector).isVisible(),selector+' remains reachable');
  assert.match(await info.innerText(),/Session claude|PDF unavailable/);
  assert.equal(await page.locator('[data-c-field=text]').inputValue(),state.conversations.items[0].text);
  await page.locator('[data-c-field=title]').fill('Release decision');await info.locator('summary').click();
  assert.equal(await page.locator('.thread-title').innerText(),'Release decision');
  assert.equal(await page.evaluate(()=>sent.filter(m=>['send','reply','fork','watch','archive'].includes(m.action)).length),0,'opening and renaming never dispatch work');
 }
 if(phase==='before'){
  assert.equal(await page.locator('[data-c=from-draft]').isVisible(),false,'UX07: separate-request creation must be collapsed by default');return;
 }
 await page.evaluate(({board,n})=>taskq.receive({open:{page:'answer',board,task:n}}),{board,n:question.n});
 assert.equal(await page.locator('.attach-label').count(),0,'text-only answers hide unusable Attach');
 assert.doesNotMatch(await page.locator('.answer-composer').innerText(),/Attachments: none|A number typed/);
 await shot('after-question-940-light');
 await page.locator('.thread-detail .conversation-info>summary').click();
 assert.match(await page.locator('.conversation-info').innerText(),/text only|Numbers typed/);
 assert.equal(await page.locator('[data-qac=choose]').count(),question.question.choices.length,'source options retained');
 // A4: the same CSS raises usable controls to 44 px for touch; no palette/type/icon change.
 const touch=await browser.newPage({viewport:{width:350,height:740},hasTouch:true});
 await touch.setContent(fs.readFileSync('build/web/workspace.html','utf8'));await touch.evaluate(s=>{window.ipc={postMessage:()=>{}};taskq.receive(s);},state);
 for(const selector of ['.conversation-info>summary','.attach-label','[data-c=reply]'])assert((await touch.locator(selector).boundingBox()).height>=44,selector+' keeps the coarse-pointer target');
 await touch.close();
 assert.deepEqual(errors,[]);console.log('PASS #1771 UX07: collapsed secondary tools, keyboard/context/rename, source choices and text-only limits at 350/620/940 in both themes; no unintended dispatch');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
