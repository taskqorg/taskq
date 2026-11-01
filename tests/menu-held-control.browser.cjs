// #2099 A2–A8 / K-A3/K-A6: truthful totals and scoped, fenced Held admission. All IPC is invented.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright'),views=require('../src/task-views.js');
const project={root:'/fixture/alpha',name:'Alpha',issues:'https://example.org/alpha/issues',board_kind:'github',host:'mac',alive:true,complete:true,age:1,age_text:'1s',tasks_age_seconds:0,tasks_live:true,observed:1000,focus:['all'],labels:[],
 hosts:[{name:'mac',live:true,age:1,age_text:'1s'},{name:'ali',live:false,age:2820,age_text:'47m'}],
 tasks:[{n:1,state:'doing',title:'Invented active task',revision:6,executions:[{id:6,attempt:6,host:'mac',runtime:'codex',role:'worker',state:'running',observed:999,live:true}]},{n:7,state:'doing',title:'Invented Claude task',revision:7,executions:[{id:7,attempt:7,host:'mac',runtime:'claude',role:'worker',state:'running',observed:999,live:true}]},...Array.from({length:5},(_,i)=>({n:i+2,state:'held',title:'Invented draft '+(i+1),revision:20+i,executions:[]}))]};
const navigation={schema:1,current:{page:'home'},dashboard:{selected:'running',views:[{id:'running',name:'Running',working:{states:['doing']},saved:{states:['doing']}}]}};
const palette=dark=>dark?'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--new:#6EA8FF;--review:#BCA3EB':'--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--new:#1F65C5;--review:#8250DF';
const display=structuredClone(require('./fixtures/display-model.json').display);display.all.counts=display.states.map(s=>({state:s.id,name:s.name,n:s.id==='doing'?2:s.id==='held'?5:0}));display.all.open=7;
const snapshot={display,platform:'macos',font:16,theme:'dark',host:'mac',now:1000,version:'fixture',projects:[project],notify:{projects:{}},navigation};
const out=process.env.MENU_2099_EVIDENCE,before=process.env.MENU_2099_BEFORE;
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const errors=[],html=fs.readFileSync(process.env.MENU_2099_BUNDLE||'build/web/tray.html','utf8');
 const open=async(s=snapshot,options={})=>{const p=await browser.newPage({viewport:{width:360,height:820},...options});p.on('pageerror',e=>errors.push(e.message));await p.setContent(html);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await p.evaluate(s=>taskq.receive(s),{...s,palette:palette(s.theme==='dark')});await p.evaluate(()=>document.fonts.ready);return p;};
 const messages=p=>p.evaluate(()=>messages.filter(m=>m.a==='answer'));
 if(out){fs.mkdirSync(out,{recursive:true});for(const [name,font,width] of [['narrow',16,320],['large-text',20,360]]){const p=await open({...snapshot,font},{viewport:{width,height:820}});await p.screenshot({path:out+'/'+(before?'before':'after')+'-'+name+'.png'});await p.close();}}
 let p=await open({...snapshot,navigation:{...navigation,dashboard:{...navigation.dashboard,pins:[{ref:views.identity(project)+'#2',title:'Invented pinned draft',url:project.issues+'/2'}]}}});
 assert.equal(await p.locator('.held-control').count(),1,'A4/A5: Doing-only still exposes one joined Held/Start group');
 assert.deepEqual(await p.locator('.host-runtime-totals .host-number').allTextContents(),['≥1','≥1']);
 assert.equal(await p.locator('.host-runtime-totals .host-completeness').textContent(),'Partial');
 assert.match(await p.locator('.host-runtime-totals').getAttribute('title'),/ali.*47m.*stale/i);
 await p.locator('.dashboard-status>summary').click();assert.match(await p.locator('[data-view-key="host:ali"]>summary').innerText(),/ali.*[\s\S]*47m[\s\S]*stale/i);if(out)await p.screenshot({path:out+'/host-details.png'});await p.locator('.dashboard-status>summary').click();
 const group=()=>p.locator('.held-control'),start=()=>group().locator('[data-a=held-review]'),held=()=>group().locator('[data-state=held]');
 assert.equal(await start().innerText(),'Start 5');assert.match(await start().getAttribute('aria-label'),/Start 5 held.*Alpha.*Running.*all searches/);
 assert.equal(await group().locator('button button').count(),0);
 assert.equal(await group().locator(':scope>button').count(),2);
 await held().focus();await p.keyboard.press('Space');assert.equal(await held().getAttribute('aria-pressed'),'true');assert.equal(await p.locator('[data-card]').count(),7);assert.equal((await messages(p)).length,0,'left target only filters');
 await held().click();assert.equal(await p.locator('[data-card]').count(),3);
 await start().focus();await p.keyboard.press('Enter');assert.equal(await p.getByRole('region',{name:'Review Held admission'}).count(),1);assert.equal((await messages(p)).length,0,'review never admits');if(out)await p.screenshot({path:out+'/admission-review.png'});
 // A changed revision is rejected before any bridge write. The next review takes a new snapshot.
 const changed={...project,tasks:project.tasks.map(t=>t.n===2?{...t,revision:99}:t)};
 await p.evaluate(s=>taskq.receive(s),{...snapshot,projects:[changed]});await p.getByRole('button',{name:'Confirm Start 5 held'}).click();assert.match(await p.getByRole('alert').innerText(),/changed/i);assert.equal((await messages(p)).length,0);
 await p.getByRole('button',{name:'Cancel Held admission'}).click();await start().click();await p.getByRole('button',{name:'Confirm Start 5 held'}).dblclick();
 await p.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===5);const ops=await messages(p);assert.deepEqual(ops.map(m=>[m.board,m.op.task,m.op.revision]),changed.tasks.filter(t=>t.state==='held').map(t=>[project.issues,t.n,t.revision]));assert.equal(await p.getByRole('button',{name:'Confirm Start 5 held'}).isDisabled(),true);
 await p.evaluate(s=>taskq.receive(s),{...snapshot,projects:[changed]});assert.equal((await messages(p)).length,5,'pending refresh cannot resend');assert.equal(await held().getAttribute('aria-pressed'),'false');
 for(const [i,state] of ['unknown','failed','stale','accepted','accepted'].entries())await p.evaluate(r=>taskq.receive({answerReceipt:r}),{root:project.root,id:ops[i].op.id,state,...(state==='accepted'?{reason:i===3?'ready':'waiting'}:{reason:'Invented '+state})});
 await p.locator('.answered>summary').click();assert.match(await p.locator('.content').innerText(),/ready/);assert.match(await p.locator('.content').innerText(),/waiting/);assert.match(await p.locator('.content').innerText(),/unknown|Unknown/);await p.locator('[data-a=answer-retry]').first().click();await p.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===6);assert.deepEqual((await messages(p)).at(-1).op,ops[0].op,'unknown retry keeps the fenced operation');
 assert.equal(await p.evaluate(()=>messages.some(m=>['focus','pause','resume','capacity','ai-send'].includes(m.a))),false);
 await p.close();
 // Search/project scopes precede state filters; equal issue numbers keep their own board keys.
 const beta={...project,root:'/fixture/beta',name:'Beta',issues:'https://example.org/beta/issues'};
 p=await open({...snapshot,projects:[project,beta]});assert.equal(await start().innerText(),'Start 10');await start().click();await p.getByRole('button',{name:'Confirm Start 10 held'}).click();await p.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===10);assert.equal(new Set((await messages(p)).map(m=>m.board+'/'+m.op.task)).size,10);await p.close();
 const criteria={states:['doing'],projects:[views.identity(beta)],query:'draft 2'};
 p=await open({...snapshot,projects:[project,beta],navigation:{...navigation,dashboard:{selected:'running',views:[{id:'running',name:'Running',working:criteria,saved:criteria}]}}});assert.equal(await start().innerText(),'Start 1');assert.match(await start().getAttribute('aria-label'),/Beta.*draft 2/);assert.equal(await p.locator('[data-card]').count(),0);await start().click();await p.getByRole('button',{name:'Confirm Start 1 held'}).click();await p.waitForFunction(()=>messages.some(m=>m.a==='answer'));assert.equal((await messages(p))[0].board,beta.issues);assert.equal((await messages(p))[0].op.task,3);assert.equal(await p.getByRole('searchbox',{name:'Search tasks'}).inputValue(),'draft 2');await p.close();
 for(const [tasks,complete,label] of [[[],true,'Start 0'],[project.tasks,false,'Start ?']]){p=await open({...snapshot,projects:[{...project,tasks,complete}]});assert.equal(await start().innerText(),label);assert(await start().isDisabled());assert.equal((await messages(p)).length,0);await p.close();}
 // Geometry/boundary/keyboard matrix; the two segments stay on the same wrapped line.
 let cells=0;
 for(const width of [320,360,470])for(const font of [14,16,18,20])for(const theme of ['dark','light']){
  p=await open({...snapshot,font,theme},{viewport:{width,height:820}});
  const rects=await group().locator(':scope>button').evaluateAll(es=>es.map(e=>{const r=e.getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom};}));assert.equal(rects[0].top,rects[1].top);assert(rects[0].right<=rects[1].left+.1,'separate hit areas');
  const bad=await p.locator('.filter-header,.filter-header *,.host-overview,.host-overview *').evaluateAll(es=>es.filter(e=>e.clientWidth&&e.scrollWidth>e.clientWidth+1).map(e=>e.className));assert.deepEqual(bad,[],width+'/'+font+'/'+theme);
  const a=rects[0];await p.mouse.click(a.right-1,(a.top+a.bottom)/2);assert.equal(await held().getAttribute('aria-pressed'),'true');assert.equal((await messages(p)).length,0);await p.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));const current=await start().boundingBox();await p.mouse.click(current.x+1,current.y+current.height/2);await p.getByRole('region',{name:'Review Held admission'}).waitFor({timeout:2000}).catch(async e=>{console.error({width,font,theme,current,hit:await p.evaluate(({x,y,height})=>document.elementFromPoint(x+1,y+height/2)?.outerHTML,current)});throw e;});assert.equal(await p.getByRole('region',{name:'Review Held admission'}).count(),1);assert.equal((await messages(p)).length,0);
  await p.getByRole('button',{name:'Cancel Held admission'}).click();await held().focus();await p.keyboard.press('Tab');assert(await start().evaluate(e=>e===document.activeElement));cells++;await p.close();
 }
 p=await open(snapshot,{hasTouch:true});assert(await group().locator('button').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=44)));await p.close();
 assert.deepEqual(errors,[]);console.log('PASS #2099: partial/unknown provenance, Held 0/1/many, search/projects/Doing-only, frozen revisions, one pending send, receipts/retry, boundaries, keyboard; '+cells+' geometry cells + touch');
 }finally{await browser.close()}})().catch(e=>{console.error(e);process.exitCode=1});
