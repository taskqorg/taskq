// #1578 A2–A8 / O1–O3: accepted dashboard behavior and geometry; invented data, no live board mutations.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.DASHBOARD_EVIDENCE;
const task=(n,state,extra={})=>({n,state,title:'Keep the complete task title readable across narrow windows and large type '+n,revision:n+50,since:900,created:500,steps:[{state:'ready',start:500,end:900,complete:true},{state,start:900,end:null,complete:false}],controls:state==='failed'?['retry']:state==='held'?['start']:['stop','later'],...extra});
const alpha={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],tasks:[task(3,'ready'),task(5,'doing'),task(2,'failed'),task(4,'held')],delivery:{preset:'auto-merge',revision:0}};
const beta={...alpha,root:'/fixture/beta',name:'Beta',issues:'https://example.org/org/beta/issues',tasks:[task(1,'review'),task(3,'ready')]};
const state={platform:'macos',font:16,theme:'dark',host:'fixture',now:1000,version:'fixture',size_anchor:'auto',attached_edge:'top',top:true,service:{state:'running'},projects:[beta,alpha,{...alpha,root:'/fixture/duplicate'}],ai:{draft:''},notify:{projects:{}},stop_modes:true,messages:{complete:true,count:1}};
const html=fs.readFileSync('build/web/tray.html','utf8');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const open=async(s=state,viewport={width:360,height:760})=>{const p=await browser.newPage({viewport});p.on('pageerror',e=>{throw e;});await p.setContent(html);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await p.evaluate(s=>taskq.receive(s),s);return p;};
  let p=await open();const list=()=>p.locator('[data-card]'),editor=()=>p.locator('#taskq-popover');
  assert.equal(await p.locator('[data-a=view-editor]').count(),1,'before: approved overlay editor missing; after: reachable');
  assert.equal(await list().count(),6,'canonical duplicate registrations count once');
  assert.equal(await p.getByRole('button',{name:'Inbox · 1',exact:true}).count(),1);assert.equal(await p.getByRole('button',{name:'Conversations',exact:true}).count(),1);
  await p.locator('[data-a=view-editor]').click();assert(await editor().evaluate(e=>e.matches(':popover-open')));
  const before=await list().first().boundingBox();
  await editor().locator('[data-facet=projects][data-value="github://example.org/org/alpha"]').click();
  assert.equal(await list().count(),4);assert.match(await p.locator('[data-a=all-tasks]').first().innerText(),/4/);
  await editor().locator('[data-facet=states][data-value=doing]').click();await editor().locator('[data-facet=states][data-value=failed]').click();
  assert.equal(await list().count(),2);assert.match(await p.locator('[data-a=all-tasks]').first().innerText(),/4/,'All counts selected projects before state filtering');
  const after=await list().first().boundingBox();assert.equal(after.x,before.x,'overlay never displaces list');
  await editor().locator('[data-a=view-save]').click();await editor().locator('[data-a=popup-close]').first().click();
  await p.locator('[data-task-query]').fill('complete');assert.equal(await list().count(),2);
  await p.locator('[data-a=view-list]').click();await editor().locator('[data-view-name]').fill('Attention');await editor().locator('[data-a=view-add]').click();
  await p.locator('[data-task-query]').fill('5');assert.equal(await list().count(),1);
  await p.locator('[data-a=view-select][data-id=all]').click();assert.equal(await p.locator('[data-task-query]').inputValue(),'complete','switch restores unsaved working query');
  await p.locator('[data-a=view-select]').filter({hasText:'Attention'}).click();assert.equal(await p.locator('[data-task-query]').inputValue(),'5');
  await p.waitForFunction(()=>messages.some(m=>m.a==='set'&&m.navigation?.dashboard?.views?.length===2));
  const navigation=await p.evaluate(()=>messages.filter(m=>m.a==='set'&&m.navigation?.dashboard?.views?.length===2).at(-1).navigation);
  await p.close();p=await open({...state,navigation});assert.equal(await p.locator('[data-task-query]').inputValue(),'5','restart retains each view working context');
  await p.locator('[data-a=view-editor]').click();await editor().locator('[data-a=view-reset]').click();assert.equal(await p.locator('[data-task-query]').inputValue(),'complete','Reset restores explicitly saved criteria');
  await editor().locator('[data-a=popup-close]').first().click();
  // All clears only the state filter. Project/search are independent and remain visible.
  await p.locator('[data-a=all-tasks]').click();assert.equal(await list().count(),4);
  await p.locator('[data-task-query]').fill('');await p.locator('[data-a=view-editor]').click();await editor().locator('[data-facet=projects][data-value=""]').click();await editor().locator('[data-a=view-group][data-value=combined]').click();await editor().locator('[data-a=popup-close]').first().click();
  const identities=await list().locator('[data-a=task-detail]').evaluateAll(es=>es.map(e=>e.textContent.trim().replace(/^GitHub /,'')));
  assert.deepEqual(identities,['Alpha #5','Beta #1','Alpha #2','Alpha #3','Beta #3','Alpha #4'],'O2 one comparator with canonical repository tie');
  assert.equal(await p.locator('[data-a=row-menu],.row-menu').count(),0,'no right ellipsis or action column');
  const identity=p.locator('[data-a=task-detail]').filter({hasText:'Alpha #5'});await identity.focus();assert.equal(await identity.getAttribute('aria-expanded'),'true');
  await p.keyboard.press('Enter');assert.equal(await editor().getByRole('button',{name:'Keep task details open'}).getAttribute('aria-pressed'),'true');
  assert.match(await editor().innerText(),/Total age:[\s\S]*1 completed/);assert(!await list().first().innerText().then(t=>/Total age|completed|runtime|host/.test(t)));
  await editor().locator('summary').click();assert.match(await editor().innerText(),/ready[\s\S]*doing[\s\S]*current/);
  assert.equal(await editor().locator('[data-a=control]').count(),2,'all offered revision-fenced controls retained');
  await p.keyboard.press('Escape');assert.equal(await editor().count(),0);assert(await identity.evaluate(e=>e===document.activeElement),'Escape restores identity focus');
  await p.locator('[data-route=settings]').click();await p.locator('[data-route=window]').click();
  for(const mode of ['auto','top','bottom','fixed']){await p.locator('[data-a=size-anchor][data-value='+mode+']').click();assert(await p.evaluate(mode=>messages.some(m=>m.a==='size-anchor'&&m.value===mode),mode));}
  await p.locator('[data-resize]').focus();await p.keyboard.press('ArrowRight');assert(await p.evaluate(()=>messages.some(m=>m.a==='resize')));
  await p.getByRole('button',{name:'Close window',exact:true}).click();assert(await p.evaluate(()=>messages.some(m=>m.a==='hide')));
  assert.equal(await p.evaluate(()=>messages.some(m=>['focus','pause','resume','capacity','ai-send'].includes(m.a))),false,'view/window controls have no admission/grant/send effects');
  await p.close();
  const observations=[];
  for(const theme of ['dark','light'])for(const font of [16,20])for(const variant of ['long','short']){
   p=await open({...state,font,theme,projects:variant==='long'?[{...alpha,tasks:Array.from({length:16},(_,i)=>task(i+1,i%2?'doing':'failed',{title:('Long unbroken identifier_'+i+'_').repeat(5),since:i%2?null:900}))}]:[{...alpha,complete:false,tasks:[]} ]},{width:320,height:variant==='short'?300:700});
   const geometry=await p.evaluate(()=>{const c=document.querySelector('.content'),head=document.querySelector('.head').getBoundingClientRect(),footer=document.querySelector('.status-footer').getBoundingClientRect();return {overflow:document.documentElement.scrollWidth-innerWidth,contentOverflow:c.scrollWidth-c.clientWidth,head:head.bottom,content:c.getBoundingClientRect().top,bottom:c.getBoundingClientRect().bottom,footer:footer.top,last:footer.bottom,viewport:innerHeight};});
   assert(geometry.overflow<=0&&geometry.contentOverflow<=0,'no horizontal overflow '+JSON.stringify(geometry));assert(geometry.head<=geometry.content+.5&&geometry.bottom<=geometry.footer+.5&&geometry.last<=geometry.viewport+.5,'fixed chrome never overlaps content');
   await p.locator('[data-a=view-editor]').click();const popup=await editor().boundingBox();assert(popup.x>=0&&popup.y>=0&&popup.x+popup.width<=320.5&&popup.y+popup.height<=geometry.viewport+.5,'bounded anchored editor');
   if(out){fs.mkdirSync(out,{recursive:true});await p.screenshot({path:out+'/'+theme+'-'+font+'-'+variant+'-editor.png'});}
   await p.keyboard.press('Escape');if(out)await p.screenshot({path:out+'/'+theme+'-'+font+'-'+variant+'.png'});
   if(variant==='long'){const t=p.locator('[data-a=task-detail]').first();await t.click();assert.match(await editor().innerText(),/elapsed unknown/);await p.keyboard.press('Escape');}
   else {assert.equal((await p.locator('.filter-header').innerText()).includes('?'),true,'partial never zero');}
   observations.push({theme,font,variant,...geometry});await p.close();
  }
  p=await open();await p.emulateMedia({reducedMotion:'reduce'});await p.locator('[data-route=settings]').click();await p.locator('[data-route=window]').click();assert.equal(await p.locator('.growth-miniature i').first().evaluate(e=>getComputedStyle(e).animationName),'none');await p.close();
  if(out)fs.writeFileSync(out+'/browser.json',JSON.stringify({browser:browser.version(),observations},null,2)+'\n');
  console.log('PASS #1578 O1/O2/O3 views, scoped counts, same comparator, Save/Reset/restart, accessible anchored details/actions, window controls and both themes 16/20 narrow/short/long/partial');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
