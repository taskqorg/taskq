// #1829 A1-A5 / R6, P-W1: exact pin reads, safe closed actions/expiry and archived menu entry.
// Invented board/native receipts; no live tasks, sessions or archive writes.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright'),views=require('../src/task-views.js');
const out=process.env.TASKQ_EVIDENCE_1829;if(out)fs.mkdirSync(out,{recursive:true});
const project=(kind,name,issues)=>({root:'fixture-'+name,name,board_kind:kind,issues,alive:true,complete:true,tasks:[],focus:['all'],labels:[]});
const gh=project('github','Alpha','https://github.example/org/alpha/issues'),gl=project('gitlab','Servers','https://gitlab.example/org/servers/-/issues');
const ref=(p,n)=>views.identity(p)+'#'+n,pin=(p,n)=>({ref:ref(p,n),title:'Pinned task '+n,url:p.issues+'/'+n});
const model=views.normalize({pins:[pin(gh,1),pin(gl,2),pin(gh,3)]});
const state={platform:'macos',font:16,theme:'dark',host:'fixture',now:200000,version:'fixture',projects:[gh,gl],navigation:{schema:1,current:{page:'home'},dashboard:model},display:require('./fixtures/display-model.json').display,ai:{draft:''},notify:{projects:{}}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const p=await browser.newPage({viewport:{width:420,height:780}}),errors=[];p.on('pageerror',e=>errors.push(e.message));
 await p.clock.install();await p.setContent(fs.readFileSync('build/web/tray.html','utf8'));await p.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};});await p.evaluate(s=>taskq.receive(s),state);
 const requests=()=>p.evaluate(()=>sent.filter(m=>m.a==='completed'&&m.task));
 assert.equal((await requests()).length,3,'A1: missing pins request exact issues without opening/paging Completed');
 const reply=async(n,extra={})=>{const m=(await requests()).find(m=>m.task===n);await p.evaluate(({m,extra})=>taskq.receive({completed:{root:m.root,id:m.id,tasks:[{n:m.task,state:'closed',title:'Closed task '+m.task,url:'https://forge.example/issues/'+m.task,closed:199990,outcome:'success',close:'delivered',pr:'https://forge.example/pull/7',...extra}]}}),{m,extra});};
 await reply(1,{merged:'merged #7 as abc'});await reply(2);const missing=(await requests()).find(m=>m.task===3);await p.evaluate(m=>taskq.receive({completed:{root:m.root,id:m.id,reason:'Board unavailable'}}),missing);
 const row=n=>p.locator('[data-pin="'+ref(n===2?gl:gh,n)+'"]');
 assert.match(await row(1).innerText(),/Merged.*delivered/s);assert.equal(await row(1).locator('[aria-label=Merged] svg').count(),1);
 assert.match(await row(2).innerText(),/Completed/);assert.equal(await row(2).locator('[aria-label=Completed] svg').count(),1,'GitLab closure is success without state_reason');
 assert.match(await row(3).innerText(),/state unknown.*Board unavailable/s);assert.equal(await row(3).getByRole('button',{name:/Retry/}).count(),1);
 assert.equal(await row(1).locator('[data-a=workspace],[data-a=control]').count(),0,'closed action set never continues a session');
 if(out)await p.screenshot({path:out+'/01-closed-pins-and-unreadable.png'});
 await row(1).getByRole('button',{name:/Open transcript/}).click();
 const destination=await p.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open);assert.deepEqual([destination.page,destination.root,destination.task],['conversations',gh.root,1],'closed pin opens the exact common reader; transcript/refusal protection is in conversations-centre.browser.cjs');
 assert.equal(await row(1).count(),0,'A3: explicit transcript open removes closed pin');
 if(out)await p.screenshot({path:out+'/02-read-only-transcript.png'});
 await p.keyboard.press('Escape');
 await p.clock.fastForward(86390000);assert.equal(await row(2).count(),0,'A3: closed pin expires at 24 h');assert.equal(await row(3).count(),1,'unreadable pin never expires as if closed');
 await row(3).getByRole('button',{name:/Unpin/}).click();assert.equal(await row(3).count(),0,'manual Unpin works on unavailable pins');
 await p.locator('[data-a=view-select][data-id=completed]').click();
 const completed=await p.evaluate(()=>sent.filter(m=>m.a==='workspace').at(-1).open);assert.equal(completed.filter,'finished','unpin never deletes Completed history; common projection pagination is protected in reports.browser.cjs');
 const links=await browser.newPage({viewport:{width:420,height:780}});await links.setContent(fs.readFileSync('build/web/tray.html','utf8'));await links.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};});
 await links.evaluate(s=>taskq.receive(s),{...state,navigation:{...state.navigation,dashboard:views.normalize({pins:[pin(gh,1)]})}});
 const linkRead=await links.evaluate(()=>sent.find(m=>m.a==='completed'));
 await links.evaluate(m=>taskq.receive({completed:{id:m.id,root:m.root,tasks:[{n:1,state:'closed',title:'Closed task',url:'https://forge.example/issues/1',closed:199990,outcome:'success'}]}}),linkRead);
 await links.getByRole('link',{name:'Open issue',exact:true}).click();await links.locator('[data-pin]').waitFor({state:'detached'});
 assert.deepEqual(await links.evaluate(()=>sent.filter(m=>m.a==='open').map(m=>m.url)),['https://forge.example/issues/1'],'issue opening sends a board URL, never a native session target');await links.close();
 const w=await browser.newPage({viewport:{width:940,height:740}});w.on('pageerror',e=>errors.push(e.message));await w.setContent(fs.readFileSync('build/web/workspace.html','utf8'));await w.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>sent.push(JSON.parse(t))};});
 const id='task:'+gh.issues+'/4',q={n:4,title:'Archived question',state:'ask',revision:44,controls:[],card:'Answer only after Restore'};
 const ws={...state,projects:[{...gh,tasks:[q]}],conversations:{complete:true,items:[],sidebar:{pins:[],collapsed:[],archived:{[id]:gh.root}}},messages:{complete:true,entries:[]}};
 await w.evaluate(s=>taskq.receive(s),ws);await w.evaluate(p=>taskq.receive({open:{page:'answer',board:p.issues,root:p.root,task:4}}),gh);
 assert.equal(await w.locator('[data-c=archive-list]').getAttribute('aria-pressed'),'true','A4: menu task entry selects Archive');assert.match(await w.locator('.thread-title').innerText(),/Archived question/);
 assert.equal(await w.locator('[data-c-field=answer],[data-a=answer-send],[data-qac=choose],[data-a=steer-send]').count(),0,'Archive task thread is read-only');assert.equal(await w.locator('.conversation-detail').getByRole('button',{name:'Restore',exact:true}).count(),1);
 if(out)await w.screenshot({path:out+'/03-archived-menu-entry.png'});
 // An archived question event is the other archive representation; direct key entry must obey it too.
 ws.conversations.sidebar.archived={};ws.messages.entries=[{key:'a'.repeat(64),kind:'question',board:gh.issues,task:4,root:gh.root,title:q.title,state:'ask',archived:true,read:true}];await w.evaluate(s=>taskq.receive(s),ws);await w.evaluate(key=>taskq.receive({open:{page:'message',key}}),ws.messages.entries[0].key);
 assert.equal(await w.locator('[data-c=archive-list]').getAttribute('aria-pressed'),'true');assert.equal(await w.locator('[data-a=answer-send],[data-qac=choose]').count(),0);

 assert.equal(await w.evaluate(()=>sent.filter(m=>['conversation-action','message-action','answer','ai-send'].includes(m.a)).length),0,'opening Archive never relaunches, restores or answers');assert.deepEqual(errors,[]);
 console.log('PASS #1829 A1-A5: exact GitHub/GitLab pins outside Completed, unknown/retry, read-only transcript/actions, first-open/24h unpin, archived local/event reopen');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
