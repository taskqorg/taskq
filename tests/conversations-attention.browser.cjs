// #2110 M14/TW5/A3: message, event and result sources share New membership and explicit read receipts.
// Risk: an unread question disappears from New, or bulk read consumes later arrivals/answers work.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/task-workspace.json'),board='https://example.org/issues';
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const menu=await browser.newPage(),page=await browser.newPage({viewport:{width:800,height:700}}),errors=[];
 for(const [p,file]of [[menu,'tray'],[page,'workspace']]){p.on('pageerror',e=>errors.push(e.message));await p.setContent(fs.readFileSync('build/web/'+file+'.html','utf8'));}
 const question=(n,read=false)=>({kind:'question',key:'invented-question-'+n,board,task:n,root:'fixture',project:'Project',title:'Question '+n,revision:71,state:'ask',read,archived:false});
 const state={...fx.state,font:20,theme:'dark',projects:[{...fx.state.projects[0],root:'fixture',name:'Project',issues:board,alive:true,complete:true,tasks:[{n:7,title:'Question 7',state:'ask',revision:71,question:{parsed:true,problem:'Invented choice',options:[{n:1,text:'Keep'}]},controls:[]}],events:[],reports:{complete:true,reports:[],history:[]}}],messages:{complete:true,entries:[question(7)]},conversations:{complete:true,items:[]},notify:{inbox:{}}};
 await page.evaluate(s=>{window.fixture=s;window.sent=[];window.pendingRead=null;window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='message-action')pendingRead=m;if(m.a==='notification-state'){for(const id of m.ids)fixture.notify.inbox[id]=m.action==='clear'?'cleared':'read';queueMicrotask(()=>taskq.receive(fixture));}}};taskq.receive(s);taskq.receive({open:{page:'conversations',filter:'new'}});},state);
 const entrance=async n=>{await menu.evaluate(s=>{window.ipc={postMessage(){}};taskq.receive(s);},await page.evaluate(()=>fixture));assert.match(await menu.locator('.conversations-entry').getAttribute('aria-label'),new RegExp(' '+n+' New'));};
 const count=async n=>assert.equal((await page.locator('.conversation-view>summary').innerText()).replace(/\s+/g,' '),'New · '+n);
 await entrance(1);await count(1);assert.equal(await page.locator('.conversation-row').count(),1);
 if(process.env.TASKQ_ATTENTION_EVIDENCE){fs.mkdirSync(process.env.TASKQ_ATTENTION_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.TASKQ_ATTENTION_EVIDENCE+'/attention-unread.png'});}
 assert(await page.getByRole('button',{name:'Mark all read',exact:true}).isEnabled());
 await page.getByRole('button',{name:'Mark all read',exact:true}).click();
 assert.deepEqual(await page.evaluate(()=>pendingRead.keys),['invented-question-7']);
 await count(1); // submission is not confirmed readback
 await page.evaluate(()=>taskq.receive({messageReceipt:{id:pendingRead.id,reason:'Invented read refusal',outcomes:[]}}));
 await count(1);assert(await page.getByRole('button',{name:'Mark all read',exact:true}).isEnabled());
 await page.getByRole('button',{name:'Mark all read',exact:true}).click();
 await page.evaluate(q=>{fixture.messages.entries.push(q);taskq.receive(fixture);},question(8));
 await page.evaluate(()=>{taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}});});
 await entrance(1);await count(1);assert.match(await page.locator('.conversation-row').innerText(),/#8 /,'later arrivals stay unread');
 assert.equal(await page.evaluate(()=>fixture.projects[0].tasks[0].state),'ask');
 await page.getByRole('button',{name:'Clear read',exact:true}).click();await count(1);await entrance(1);assert(await page.getByRole('button',{name:'Clear read',exact:true}).isDisabled(),'confirmed clears survive subsequent draws');
 // Owner-linked and detached request sources retain one canonical New identity each.
 await page.evaluate(()=>{fixture.notify.inbox={};fixture.messages.entries=[{...fixture.messages.entries[0],read:false},{kind:'request',id:'detached',key:'invented-request',root:'fixture',title:'Detached request',read:false,state:'failed'}];fixture.conversations.items=[{id:'owner',root:'fixture',revision:1,text:'Saved draft',files:[],sent:{root:'fixture',text:'Original',files:[]}}];fixture.projects[0].tasks[0].conversation={id:'owner'};fixture.projects[0].events=[[1,'ask','71',7]];taskq.receive(fixture);});
 await entrance(2);await count(2);assert.equal(await page.locator('.conversation-row').count(),2);
 await page.getByRole('button',{name:'Mark all read',exact:true}).click();
 assert.deepEqual(await page.evaluate(()=>pendingRead.keys),['invented-question-7','invented-request']);
 await page.evaluate(()=>taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}}));
 await count(0);await entrance(0);
 await page.evaluate(()=>{fixture.messages.entries[0].read=false;fixture.messages.entries[0].archived=true;delete fixture.projects[0].tasks[0].conversation;fixture.projects[0].events=[];fixture.conversations.items=[];taskq.receive(fixture);});
 await count(0);await entrance(0);
 if(process.env.TASKQ_ATTENTION_EVIDENCE){fs.mkdirSync(process.env.TASKQ_ATTENTION_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.TASKQ_ATTENTION_EVIDENCE+'/attention.png'});}
 // Reviewer regression: thread read must capture every message revision as well as its event,
 // without touching another board/task or a message that arrives while readback is pending.
 const thread=await browser.newPage({viewport:{width:800,height:700}});thread.on('pageerror',e=>errors.push(e.message));
 await thread.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 const threadState=structuredClone(state);threadState.messages.entries=[question(7),{...question(7),key:'invented-previous-7',revision:70},question(8),{...question(7),key:'invented-other-board-7',board:'https://other.example.org/issues'}];
 threadState.projects[0].events=[[1,'ask','71',7],[2,'ask','72',8]];
 await thread.evaluate(s=>{window.fixture=s;window.sent=[];window.pendingRead=null;window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='message-action')pendingRead=m;if(m.a==='notification-state'){for(const id of m.ids)fixture.notify.inbox[id]=m.action==='clear'?'cleared':'read';queueMicrotask(()=>taskq.receive(fixture));}}};taskq.receive(s);taskq.receive({open:{page:'conversations',board:s.projects[0].issues,task:7,filter:'new'}});},threadState);
 const threadCount=async n=>assert.equal((await thread.locator('.conversation-view>summary').innerText()).replace(/\s+/g,' '),'New · '+n);
 await threadCount(3);
 const history=thread.locator('.thread-events');await history.locator('summary').click();
 const readThread=thread.getByRole('button',{name:'Mark thread read',exact:true});await readThread.click();
 assert.deepEqual(await thread.evaluate(()=>pendingRead?.keys),['invented-question-7','invented-previous-7'],'thread read includes all exact message sources');
 assert(await readThread.isDisabled(),'pending read cannot be resubmitted');
 assert.equal(await thread.evaluate(()=>fixture.messages.entries[0].read),false,'submission is not readback');
 assert.deepEqual(await thread.evaluate(()=>sent.filter(m=>m.a==='notification-state').flatMap(m=>m.ids).map(id=>JSON.parse(id)[2])),[7],'other task events stay unread');
 await thread.evaluate(()=>taskq.receive({messageReceipt:{id:pendingRead.id,reason:'Invented thread read refusal',outcomes:[]}}));
 assert(await readThread.isEnabled(),'message refusal keeps thread read available after its event was read');
 if(process.env.TASKQ_ATTENTION_EVIDENCE)await thread.screenshot({path:process.env.TASKQ_ATTENTION_EVIDENCE+'/thread-read.png'});
 await readThread.click();
 await thread.evaluate(q=>{fixture.messages.entries.push({...q,key:'invented-later-7',revision:72});taskq.receive(fixture);taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}});},question(7));
 assert(await readThread.isEnabled(),'later revision remains unread');await threadCount(3);
 assert.deepEqual(await thread.evaluate(()=>fixture.messages.entries.filter(r=>!r.read).map(r=>r.key)),['invented-question-8','invented-other-board-7','invented-later-7']);
 await readThread.click();assert.deepEqual(await thread.evaluate(()=>pendingRead.keys),['invented-later-7']);
 await thread.evaluate(()=>taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}}));
 assert.equal(await readThread.count(),0,'button disappears only after all captured thread sources are read');await threadCount(2);
 await menu.evaluate(s=>taskq.receive(s),await thread.evaluate(()=>fixture));assert.match(await menu.locator('.conversations-entry').getAttribute('aria-label'),/2 New · 1 Needs me/);
 assert.equal(await thread.evaluate(()=>fixture.projects[0].tasks[0].state),'ask','read never answers a question');
 // No event row is required for the thread-level action.
 await thread.evaluate(()=>{fixture.projects[0].events=[];fixture.messages.entries[0].read=false;taskq.receive(fixture);});
 await readThread.click();assert.deepEqual(await thread.evaluate(()=>pendingRead.keys),['invented-question-7'],'message-only thread has the same exact read action');
 await thread.evaluate(()=>taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}}));
 assert.equal(await readThread.count(),0);assert.equal(await thread.evaluate(()=>fixture.messages.entries[2].read),false);
 // Linked PM cards use the same read scope, including their event sources.
 await thread.evaluate(()=>{fixture.messages.entries[0].read=false;fixture.projects[0].events=[[1,'ask','71',7]];fixture.notify.inbox={};fixture.conversations.items=[{id:'owner',root:'fixture',revision:1,text:'Saved draft',files:[],sent:{root:'fixture',text:'Original',files:[]}}];fixture.projects[0].tasks[0].conversation={id:'owner',brief:'Invented brief'};taskq.receive(fixture);});
 await thread.locator('[data-c=select][data-id=owner]').click();
 const linkedHistory=thread.locator('.linked-task .thread-events');await linkedHistory.locator('summary').click();await readThread.click();
 assert.deepEqual(await thread.evaluate(()=>pendingRead.keys),['invented-question-7']);
 assert.equal(await thread.evaluate(()=>sent.filter(m=>m.a==='notification-state').length),2,'linked card reads its event as well as its message');
 await thread.evaluate(()=>taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}}));await threadCount(2);
 // A finished task may still have unread question news; result reads remain SHA-bound.
 await thread.evaluate(()=>{fixture.conversations.items=[];delete fixture.projects[0].tasks[0].conversation;fixture.projects[0].tasks[0].state='closed';fixture.messages.entries[0].read=false;fixture.projects[0].reports.reports=[{n:7,sha:'a'.repeat(40),kind:'task',title:'Finished question 7',closed:3,read:false,visible:true,body:'Changed: Invented completion\nChecks: fixture\nOpen: none'}];taskq.receive(fixture);taskq.receive({open:{page:'conversations',board:fixture.projects[0].issues,task:7}});});
 const resultHistory=thread.locator('.thread-events');if(!await readThread.isVisible())await resultHistory.locator('summary').click();
 await readThread.click();assert.deepEqual(await thread.evaluate(()=>pendingRead.keys),['invented-question-7']);
 const resultRead=await thread.evaluate(()=>sent.filter(m=>m.a==='reports'&&m.action==='read').at(-1));assert.equal(resultRead.sha,'a'.repeat(40));assert.equal(resultRead.n,7);
 await thread.evaluate(m=>{const report=fixture.projects[0].reports.reports[0];report.read=true;taskq.receive({reportReceipt:{id:m.id,root:m.root,report}});taskq.receive({messageReceipt:{id:pendingRead.id,outcomes:pendingRead.keys.map(key=>({key,action:'read',state:'accepted'}))}});},resultRead);
 await threadCount(2);assert.equal(await readThread.count(),0);
 assert(await thread.evaluate(()=>sent.filter(m=>['answer','conversation-action','start'].includes(m.a)).every(m=>m.a==='conversation-action'&&m.action==='save'&&m.draft.text==='Saved draft')),'only the unchanged fixture draft may be saved; no send, answer or admission');await thread.close();
 assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','conversation-action','start'].includes(m.a)).length),0);
 assert.deepEqual(errors,[]);console.log('PASS #2110 shared New, message/event reads, refusal, later arrivals, owner coalescing and archive; no live writes');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
