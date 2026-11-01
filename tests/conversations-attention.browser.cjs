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
 assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','conversation-action','start'].includes(m.a)).length),0);
 assert.deepEqual(errors,[]);console.log('PASS #2110 shared New, message/event reads, refusal, later arrivals, owner coalescing and archive; no live writes');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
