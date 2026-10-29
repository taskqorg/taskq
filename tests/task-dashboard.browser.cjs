// #1578 A2–A8 / O1–O3, #1659 A1–A7: accepted dashboard behavior and geometry; invented data, no live board mutations.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.DASHBOARD_EVIDENCE;
const task=(n,state,extra={})=>({n,state,title:'Keep the complete task title readable across narrow windows and large type '+n,revision:n+50,since:900,created:500,steps:[{state:'ready',start:500,end:900,complete:true},{state,start:900,end:null,complete:false}],controls:state==='failed'?['retry']:state==='held'?['start']:['stop','later'],...extra});
const alpha={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],tasks:[task(3,'ready',{rank:3}),task(5,'doing',{rank:0,runtime:'claude',host:'remote-host',local_session:false,age:'4m'}),task(2,'failed',{rank:2}),task(4,'held',{rank:5})],delivery:{preset:'auto-merge',revision:0}};
const beta={...alpha,root:'/fixture/beta',name:'Beta',issues:'https://example.org/org/beta/issues',tasks:[task(1,'review',{rank:1}),task(3,'ready',{rank:4})]};
const state={platform:'macos',font:16,theme:'dark',host:'fixture',now:1000,version:'fixture',size_anchor:'auto',attached_edge:'top',top:true,service:{state:'running'},projects:[beta,alpha,{...alpha,root:'/fixture/duplicate',tasks:alpha.tasks.map(t=>({...t,dup:true}))}],ai:{draft:''},notify:{projects:{}},stop_modes:true,display:require('./fixtures/display-model.json').display,messages:{complete:true,count:1}};
const html=fs.readFileSync('build/web/tray.html','utf8');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const open=async(s=state,viewport={width:360,height:760})=>{const p=await browser.newPage({viewport});p.on('pageerror',e=>{throw e;});await p.setContent(html);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await p.evaluate(s=>taskq.receive(s),s);return p;};
  let p=await open();const list=()=>p.locator('[data-card]'),editor=()=>p.locator('#taskq-popover');
  // #1775 M1: task search is an icon until opened or a query is active; a query keeps the field visible.
  const query=async()=>{const q=p.locator('[data-task-query]');if(!await q.count())await p.locator('[data-a=search-open]').click();return q;};
  assert.equal(await p.locator('[data-a=view-editor]').count(),1,'before: approved overlay editor missing; after: reachable');
  assert.equal(await list().count(),6,'canonical duplicate registrations count once');
  assert.equal(await p.getByRole('button',{name:'Conversations · 1 new',exact:true}).count(),1);assert.equal(await p.getByRole('button',{name:/^Inbox/}).count(),0);
  await p.locator('[data-a=view-editor]').click();assert(await editor().evaluate(e=>e.matches(':popover-open')));
  const before=await list().first().boundingBox();
  await editor().locator('[data-facet=projects][data-value="github://example.org/org/alpha"]').click();
  assert.equal(await list().count(),4);assert.match(await p.locator('[data-a=all-tasks]').first().innerText(),/4/);
  await editor().locator('[data-facet=states][data-value=doing]').click();await editor().locator('[data-facet=states][data-value=failed]').click();
  assert.equal(await list().count(),2);assert.match(await p.locator('[data-a=all-tasks]').first().innerText(),/4/,'All counts selected projects before state filtering');
  const after=await list().first().boundingBox();assert.equal(after.x,before.x,'overlay never displaces list');
  await editor().locator('[data-a=view-save]').click();await editor().locator('[data-a=popup-close]').first().click();
  await (await query()).fill('complete');assert.equal(await list().count(),2);
  await p.locator('[data-a=view-list]').click();await editor().locator('[data-a=view-save-as]').click();await editor().locator('[data-view-name]').fill('Attention');await editor().locator('[data-a=view-add]').click();
  await (await query()).fill('5');assert.equal(await list().count(),1);
  await p.locator('[data-a=view-select][data-id=all]').click();assert.equal(await (await query()).inputValue(),'complete','switch restores unsaved working query');
  await p.locator('[data-a=view-select]').filter({hasText:'Attention'}).click();assert.equal(await (await query()).inputValue(),'5');
  await p.waitForFunction(()=>messages.some(m=>m.a==='set'&&m.navigation?.dashboard?.views?.length===3));
  const navigation=await p.evaluate(()=>messages.filter(m=>m.a==='set'&&m.navigation?.dashboard?.views?.length===3).at(-1).navigation);
  await p.close();p=await open({...state,navigation});assert.equal(await (await query()).inputValue(),'5','restart retains each view working context');
  await p.locator('[data-a=view-editor]').click();await editor().locator('[data-a=view-reset]').click();assert.equal(await (await query()).inputValue(),'complete','Reset restores explicitly saved criteria');
  await editor().locator('[data-a=popup-close]').first().click();
  // All clears only the state filter. Project/search are independent and remain visible.
  await p.locator('[data-a=all-tasks]').click();assert.equal(await list().count(),4);
  await (await query()).fill('');await p.locator('[data-a=view-editor]').click();await editor().locator('[data-facet=projects][data-value=""]').click();await editor().locator('[data-a=view-group][data-value=combined]').click();await editor().locator('[data-a=popup-close]').first().click();
  const identities=await list().locator('[data-a=task-detail]').evaluateAll(es=>es.map(e=>e.textContent.trim().replace(/^GitHub /,'')));
  assert.deepEqual(identities,['Alpha #5','Beta #1','Alpha #2','Alpha #3','Beta #3','Alpha #4'],'O2 one comparator with canonical repository tie');
  assert.equal(await p.locator('[data-a=row-menu],.row-menu').count(),0,'no right ellipsis or action column');
  const identity=p.locator('[data-a=task-detail]').filter({hasText:'Alpha #5'});
  // #1659 A2: the remote mark (proven remote run) then the Claude mark sit right before the current-stage elapsed time.
  assert.deepEqual(await list().filter({has:identity}).locator('.tqp-task-head').evaluate(e=>[...e.querySelectorAll('[data-icon],.tqp-age')].map(x=>x.dataset.icon||x.textContent)),['play','github','network','claude','4m']);
  assert.equal(await list().filter({hasText:'Beta #1'}).locator('.task-stage.review').count(),1,'Review uses the purple review tone');
  // #1659 A1: hover and focus never open details; Enter does. IDs, age and steps stay behind a disclosure; no pin latch.
  await identity.hover();await identity.focus();assert.equal(await editor().count(),0,'hover/focus open nothing');assert.equal(await identity.getAttribute('aria-expanded'),'false');
  await p.keyboard.press('Enter');assert.equal(await identity.getAttribute('aria-expanded'),'true');assert.equal(await editor().getByRole('button',{name:'Keep task details open'}).count(),0);
  assert(!/Total age|Attempt/.test(await editor().innerText()),'technical details start closed');assert(!await list().first().innerText().then(t=>/Total age|completed|runtime|host/.test(t)));
  await editor().locator('summary').click();if(out){fs.mkdirSync(out,{recursive:true});await p.screenshot({path:out+'/dark-details.png'});}assert.match(await editor().innerText(),/Total age\s*\S+[\s\S]*1 completed/);assert.match(await editor().innerText(),/ready[\s\S]*doing[\s\S]*current/);
  assert.equal(await editor().locator('[data-a=control]').count(),2,'all offered revision-fenced controls retained');
  await p.keyboard.press('Escape');assert.equal(await editor().count(),0);assert(await identity.evaluate(e=>e===document.activeElement),'Escape restores identity focus');
  // #1659 A5: a global pin closes details and shows the task once, first, in every view and filter; it survives restart.
  await identity.click();await editor().getByRole('button',{name:'Pin task',exact:true}).click();assert.equal(await editor().count(),0,'pinning closes details');
  assert.equal(await list().count(),6,'pinned task appears once');assert.match(await p.locator('.task-pins').innerText(),/Alpha #5/);
  await p.locator('[data-a=view-editor]').click();await editor().locator('[data-facet=states][data-value=held]').click();await editor().locator('[data-a=popup-close]').first().click();
  assert.deepEqual(await list().locator('[data-a=task-detail]').evaluateAll(es=>es.map(e=>e.textContent.trim().replace(/^GitHub /,''))),['Alpha #5','Alpha #4'],'pin stays first outside the filter');
  await p.waitForFunction(()=>messages.some(m=>m.a==='set'&&m.navigation?.dashboard?.pins?.length===1));
  const pinned=await p.evaluate(()=>messages.filter(m=>m.a==='set'&&m.navigation?.dashboard?.pins?.length===1).at(-1).navigation);
  await p.close();p=await open({...state,navigation:pinned});assert.match(await p.locator('.task-pins').innerText(),/Alpha #5/,'pins survive restart');
  await p.locator('[data-a=view-select][data-id=completed]').click();assert.match(await p.locator('.task-pins').innerText(),/Alpha #5/,'pins show in Completed');
  // #1659 A6: Completed reads one board page per project on demand; outcomes stay distinct; Load more asks for the next page.
  const asks=await p.evaluate(()=>messages.filter(m=>m.a==='completed'));assert.deepEqual(asks.map(m=>[m.root,m.page,typeof m.since]).sort(),[['/fixture/alpha',1,'number'],['/fixture/beta',1,'number']]);
  const alphaAsk=asks.find(m=>m.root==='/fixture/alpha');
  await p.evaluate(a=>taskq.receive({completed:{root:a.root,id:a.id,page:1,more:true,tasks:[{n:7,title:'Merged work',url:'https://example.org/org/alpha/issues/7',closed:950,outcome:'success',result:'Shipped the fix',pr:'https://example.org/org/alpha/pull/8',merged:'merged #8 as 0123456; merge-result CI green'},{n:8,title:'Linked only',url:'https://example.org/org/alpha/issues/8',closed:945,outcome:'success',pr:'https://example.org/org/alpha/pull/9',merged:null},{n:6,title:'Dropped idea',url:'https://example.org/org/alpha/issues/6',closed:940,outcome:'not_planned'},{n:1,title:'Old close',url:'https://example.org/org/alpha/issues/1',closed:930,outcome:'unknown'}]}}),alphaAsk);
  const done=await p.locator('[data-completed]').allInnerTexts();assert.equal(done.length,4);assert.match(done[0],/Merged[\s\S]*Shipped the fix[\s\S]*PR/);
  // #1679 A3: a success with a PR link but no merge receipt reads Completed and keeps the PR link.
  assert.match(done[1],/Completed[\s\S]*PR/);assert.doesNotMatch(done[1],/Merged/);assert.match(done[2],/Not planned/);assert.match(done[3],/Outcome unknown/);
  assert.equal(await p.locator('[data-completed] [data-icon="git-merge"],[data-completed] .ui-icon').first().evaluate(e=>!!e.querySelector('svg')),true);
  if(out){fs.mkdirSync(out,{recursive:true});await p.screenshot({path:out+'/dark-completed.png'});}
  await (await query()).fill('dropped');assert.equal(await p.locator('[data-completed]').count(),1,'search covers loaded history');await (await query()).fill('');
  await p.locator('[data-a=completed-more]').click();assert(await p.evaluate(()=>messages.some(m=>m.a==='completed'&&m.root==='/fixture/alpha'&&m.page===2)));
  // #1711 A1–A4: delayed sources retain displayed rows through period/outcome changes and Reset + Done.
  const historyRequests=()=>p.evaluate(()=>messages.filter(m=>m.a==='completed'));
  const reply=(request,tasks=[],extra={})=>p.evaluate(page=>taskq.receive({completed:page}),{...request,tasks,more:false,...extra});
  await reply((await historyRequests()).find(m=>m.root==='/fixture/beta'),[{n:9,title:'Beta history',closed:960,outcome:'success'}]);
  await reply((await historyRequests()).find(m=>m.root==='/fixture/alpha'&&m.page===2),[]);
  await p.clock.install();
  const retained=await p.locator('[data-completed]').allInnerTexts();
  await p.locator('[data-a=view-editor]').click();await editor().locator('[data-a=view-period][data-value="7d"]').click();
  await editor().locator('[data-facet=outcomes][data-value="not_planned"]').click();await editor().getByRole('button',{name:'Done',exact:true}).click();
  assert.deepEqual(await p.locator('[data-completed]').allInnerTexts(),retained,'A1: slow filter reload keeps the previous rows');
  const source=name=>p.locator('[data-history-source]').filter({hasText:name+':'});
  assert.match(await source('Alpha').innerText(),/Reading page 1.*previous rows/);assert.equal(await source('Alpha').getAttribute('role'),'status');
  let requests=await historyRequests(),alphaReload=requests.filter(m=>m.root==='/fixture/alpha').at(-1),betaReload=requests.filter(m=>m.root==='/fixture/beta').at(-1);
  await reply(alphaReload,[],{reason:'Board unavailable'});
  assert.deepEqual(await p.locator('[data-completed]').allInnerTexts(),retained,'A1: failed reload retains rows');
  assert.equal(await source('Alpha').getAttribute('role'),'alert');assert.match(await source('Alpha').innerText(),/Board unavailable/);
  await p.clock.fastForward(65000);
  assert.match(await source('Beta').innerText(),/No history response within 65 seconds/);assert.equal(await source('Beta').getAttribute('role'),'alert');
  if(out){await source('Alpha').scrollIntoViewIfNeeded();await p.screenshot({path:out+'/completed-reload-errors.png'});}
  await source('Alpha').getByRole('button',{name:'Retry Alpha history'}).click();
  assert.equal((await historyRequests()).filter(m=>m.root==='/fixture/beta').length,2,'Retry targets only the failed source');
  const retried=(await historyRequests()).filter(m=>m.root==='/fixture/alpha').at(-1);assert.equal(retried.page,1);
  await reply(alphaReload,[{n:99,title:'Stale reply',closed:999,outcome:'not_planned'}]);assert.equal(await p.getByText('Stale reply',{exact:true}).count(),0,'old replies cannot replace retained rows');
  await reply(retried,[{n:6,title:'Dropped idea refreshed',closed:940,outcome:'not_planned'}]);
  assert.equal(await p.getByText('Merged work',{exact:true}).count(),0,'first successful page replaces the old source rows');assert.match(await source('Alpha').innerText(),/Page 1 loaded/);
  await p.locator('[data-a=view-editor]').click();await editor().getByRole('button',{name:'Reset',exact:true}).click();await editor().getByRole('button',{name:'Done',exact:true}).click();
  assert(await p.locator('[data-completed]').count()>0,'A3: Reset + Done never blanks the list while reloading');
  if(out)await p.screenshot({path:out+'/completed-reset-loading.png'});
  requests=await historyRequests();
  await reply(betaReload,[{n:99,title:'Timed-out reply',closed:999,outcome:'success'}]);assert.equal(await p.getByText('Timed-out reply',{exact:true}).count(),0);
  await reply(requests.filter(m=>m.root==='/fixture/alpha').at(-1),[{n:7,title:'Full history restored',closed:950,outcome:'success'}],{more:true});
  await reply(requests.filter(m=>m.root==='/fixture/beta').at(-1),[]);
  assert.deepEqual(await p.locator('[data-completed] .detail-title').allInnerTexts(),['Full history restored'],'A3: Reset restores all outcomes');
  await reply(requests.filter(m=>m.root==='/fixture/alpha').at(-1),[{n:99,title:'Duplicate reply',closed:999,outcome:'success'}]);
  assert.equal(await p.getByText('Duplicate reply',{exact:true}).count(),0,'duplicate terminal replies are ignored');
  await p.locator('[data-a=completed-more]').click();
  const nextPage=(await historyRequests()).filter(m=>m.root==='/fixture/alpha').at(-1);assert.equal(nextPage.page,2);
  await reply(nextPage,[],{reason:'Next page failed'});assert.equal(await p.getByText('Full history restored',{exact:true}).count(),1,'paging failure keeps prior pages');
  await source('Alpha').getByRole('button',{name:'Retry Alpha history'}).click();
  const retryPage=(await historyRequests()).filter(m=>m.root==='/fixture/alpha').at(-1);assert.equal(retryPage.page,2,'paging Retry requests the failed page');await reply(retryPage,[]);
  assert.equal(await p.getByText('Full history restored',{exact:true}).count(),1,'paging Retry appends instead of replacing prior pages');
  await p.locator('[data-a=view-select][data-id=all]').click();await p.locator('.task-pins [data-a=task-detail]').click();await editor().getByRole('button',{name:'Unpin task',exact:true}).click();assert.equal(await p.locator('.task-pins').count(),0);
  // #1659 A4 (P-V1): custom views delete by keyboard, built-ins cannot; Undo restores the view and its context.
  await p.locator('[data-a=view-list]').click();await editor().locator('[data-a=view-select]').filter({hasText:'Attention'}).click();await (await query()).fill('5');
  await p.locator('[data-a=view-list]').click();assert.equal(await editor().locator('[data-a=view-delete]').count(),1,'only the custom view is deletable');
  await editor().getByRole('button',{name:'Delete view Attention'}).focus();await p.keyboard.press('Enter');assert.match(await editor().locator('.view-receipt').innerText(),/Deleted\s*Attention/);assert.equal(await editor().locator('[data-a=view-select]').count(),2);assert.notEqual(await (await query()).inputValue(),'5','active delete falls back to All tasks');
  await editor().getByRole('button',{name:'Undo deleting Attention'}).click();assert.equal(await editor().locator('[data-a=view-select]').count(),3);assert.equal(await (await query()).inputValue(),'5','unchanged fallback lets Undo reselect the view');
  await editor().locator('[data-a=view-select]').filter({hasText:'Attention'}).click();assert.equal(await (await query()).inputValue(),'5','Undo restores working context');
  await p.locator('[data-a=view-select][data-id=all]').click();
  await p.locator('[data-route=settings]').click();await p.locator('[data-route=appearance]').click();
  for(const mode of ['auto','top','bottom','fixed']){await p.locator('[data-a=size-anchor][data-value='+mode+']').click();assert(await p.evaluate(mode=>messages.some(m=>m.a==='size-anchor'&&m.value===mode),mode));}
  await p.locator('[data-resize]').focus();await p.keyboard.press('ArrowRight');assert(await p.evaluate(()=>messages.some(m=>m.a==='resize')));
  await p.getByRole('button',{name:'Close menu',exact:true}).click();assert(await p.evaluate(()=>messages.some(m=>m.a==='hide')));
  assert.equal(await p.evaluate(()=>messages.some(m=>['focus','pause','resume','capacity','ai-send'].includes(m.a))),false,'view/window controls have no admission/grant/send effects');
  await p.close();
  p=await open({...state,projects:[alpha,{...alpha,root:'/fixture/conflict',partial:['Alpha'],tasks:alpha.tasks.map(t=>({...t,title:'Changed title',revision:t.revision+1}))}]});
  await (await query()).fill('complete');assert.equal(await p.locator('[data-a=all-tasks]').innerText(),'All tasks ?','R12: search cannot hide a same-board conflict');await p.close();
  const observations=[];
  for(const theme of ['dark','light'])for(const font of [16,20])for(const variant of ['long','short']){
   p=await open({...state,font,theme,projects:variant==='long'?[{...alpha,tasks:Array.from({length:16},(_,i)=>task(i+1,i%2?'doing':'failed',{title:('Long unbroken identifier_'+i+'_').repeat(5),since:i%2?null:900}))}]:[{...alpha,complete:false,partial:['Alpha'],tasks:[]}]},{width:320,height:variant==='short'?300:700});
   const geometry=await p.evaluate(()=>{const c=document.querySelector('.content'),head=document.querySelector('.head').getBoundingClientRect(),footer=document.querySelector('.status-footer').getBoundingClientRect();return {overflow:document.documentElement.scrollWidth-innerWidth,contentOverflow:c.scrollWidth-c.clientWidth,head:head.bottom,content:c.getBoundingClientRect().top,bottom:c.getBoundingClientRect().bottom,footer:footer.top,last:footer.bottom,viewport:innerHeight};});
   assert(geometry.overflow<=0&&geometry.contentOverflow<=0,'no horizontal overflow '+JSON.stringify(geometry));assert(geometry.head<=geometry.content+.5&&geometry.bottom<=geometry.footer+.5&&geometry.last<=geometry.viewport+.5,'fixed chrome never overlaps content');
   await p.locator('[data-a=view-editor]').click();const popup=await editor().boundingBox();assert(popup.x>=0&&popup.y>=0&&popup.x+popup.width<=320.5&&popup.y+popup.height<=geometry.viewport+.5,'bounded anchored editor');
   if(out&&font===16){fs.mkdirSync(out,{recursive:true});await p.screenshot({path:out+'/'+theme+'-'+variant+'-editor.png'});}
   if(variant==='short'){const done=editor().getByRole('button',{name:'Done',exact:true});await done.scrollIntoViewIfNeeded();const end=await done.boundingBox();assert(end.y>=0&&end.y+end.height<=geometry.viewport+.5,'last overlay control remains reachable in short windows');await done.click();assert.equal(await editor().count(),0);}else await p.keyboard.press('Escape');if(out&&font===16)await p.screenshot({path:out+'/'+theme+'-'+variant+'.png'});
   if(variant==='long'){const t=p.locator('[data-a=task-detail]').first();await t.click();assert.match(await editor().innerText(),/elapsed unknown/);await p.keyboard.press('Escape');}
   else {assert.equal((await p.locator('.filter-header').innerText()).includes('?'),true,'partial never zero');}
   observations.push({theme,font,variant,...geometry});await p.close();
  }
  // #1700 A2–A5 (P-W4): with native windows the menu opens the one View window; that window edits the menu's model.
  const eight=Array.from({length:8},(_,i)=>({...alpha,root:'/fixture/p'+i,name:'Project '+i,issues:'https://example.org/org/p'+i+'/issues',tasks:i?[]:[task(1,'review',{rank:0})]})),wide={...state,projects:eight,palette:'--review:#BCA3EB'};
  p=await open(wide);await p.evaluate(()=>taskq.receive({settingsWindow:true,viewsWindow:true}));
  for(const a of ['view-editor','view-list']){await p.locator('[data-a='+a+']').click();assert.equal(await editor().count(),0,'no overlay with a View window');}
  assert.equal(await p.evaluate(()=>messages.filter(m=>m.a==='views-open').length),2,'both view controls open the View window');
  await p.locator('[data-a=task-detail]').first().click();assert.equal(await editor().locator('.body.meta .review').evaluate(e=>getComputedStyle(e).color),'rgb(188, 163, 235)','A4: detail state line Review token');await p.keyboard.press('Escape');
  await p.waitForFunction(()=>messages.some(m=>m.a==='set'&&m.navigation?.dashboard));
  const model=await p.evaluate(()=>messages.filter(m=>m.a==='set'&&m.navigation?.dashboard).at(-1).navigation.dashboard);
  model.views.push({...model.views[0],id:'attention',name:'Attention'});
  const w=await browser.newPage({viewport:{width:480,height:600}});w.on('pageerror',e=>{throw e;});await w.setContent(html.replace('<head>','<head><script>window.taskqSurface=\'views\';</script>'));
  await w.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await w.evaluate(s=>taskq.receive(s),wide);
  assert.equal(await w.locator('[data-a=view-facet]').count(),0,'nothing drawn or sent before the menu model arrives');
  await w.evaluate(d=>taskq.receive({dashboard:d,initial:true}),model);
  assert.deepEqual(await w.locator('legend').allInnerTexts(),['Saved views','Projects','States','Display'],'compact sections in one fixed order');
  assert.equal(await w.locator('[data-facet=states][data-value=review] .review').evaluate(e=>getComputedStyle(e).color),'rgb(188, 163, 235)','A4: Review choice uses the Review token');
  if(out){fs.mkdirSync(out,{recursive:true});await w.screenshot({path:out+'/dark-view-window.png'});}
  assert(await w.locator('.content').evaluate(e=>e.scrollHeight<=e.clientHeight+1),'#1730 A2: saved views, eight projects and eight states fit together at 480x600 without scrolling');
  assert.equal(await w.locator('[data-a=view-select]').count(),3,'A2: three saved views');
  assert.deepEqual(await w.locator('[data-facet=states]').evaluateAll(es=>es.map(e=>e.dataset.value)),['doing','review','ask','failed','ready','waiting','later','held'],'MP11 row-major order');
  for(const facet of ['projects','states']){
   const cells=w.locator(facet==='states'?'[data-facet=states],[data-a=all-tasks]':'[data-facet=projects]'),boxes=await cells.evaluateAll(es=>es.map(e=>{const r=e.getBoundingClientRect();const c=e.closest('.content').getBoundingClientRect();return {x:r.x,y:r.y,visible:r.top>=c.top&&r.bottom<=c.bottom};}));
   assert.equal(boxes.length,9);assert.equal(new Set(boxes.slice(0,3).map(r=>r.y)).size,1,'A2: three columns');assert(boxes.every(r=>r.visible),'A2: all filters visible together');
  }
  const reach=await w.evaluate(()=>[...document.querySelectorAll('[data-a=hide],[data-a=view-group],[data-a=view-save],[data-a=view-save-as],[data-a=view-reset],[data-a=popup-close]')].map(e=>{const r=e.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight&&!e.closest('.content');}));
  assert(reach.length===7&&reach.every(Boolean),'Close, Display, Save, Reset and Done reachable without scrolling '+reach);
  assert.equal(await w.locator('.content').evaluate(e=>getComputedStyle(e).overflowY),'auto','one designated content scroll, used only for overflow');
  assert.equal(await w.evaluate(()=>messages.filter(m=>m.a==='views-dashboard').length),0,'opening sends nothing back');
  // #1730 A3: names select exclusively; checkboxes toggle membership, including All and the last selected project.
  const working=()=>w.evaluate(()=>{const d=messages.filter(m=>m.a==='views-dashboard').at(-1).dashboard;return d.views.find(v=>v.id===d.selected).working;});
  const scope=i=>'github://example.org/org/p'+i;
  await w.getByRole('checkbox',{name:'Include Project 0',exact:true}).uncheck();assert.deepEqual((await working()).projects,Array.from({length:7},(_,i)=>scope(i+1)));
  await w.getByRole('button',{name:'Show only Project 1',exact:true}).click();assert.deepEqual((await working()).projects,[scope(1)]);
  await w.getByRole('button',{name:'Show only Project 1',exact:true}).click();assert.deepEqual((await working()).projects,[scope(1)],'clicking the selected name stays exclusive');
  await w.getByRole('checkbox',{name:'Include Project 2',exact:true}).check();assert.deepEqual((await working()).projects,[scope(1),scope(2)]);
  await w.getByRole('button',{name:'Show only Project 0',exact:true}).click();assert.deepEqual((await working()).projects,[scope(0)]);
  await w.getByRole('checkbox',{name:'Include Project 0',exact:true}).click();assert.deepEqual((await working()).projects,[],'removing the last selects All');
  await w.getByRole('button',{name:'Show only Project 0',exact:true}).focus();await w.keyboard.press('Enter');assert.deepEqual((await working()).projects,[scope(0)]);
  await w.getByRole('checkbox',{name:'Include Project 1',exact:true}).focus();await w.keyboard.press('Space');assert.deepEqual((await working()).projects,[scope(0),scope(1)]);assert.equal(await w.evaluate(()=>document.activeElement.getAttribute('aria-label')),'Include Project 1','checkbox retains focus after repaint');
  await w.locator('[data-facet=states][data-value=doing]').click();await w.getByRole('button',{name:'All projects',exact:true}).click();assert.deepEqual((await working()).states,['doing'],'All projects preserves states');
  await w.getByRole('button',{name:'Show only Project 0',exact:true}).click();await w.getByRole('button',{name:'All states',exact:true}).click();assert.deepEqual((await working()).projects,[scope(0)],'All states preserves projects');
  await w.getByRole('button',{name:'All projects',exact:true}).click();
  // A4: naming is disclosed, drafts survive Cancel/Escape; Create, Delete and Undo retain existing view protection.
  assert.equal(await w.locator('[data-view-name]').count(),0);
  await w.getByRole('button',{name:'Save as…',exact:true}).click();await w.getByRole('textbox',{name:'New view name'}).fill('My filters');
  await w.getByRole('button',{name:'Cancel',exact:true}).click();assert.equal(await w.locator('[data-view-name]').count(),0);
  await w.getByRole('button',{name:'Save as…',exact:true}).click();assert.equal(await w.locator('[data-view-name]').inputValue(),'My filters');
  await w.keyboard.press('Escape');assert.equal(await w.locator('[data-view-name]').count(),0);assert.equal(await w.evaluate(()=>messages.filter(m=>m.a==='hide').length),0,'Escape dismisses naming before hiding View');
  await w.getByRole('button',{name:'Save as…',exact:true}).click();await w.getByRole('button',{name:'Create',exact:true}).click();assert.equal(await w.locator('[data-view-name]').count(),0);
  await w.getByRole('button',{name:'Delete view My filters',exact:true}).click();await w.getByRole('button',{name:'Save as…',exact:true}).click();
  if(out)await w.screenshot({path:out+'/compact-view-naming-undo.png'});
  const dense=await w.locator('.content').evaluate(e=>({height:e.clientHeight,scroll:e.scrollHeight}));assert(dense.scroll<=dense.height+1,'A2/A4: three views, naming and Undo still fit '+JSON.stringify(dense));
  await w.getByRole('button',{name:'Cancel',exact:true}).click();await w.getByRole('button',{name:'Undo deleting My filters',exact:true}).click();assert.equal(await w.locator('[data-a=view-select]').count(),4);
  await w.getByRole('button',{name:'Delete view My filters',exact:true}).click();await w.getByRole('button',{name:'All tasks',exact:true}).click();
  assert.equal(await w.getByRole('button',{name:'Delete view All tasks',exact:true}).count(),0);
  // A5: larger text and long names overflow only the content; keyboard can reach every control.
  await w.evaluate(s=>taskq.receive(s),{...wide,font:20,projects:eight.map(p=>({...p,name:(p.name+' long_identifier_').repeat(6)}))});
  const longModel=await w.evaluate(()=>messages.filter(m=>m.a==='views-dashboard').at(-1).dashboard);longModel.selected='attention';longModel.views.find(v=>v.id==='attention').name='Long_unbroken_view_name_'.repeat(3);longModel.undo.view.name='Long_deleted_view_name_'.repeat(3);await w.evaluate(d=>taskq.receive({dashboard:d,initial:true}),longModel);
  assert(await w.getByRole('button',{name:'All projects',exact:true}).evaluate(e=>e.getBoundingClientRect().height<=48),'A5/F1: All projects stays compact beside long names');
  assert(await w.locator('.head h1').evaluate(e=>{const r=e.getBoundingClientRect(),p=e.parentElement.getBoundingClientRect();return e.textContent.includes('Long_unbroken_view_name_')&&r.right<=p.right+1&&e.scrollWidth<=e.clientWidth+1&&getComputedStyle(e).textOverflow==='ellipsis';}),'A5/F2: long View title stays bounded with ellipsis');
  assert(await w.locator('.content').evaluate(e=>e.scrollHeight>e.clientHeight),'overflow uses the content scroll');
  assert.equal(await w.locator('*').evaluateAll(es=>es.filter(e=>e.scrollHeight>e.clientHeight+1&&['auto','scroll'].includes(getComputedStyle(e).overflowY)).length),1,'A5: overflow has exactly one scroll container');
  assert.deepEqual(await w.locator('button,input,.ui-icon,.project-choice,.facet-list').evaluateAll(es=>es.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.getAttribute('aria-label')||e.textContent)),[],'A5: no clipped text or icons at 20 pt');
  assert(await w.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth),'no horizontal overflow');
  if(out)await w.screenshot({path:out+'/long-view-20pt-top.png'});
  await w.getByRole('button',{name:'Done',exact:true}).focus();await w.keyboard.press('Tab');
  const expected=await w.locator('button:not(:disabled),input:not(:disabled)').evaluateAll(es=>es.map(e=>e.getAttribute('aria-label')||e.textContent.trim()));
  const reached=new Set();for(let i=0;i<expected.length+2;i++){const name=await w.evaluate(()=>document.activeElement.getAttribute('aria-label')||document.activeElement.textContent.trim());reached.add(name);await w.keyboard.press('Tab');}
  assert(expected.every(name=>reached.has(name)),'keyboard reaches every named control');
  await w.getByRole('button',{name:'All tasks',exact:true}).click();
  await w.evaluate(s=>taskq.receive(s),wide);
  await w.locator('[data-facet=states][data-value=review]').click();
  const edited=await w.evaluate(()=>messages.filter(m=>m.a==='views-dashboard').at(-1).dashboard);assert.deepEqual(edited.views.find(v=>v.id===edited.selected).working.states,['review'],'A3: a working change applies at once');
  assert.equal(await w.evaluate(()=>messages.some(m=>m.a==='set')),false,'the View window never writes tray.json itself');
  await w.locator('[data-a=view-save]').click();await w.locator('[data-a=view-reset]').click();assert.equal(await w.locator('[data-a=view-delete]').count(),1,'only Attention is deletable; built-in views stay protected');
  await w.locator('[data-a=popup-close]').click();assert(await w.evaluate(()=>messages.some(m=>m.a==='hide')),'Done hides the View window');await w.close();
  await p.evaluate(d=>taskq.receive({dashboard:d}),edited);assert.equal(await list().count(),1,'menu adopts the View window edit');
  await p.waitForFunction(()=>messages.filter(m=>m.a==='set'&&m.navigation?.dashboard?.views?.[0]?.working?.states?.[0]==='review').length>0);await p.close();
  const preview='data:image/png;base64,'+fs.readFileSync('docs/shots/icon-summary-run-dark.png').toString('base64');
  for(const font of [16,20,40]){p=await open({...state,font,icons:{icon:preview,full:'<svg xmlns="http://www.w3.org/2000/svg" width="68" height="22"></svg>'}},{width:320,height:700});await p.locator('[data-route=settings]').click();await p.locator('[data-route=appearance]').click();assert.equal(await p.locator('.views button').count(),2,'#1611 removed Custom icons');assert.deepEqual(await p.locator('.views button').evaluateAll(es=>es.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.textContent)),[],'actual-size previews keep complete labels at '+font+'px');await p.close();}
  p=await open();await p.locator('[data-route=settings]').click();await p.locator('[data-route=appearance]').click();
  const start=await p.locator('.growth-miniature[data-growth=top] i').evaluate(e=>e.getBoundingClientRect().height);
  await p.waitForFunction(start=>Math.abs(document.querySelector('.growth-miniature[data-growth=top] i').getBoundingClientRect().height-start)>2,start,{timeout:2500});
  assert.equal(await p.locator('.growth-miniature[data-growth=fixed] i').evaluate(e=>getComputedStyle(e).animationName),'none');
  await p.emulateMedia({reducedMotion:'reduce'});assert.equal(await p.locator('.growth-miniature i').first().evaluate(e=>getComputedStyle(e).animationName),'none');await p.close();
  if(out)fs.writeFileSync(out+'/browser.json',JSON.stringify({browser:browser.version(),observations},null,2)+'\n');
  console.log('PASS #1730 compact grids, exclusive names, checkbox scope, naming drafts, large text and keyboard; #1578 O1/O2/O3 views, scoped counts, same comparator, Save/Reset/restart, accessible anchored details/actions, window controls and both themes 16/20 narrow/short/long/partial');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
