// #1929 A1/A3/A4/A5/A6: production bundle with invented board/native receipts; never sends live input.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const root='fixture',board='https://example.org/alpha/issues',q=require('./fixtures/answer-card.json').cases.find(c=>c.name==='choice').rows[0];
const owner={id:'owner-flow',title:'PM flow',revision:1,root,text:'',caret:[0,0],files:[],sent:{root,runtime:'claude',text:'Request',files:[]},request:{state:'launched',runtime:'claude'}};
const task={...q,state:'doing',role:'worker',attempt:20,executions:[{attempt:20,state:'running'}],revision:20,session:'claude:fixture host',host:'host',runtime:'claude',conversation:{id:owner.id,brief:'An inspectable held brief'},controls:['stop','later'],question:null,card:null};
const state={palette:'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--new:#6EA8FF;--review:#BCA3EB',font:20,theme:'dark',host:'host',projects:[{root,name:'Alpha',issues:board,alive:true,complete:true,tasks:[task]}],ai:{history:[],runtime:{id:'claude',available:true}},conversations:{complete:true,items:[owner,{...owner,id:"owner-other",title:"Other PM"}],navigation:{selected:owner.id,list:'recent'}},messages:{complete:true,entries:[]}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:940,height:740}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await page.evaluate(s=>{window.state=s;window.sent=[];window.header=t=>{const row={...t,body_lazy:true,body_source_revision:t.revision,history_count:t.history?.length||0,history_lazy:true,conversation:t.conversation&&{id:t.conversation.id}};for(const key of ['question','card','history','verdict','answered'])delete row[key];return row;};window.deliver=()=>taskq.receive({...state,projects:state.projects.map(p=>({...p,tasks:p.tasks.map(header)}))});window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);if(m.a==='conversation-action'&&m.action==='save'){const c=state.conversations.items[0];Object.assign(c,m.draft);c.revision++;queueMicrotask(()=>taskq.receive({conversationReceipt:{id:m.id,state:'accepted',snapshot:state.conversations}}));}if(m.a==='conversation-dialogue'&&m.action==='task-body'){const t=state.projects.find(p=>p.root===m.root)?.tasks.find(t=>m.conversation.endsWith('/'+t.n))||(window.completedRows||[]).find(t=>m.conversation.endsWith('/'+t.n)),value={...t,body_lazy:false,history_count:t?.history?.length||0,history_lazy:true,history_source_revision:t?.revision,history_source_count:t?.history?.length||0};delete value.history;queueMicrotask(()=>taskq.receive({dialogueReceipt:{...m,state:t?.revision===m.source_revision?'accepted':'unknown',value}}));return;}if(m.a==='conversation-dialogue'&&m.action==='history'){const t=state.projects[0].tasks.find(t=>m.conversation.endsWith('/'+t.n))||(window.completedRows||[]).find(t=>m.conversation.endsWith('/'+t.n)),rows=t?.history||[],start=m.start||0;queueMicrotask(()=>taskq.receive({dialogueReceipt:{...m,state:t?.revision===m.revision?'accepted':'unknown',value:{rows:rows.slice(start,start+64),start,next:Math.min(rows.length,start+64),total:rows.length,revision:t?.revision,complete:true}}}));return;}if(m.a==='conversation-dialogue')queueMicrotask(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:m.action==='status'?{state:'idle',watch:'no watch'}:m.action==='tasks'?{tasks:(window.completedRows||[]).map(header),more:false}:{generation:window.sourceGeneration||'fixture-a',items:Array.from({length:30},(_,i)=>({at:i,i:0,role:'assistant',text:'Record '+i+' body'.repeat(100)})),start:0,end:30,size:30}}}));}};deliver();},state);
 await page.evaluate(({board,n})=>taskq.receive({open:{page:'conversations',board,task:n}}),{board,n:task.n});
 assert.equal(await page.locator('[data-c-field=steer]').count(),1,'A5: a working worker accepts input before any question');
 await page.locator('[data-c-field=steer]').fill('Keep this intent');await page.locator('[data-a=answer-steer]').click();
 const op=await page.evaluate(()=>sent.find(m=>m.a==='answer').op);assert.equal(op.action,'steer');assert.equal(op.revision,20);
 await page.evaluate(id=>taskq.receive({answerReceipt:{root:'fixture',id,state:'unknown',reason:'Invented lost receipt'}}),op.id);
 assert.equal(await page.locator('[data-c-field=steer]').inputValue(),'Keep this intent');
 await page.locator('[data-a=answer-retry]').click();assert.deepEqual(await page.evaluate(()=>sent.filter(m=>m.a==='answer').at(-1).op),op,'A6: unknown checks the same operation');
 await page.evaluate(id=>taskq.receive({answerReceipt:{root:'fixture',id,state:'accepted'}}),op.id);
 await page.locator('[data-c=select][data-id=owner-flow]').click();
 await page.locator('.dialogue-transcript .dialogue-item').first().waitFor();
 const reader=page.locator('.conversation-reader');await reader.hover();await page.mouse.wheel(0,-700);await page.waitForFunction(()=>document.querySelector('[data-c=latest]'));
 await reader.evaluate(e=>{e.scrollTop=e.scrollHeight;e.dispatchEvent(new Event('scroll'));});
 assert.equal(await page.locator('[data-c=latest]').count(),1,'A4: scrolling down never resumes follow');
 const before=await page.locator('.dialogue-transcript').evaluate(e=>e.scrollHeight);
 await page.locator('[data-c-field=text]').fill('Long draft\n'.repeat(100));
 assert((await page.locator('.dialogue-transcript').evaluate(e=>e.getBoundingClientRect().height))>=before-1,'A3: transcript never shrinks beneath long input');
 const send=page.locator('[data-c=reply]');assert(await send.evaluate(e=>{const r=e.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight&&document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.closest('button')===e;}),'A3: long-draft Send stays reachable');

 await page.locator('[data-c-field=text]').fill('PM draft kept');
 assert(await page.locator('[data-c=reply]').isEnabled(),await page.locator('#conversation-reason').textContent());
 // A1: one real component chain, driven by invented receipts/snapshots rather than progression timers.
 await page.evaluate(()=>{state.projects[0].tasks[0]={...state.projects[0].tasks[0],state:'held',revision:21,controls:['start']};deliver();});
 const assign=page.locator('[data-a=pm-assign]');await assign.waitFor();await assign.click();assert(await assign.isDisabled(),'one assignment remains pending');
 const start=await page.evaluate(()=>sent.filter(m=>m.a==='answer'&&m.op.action==='start').at(-1));
 await page.evaluate(m=>{taskq.receive({answerReceipt:{root:'fixture',id:m.op.id,state:'accepted'}});Object.assign(state.projects[0].tasks[0],{state:'doing',revision:22});deliver();},start);
 assert.match(await page.locator('.linked-task').innerText(),/Worker is working/);
 await page.evaluate(q=>{Object.assign(state.projects[0].tasks[0],{state:'ask',revision:30,question:q.question,card:q.card});deliver();},q);
 await page.locator('[data-a=pm-answer]').click();assert.equal(await page.locator('.conversation-composer textarea').count(),1,'PM and task share one active composer');
 await page.locator('[data-c-field=answer]').fill('Use the compatible route');await page.locator('[data-a=answer-send]').click();
 const answer=await page.evaluate(()=>sent.filter(m=>m.a==='answer'&&!m.op.action).at(-1));assert.equal(answer.op.revision,30);
 await page.evaluate(m=>taskq.receive({answerReceipt:{root:'fixture',id:m.op.id,state:'accepted'}}),answer);
 assert.equal(await page.locator('[data-c-field=text]').inputValue(),'PM draft kept','acceptance returns to the originating PM draft');
 await page.evaluate(q=>{Object.assign(state.projects[0].tasks[0],{state:'doing',revision:31,history:[{question:{...q.question,done:'Checked old implementation',evidence:[{label:'Original evidence',url:'https://example.org/evidence',image:false}]},answered:{revision:30,by:'fixture-owner',text:'Use the compatible route'}}],question:null});deliver();},q);
 await page.locator('.linked-task .thread-events>summary').click();await page.locator('.linked-task .answered').first().waitFor();
 assert.match(await page.locator('.linked-task').innerText(),/Use the compatible route/);
 assert.match(await page.locator('.linked-task').innerText(),/Checked old implementation/);assert.equal(await page.locator('.linked-task .asked a[href="https:\/\/example.org\/evidence"]').count(),1,'A7: old evidence remains accessible');
 // A4: a board answer keeps its record/offset while preceding question text reflows.
 await page.evaluate(()=>{state.projects[0].tasks[0].history[0].question.done='Checked original details '.repeat(60);state.projects[0].tasks[0].conversation.verdict='Following recorded evidence '.repeat(100);deliver();});
 const boardAnswer=page.locator('.linked-task .answered').first();
 await boardAnswer.evaluate(e=>{const r=e.closest('.conversation-reader');r.scrollTop+=e.getBoundingClientRect().top-r.getBoundingClientRect().top+5;r.dispatchEvent(new Event('scroll'));});
 const boardY=await boardAnswer.evaluate(e=>e.getBoundingClientRect().top-e.closest('.conversation-reader').getBoundingClientRect().top);
 await page.setViewportSize({width:620,height:740});
 await page.waitForTimeout(200);
 assert(Math.abs((await boardAnswer.evaluate(e=>e.getBoundingClientRect().top-e.closest('.conversation-reader').getBoundingClientRect().top))-boardY)<=2,'A4: board history keeps its answer offset across reflow');
 await page.locator('[data-c=list]').click();
 await page.locator('[data-c=select][data-id=owner-other]').click();
 await page.locator('[data-c=list]').click();
 await page.locator('[data-c=select][data-id=owner-flow]').click();
 await page.locator('.linked-task .answered').first().waitFor();
 assert(Math.abs((await boardAnswer.evaluate(e=>e.getBoundingClientRect().top-e.closest('.conversation-reader').getBoundingClientRect().top))-boardY)<=2,'A4: board answer offset survives a thread switch');
 await page.setViewportSize({width:940,height:740});
 await page.evaluate(()=>{state.projects[0].tasks[0].history[0].question.done='Checked old implementation';delete state.projects[0].tasks[0].conversation.verdict;deliver();});
 await page.evaluate(()=>{Object.assign(state.projects[0].tasks[0],{state:'review',revision:32,role:'reviewer',result:'a'.repeat(40)});deliver();});
 assert.match(await page.locator('.linked-task').innerText(),/Reviewer is checking/);
 if(process.env.TASKQ_EVIDENCE_1929){fs.mkdirSync(process.env.TASKQ_EVIDENCE_1929,{recursive:true});await page.locator('.linked-task').evaluate(e=>{const r=e.closest('.conversation-reader');r.scrollTop+=e.getBoundingClientRect().top-r.getBoundingClientRect().top;r.dispatchEvent(new Event('scroll'));});await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));await page.screenshot({path:process.env.TASKQ_EVIDENCE_1929+'/review.png'});}
 await page.evaluate(()=>{const t=state.projects[0].tasks[0];window.completedRows=[{...t,state:'closed',revision:33,conversation:{...t.conversation,verdict:'Changed: Compatible result delivered.\nOpen: none'},outcome:'success'}];state.projects[0].tasks=[];deliver();});
 await page.waitForFunction(()=>document.querySelector('.linked-task')?.textContent.includes('Compatible result delivered'));
 assert.equal(await page.locator('[data-c-field=text]').inputValue(),'PM draft kept');
 await page.evaluate(()=>{window.sourceGeneration='fixture-b';});
 await page.waitForFunction(()=>document.querySelector('.dialogue-transcript')?.textContent.includes('Transcript source changed'),null,{timeout:8000});
 assert.equal(await page.locator('.dialogue-transcript .dialogue-item').count(),0,'A4: generations never mix');
 await page.locator('[data-c=latest]').click();await page.locator('.dialogue-transcript .dialogue-item').first().waitFor();
 assert.equal(await page.locator('[data-c-field=text]').inputValue(),'PM draft kept');
 assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='answer'&&m.op.action==='start').length),1,'one held brief, one Start operation');
 if(process.env.TASKQ_EVIDENCE_1929){await page.setViewportSize({width:350,height:560});await page.locator('.conversation-reader').hover();await page.mouse.wheel(0,-500);await page.locator('[data-c=latest]').waitFor();await page.locator('.linked-task [aria-label="Final result"] .conversation-text').evaluate(e=>{const r=e.closest('.conversation-reader');r.scrollTop+=e.getBoundingClientRect().top-r.getBoundingClientRect().top;r.dispatchEvent(new Event('scroll'));});await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));await page.screenshot({path:process.env.TASKQ_EVIDENCE_1929+'/result-narrow.png'});}
 assert.deepEqual(errors,[]);console.log('PASS #1929 worker steer, immutable unknown recovery, explicit follow, unshrunk history and long-draft Send');
 }finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
