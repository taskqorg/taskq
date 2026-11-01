// #1661 A1–A6 (R16–R20, R28–R35): compact Settings widgets, host lines, sign-in, Appearance previews and one host
// service line, in light and dark at 320 px. Native data (Mac, Windows, FRA) and OS permission are not measured here.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.SETTINGS_EVIDENCE;
const fx=require('./fixtures/display-model.json');
const execution=(id,host,runtime,role)=>({id,attempt:id,host,runtime,role,state:'running',observed:990,live:true});
const access=[{runtime:'claude',requested:'workspace-write',grant:'workspace-write',effective:'workspace-write',enforcement:'sandbox'},{runtime:'codex',requested:'full',grant:null,effective:null,refused:'host grant missing'}];
const project=(root,name,host,more={})=>({root,name,host,mode:'run',version:'4.1.0',age:2,age_text:'<1m',alive:true,complete:true,tasks_live:true,tasks_age_seconds:0,observed:1000,focus:['all'],labels:[],delivery:{preset:'auto-merge',revision:3},
 hosts:[{name:'mac',updated:990,live:true},{name:'win',updated:990,live:true},{name:'fra',updated:690,live:false,age:310,age_text:'5m'}],tasks:[],...more});
const alpha=project('/fixture/alpha','Infrastructure provisioning and observability','mac',{access,sample_age_text:'2m',
 capacity:{cpu_percent:42,memory_available_percent:62,host_runtime_used:{claude:1,codex:1},reservations:[]},
 tasks:[{n:1,state:'doing',key:'github:a/1',executions:[execution(11,'mac','claude','worker')]},{n:2,state:'review',key:'github:a/2',executions:[execution(12,'mac','codex','reviewer')]},{n:3,state:'doing',key:'github:a/3',executions:[execution(13,'win','codex','worker')]}]});
const state={platform:'windows',font:16,host:'mac',now:1000,version:'4.1.0',size_anchor:'auto',service:{state:'running',pid:42,fresh:true},stop_modes:true,display:fx.display,ai:{draft:''},
 notify:{status:'allowed',host:{ask:'on',close:'off',failed:'on',gone:'off'},projects:{}},period:'1d',
 agents:{at:700,age:'5m',fresh:true,rows:[{runtime:'claude',state:'signed-in',version:'2.1.3',account:'owner@example.org',method:'Claude subscription',expires:null,refreshed:null,reason:null,next:null},
  {runtime:'codex',state:'expired',version:'0.48.0',account:null,method:'ChatGPT',expires:900,refreshed:null,reason:null,next:'run `codex login` with the PM'}]},
 projects:[alpha,project('/fixture/beta','Beta','mac'),project('/fixture/gamma','Gamma','win',{version:'4.0.9'})]};
// the icon pair the native menu sends; the previews show them at actual size
state.icons={icon:'data:image/png;base64,'+fs.readFileSync('docs/shots/icon-summary-run-dark.png').toString('base64'),full:'<svg xmlns="http://www.w3.org/2000/svg" width="68" height="22"><rect width="68" height="22" rx="4" fill="#888"/></svg>'};
const html=fs.readFileSync('build/web/tray.html','utf8');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  if(out)fs.mkdirSync(out,{recursive:true});
  for(const theme of ['light','dark']){
   const p=await browser.newPage({viewport:{width:320,height:900},reducedMotion:theme==='dark'?'reduce':'no-preference'}),errors=[];p.on('pageerror',e=>errors.push(e.message));
   await p.setContent(html);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   const go=async page=>{await p.evaluate(s=>taskq.receive(s),{...state,theme,page});};
   const text=()=>p.locator('.content').evaluate(e=>e.textContent);
   const shot=async()=>{};
   // R28: one compact widget per route; a label and its value stay whole at 320 px
   await go('settings');
   const layout=await p.locator('.settings-widget').evaluateAll(es=>es.map(e=>{const a=e.children[1].getBoundingClientRect(),b=e.querySelector('.meta').getBoundingClientRect();return {label:e.children[1].textContent,display:getComputedStyle(e).display,overlap:Math.min(a.right,b.right)-Math.max(a.left,b.left)>1&&Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)>1};}));
   assert.deepEqual(layout.filter(x=>x.display!=='grid'||x.overlap),[],'F05: Settings owns its grid; label and summary never overlap');
   const widgets=await p.locator('.settings-widget').evaluateAll(es=>es.map(e=>({label:e.children[1].textContent,meta:e.querySelector('.meta').textContent,cut:[...e.children].some(c=>c.scrollWidth>c.clientWidth+1)})));
   assert.deepEqual(widgets.map(w=>w.label),['Appearance','Notifications','Activity history','Hosts and resources','Work mode','Agents and access','Projects','Delivery','Services and support','Emergency stop','Recovery','About and updates']);
   // #1775 R43: route marks follow the visual-style control icon table, not near-miss substitutes
   assert.deepEqual(await p.locator('.settings-widget').evaluateAll(es=>es.map(e=>e.querySelector('.ui-icon svg')?.getAttribute('class').replace('lucide lucide-',''))),['type','bell','chart-no-axes-column','network','focus','bot','folder','git-pull-request','activity','octagon-x','rotate-ccw','info']);
   assert.deepEqual(widgets.filter(w=>w.cut).map(w=>w.label),[],'no clipped widget text');
   assert.equal(widgets.find(w=>w.label==='Hosts and resources').meta,'3 hosts · observations incomplete','a global count is never one host; #1729 zero counts hide');
   assert.equal(widgets.find(w=>w.label==='Agents and access').meta,'Claude signed in · Codex expired');
   assert.equal(widgets.find(w=>w.label==='Delivery').meta,'3 projects · 1 policy','equal values group');
   assert.equal(widgets.find(w=>w.label==='Work mode').meta,'3 projects · 3 unknown · capacity unavailable','R32: admission first, capacity apart');
   await shot('settings');
   // R16/R17: one line per host, Claude and Codex apart; stale never 0; this host's expansion holds units and age
   await go('hosts');
   const lines=await p.locator('.host-line > summary').evaluateAll(es=>es.map(e=>[e.querySelector('.host-name').textContent,...e.querySelectorAll('.host-number')].map(x=>typeof x==='string'?x:x.textContent)));
   assert.deepEqual(lines,[['fra','?','?','?'],['mac','1','1','2'],['win','1','0','1']],'UX30-06: comparable per-host runtime/total counts, stale never zero');
   assert.match(await p.locator('.host-line > summary',{hasText:'mac'}).innerText(),/Sign-in: Codex expired/,'A5: sign-in warning stays separate and visible in the first host expansion');
   await p.locator('.host-line > summary',{hasText:'mac'}).click();
   assert.equal(await p.locator('.resource-meter meter').count(),2);
   assert.match(await p.locator('.resource-meters').innerText(),/CPU busy.*42%[\s\S]*Memory available.*62%/s);
   await shot('hosts');
   // R18/R19/R29: account, not method; expiry; the sign-in route; access effective per project; no placeholder
   await go('agents');
   const t=await text();
   for(const s of ['Claude 2.1.3 · Signed in · owner@example.org','Codex 0.48.0 · Expired · account unknown','Codex · unknown: win heartbeat publishes no client version','Codex · unknown: fra heartbeat publishes no client version','Access unknown for Beta','taskq service permissions'])assert(t.includes(s),s);
   assert.equal(await p.locator('.content [data-route=agents]').count(),0,'no circular placeholder');
   await p.locator('.agent-signin > summary').nth(1).click();await p.locator('details.access > summary').first().click();
   await shot('agents');
   // R20: one Appearance route with tray icon, text size and window previews; reduced motion keeps a still preview
   await go('appearance');
   assert.equal(await p.locator('.content .views button').count(),2);assert.equal(await p.locator('.content .growth-options button').count(),4);
   assert.equal(await p.locator('.content [data-set=font][data-v="20"]').evaluate(e=>getComputedStyle(e).fontSize),'20px','text size previews itself');
   assert.equal((await text()).includes('Reduced motion: growth previews show a still outline and arrow.'),theme==='dark');
   await shot('appearance');
   // R30: the host service once, then each project's status age
   await go('services');
   assert.equal((await text()).split('Host service: running · PID 42').length-1,1,'one host service line');
   await shot('services');
   assert.deepEqual(errors,[]);await p.close();
   // #1713 A1–A4 / TW1–TW2: Settings keeps its inner routes and Back focus; only an explicit review opens operations.
   for(const resources of [null,require('./fixtures/resource-controls.json').cases.find(c=>c.name.startsWith('host commands')).groups[0]]){
    const s=await browser.newPage({viewport:{width:320,height:900}});
    s.on('pageerror',e=>errors.push(e.message));
    await s.setContent(html.replace('<head>',"<head><script>window.taskqSurface='settings';</script>"));
    await s.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
    const current={...state,theme,projects:[{...alpha,resources}]};
    await s.evaluate(snapshot=>taskq.receive(snapshot),current);
    for(const [label,route,operation] of [['Work mode','work',resources?'resources':'admission'],['Recovery','recovery','recovery'],['Agents and access','agents'],['Services and support','services'],['About and updates','about']]){
     await s.evaluate(()=>{messages.length=0;});
     await s.locator('.settings-widget').getByText(label,{exact:true}).click();
     assert.equal(await s.locator('.head h1').innerText(),label,'#1713: inner title equals list label');
     assert.equal(await s.locator('.breadcrumbs').innerText(),'Settings');
     assert.equal(await s.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'Settings inner route fits at 320 px');
     assert.equal(await s.locator('[aria-label="Close settings"]').count(),1,'Settings stays open');
     assert.deepEqual(await s.evaluate(()=>messages.filter(m=>!['web-observation','size','set'].includes(m.a))),[],'#1713 A4: opening sends no operation or workspace handoff');
     await s.evaluate(snapshot=>taskq.receive(snapshot),current);
     assert.equal(await s.locator('.head h1').innerText(),label,'refresh retains the inner route');

     if(operation){
      await s.locator('[data-a=workspace][data-operation='+operation+']').click();
      assert.deepEqual(await s.evaluate(()=>messages.filter(m=>m.a==='workspace').map(m=>m.open)),[{page:'operations',operation,root:null,board:null,task:null,revision:null}],'explicit review keeps the existing operation destination');
     }
     await s.locator('.head [data-a=back]').click();
     assert.equal(await s.locator('.head h1').innerText(),'Settings');
     assert.equal(await s.evaluate(()=>document.activeElement.dataset.route),route,'Back restores the invoking Settings link');
    }
    assert.deepEqual(errors,[]);await s.close();
   }
  }
  // #2096 A2–A8 / F05,F06,F10: all twelve destinations share the same dense, long-name snapshot.
  const routes=['settings','appearance','notification-settings','cadence','hosts','work','agents','projects','delivery','services','stop','recovery','about'];
  const resources=require('./fixtures/resource-controls.json').cases.find(c=>c.name.startsWith('host commands')).groups[0];
  const ps=Array.from({length:11},(_,i)=>project('/fixture/project-'+i,i===0?'Infrastructure provisioning and observability':'Project '+(i+1),'mac',{
   issues:'https://github.com/invented/project-'+i+'/issues',board_kind:'github',access,resources,
   log:'/fixture/project-'+i+'/service.log',request:i?'':'Keep infrastructure observed',
   capacity:{gate:'open',reasons:i===1?[{code:'access',text:'Codex: host grant missing'}]:[]},
   ...(i===2?{alive:false,age:600,age_text:'10m',error:'board observation unavailable'}:{}),
   delivery:{preset:['auto-merge','review-pr','direct'][i%3],revision:0}
  }));
  const dense={...state,projects:ps,history:require('./fixtures/icon-history/day-light.json'),opacity:83,opacity_mode:'background',background_opacity:37,
   stop_hold:{mode:'drain',selected_count:0},full_stop_review:{reason:'Native execution identity unknown'},
   notify:{status:'blocked',host:{ask:'sound',close:'off',failed:'banner',gone:'silent'},projects:Object.fromEntries(ps.map(p=>[p.root,{ask:[null,'sound'],close:['banner','banner'],failed:[null,'banner'],gone:[null,'silent']}]))}};
  const cells=[];
  const geometry=async p=>p.evaluate(()=>{
   const content=document.querySelector('.content'),root=content.getBoundingClientRect();
   const clipped=[...content.querySelectorAll('button,a,select,input,summary,.settings-widget>span')].filter(e=>e.checkVisibility()).flatMap(e=>{
    const r=e.getBoundingClientRect();return r.width&&(r.left<root.left-1||r.right>root.right+1||e.scrollWidth>e.clientWidth+1)?[{tag:e.tagName,text:e.textContent.slice(0,65),left:r.left,right:r.right,scroll:e.scrollWidth-e.clientWidth}]:[];
   });
   const overlap=[...content.querySelectorAll('.settings-widget')].flatMap(e=>{const a=e.children[1].getBoundingClientRect(),b=e.querySelector('.meta').getBoundingClientRect();return getComputedStyle(e).display!=='grid'||b.top<a.bottom-.5?[e.textContent]:[];});
   return {scroll:document.documentElement.scrollWidth>innerWidth||content.scrollWidth>content.clientWidth+1,clipped,overlap};
  });
  for(const theme of ['light','dark'])for(const font of [18,20])for(const width of [320,560]){
   const p=await browser.newPage({viewport:{width,height:960}}),errors=[];p.on('pageerror',e=>errors.push(e.message));
   await p.setContent(html.replace('<head>',"<head><script>window.taskqSurface='settings';</script>"));
   await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   const go=async(page,more={})=>p.evaluate(s=>taskq.receive(s),{...dense,theme,font,page,...more});
   const check=async(route,phase)=>{assert.deepEqual(await geometry(p),{scroll:false,clipped:[],overlap:[]},`${route} ${phase} ${width}/${font}/${theme}`);};
   for(const route of routes){
    await go(route);await check(route,'initial');
    // Closed disclosures cannot hide bad geometry or remove advanced controls from the check.
    await p.locator('.content details').evaluateAll(es=>es.forEach(e=>e.open=true));await check(route,'expanded');
    if(route==='settings'){
     assert.equal(await p.locator('.settings-widget').count(),12);
     assert.match(await p.locator('[data-route=work] .meta').innerText(),/^11 projects · 1 blocked · 1 unknown/);
     assert.equal(await p.locator('.settings-widget .meta').evaluateAll(es=>es.some(e=>e.textContent.includes('Project 11'))),false,'root never enumerates projects');
    }
    if(route==='appearance'){
     assert.equal(await p.locator('#menu-opacity').count(),0);assert.equal(await p.locator('#background-opacity').inputValue(),'37');
     await p.locator('[data-set=opacity_mode][data-v=whole]').click();assert.equal(await p.locator('#background-opacity').count(),0);assert.equal(await p.locator('#menu-opacity').inputValue(),'83');
     await p.locator('[data-set=opacity_mode][data-v=background]').click();assert.equal(await p.locator('#background-opacity').inputValue(),'37','switching retains both saved values');
     await go(route,{opacity_mode:'whole',opacity:83,background_opacity:37,opacity_error:'Save failed; previous mode restored'});
     assert.match(await p.locator('[role=alert]').innerText(),/Save failed; previous mode restored/);assert.equal(await p.locator('#menu-opacity').inputValue(),'83','native rollback snapshot wins');
     await go(route);
    }
    if(route==='notification-settings'){
     assert.equal(await p.locator('.notification-legend').count(),1);assert.equal(await p.locator('[data-notify]').count(),48,'four host rows plus all project overrides');
     assert.equal(await p.locator('[data-a=notify-test]:disabled').count(),4,'blocked OS never sends tests');
     await p.locator('[data-notify=ask]:not([data-root])').selectOption('silent');
     await p.locator('[data-notify=close][data-root="/fixture/project-0"]').selectOption('inherit');
     assert.deepEqual(await p.evaluate(()=>messages.filter(m=>m.a==='notify').map(({context,...m})=>m)),[{a:'notify',kind:'ask',value:'silent'},{a:'notify',kind:'close',value:'inherit',root:'/fixture/project-0'}]);
     await go(route);await p.locator('.content details').evaluateAll(es=>es.forEach(e=>e.open=false));
    }
    if(route==='agents'){
     assert.equal((await p.locator('.content').innerText()).split('Claude 2.1.3').length-1,1,'client version once');
     assert.equal(await p.locator('.access-grid').count(),11,'all project/runtime readbacks remain comparable');
     assert.match(await p.locator('.content').innerText(),/host grant missing/);
    }
    if(route==='projects'){
     assert.equal(await p.locator('.settings-project .project-overview').count(),11);
     assert.equal(await p.locator('.settings-project [data-a=folder]').count(),11);
     assert.equal(await p.locator('.settings-project a[data-url]').count(),11);
     assert.match(await p.locator('.content').innerText(),/Keep infrastructure observed/);
    }
    if(route==='delivery')for(const text of ['publish and merge automatically','owner must approve its exact candidate','fast-forward MR on GitLab','Revision 0'])assert((await p.locator('.content').innerText()).includes(text),text);
    if(route==='services'){
     assert.equal((await p.locator('.content').innerText()).split('Host service: running · PID 42').length-1,1);
     assert.match(await p.locator('.content').innerText(),/board observation unavailable|10m/);
     assert.equal(await p.locator('[data-a=log]').count()>=11,true,'logs remain available');
    }
    if(route==='work')assert.match(await p.locator('.content').innerText(),/Admission on mac[\s\S]*11 projects[\s\S]*Review project admission[\s\S]*Review host limits/);
    if(route==='recovery')assert.match(await p.locator('.content').innerText(),/Admission held · drain[\s\S]*Unknown effects|Admission held · drain[\s\S]*unknown effects/);
    if(out&&theme==='dark'&&font===18&&width===320&&['settings','appearance','work','recovery','notification-settings','agents','services'].includes(route)){
     await p.locator('.content details').evaluateAll(es=>es.forEach(e=>e.open=false));
     await p.locator('.content').evaluate(e=>e.scrollTop=0);
     await p.screenshot({path:out+'/after-'+route+'.png'});
    }
    cells.push({route,width,font,theme,result:'Pass'});
    await p.evaluate(()=>messages.length=0);
   }
   // Entry -> native operation-review handoff -> Settings Back preserves context without applying anything.
   for(const route of ['work','recovery']){
    await go('settings');await p.locator('[data-route='+route+']').click();
    assert.equal(await p.locator('.head h1').innerText(),route==='work'?'Work mode':'Recovery');
    await p.evaluate(()=>messages.length=0);
    await p.locator('[data-a=workspace]').first().click();
    assert.deepEqual(await p.evaluate(()=>messages.filter(m=>!['web-observation','size','set'].includes(m.a)).map(m=>[m.a,m.open?.operation])),[['workspace',route==='work'?'admission':'recovery']]);
    await p.locator('.head [data-a=back]').click();assert.equal(await p.locator('.head h1').innerText(),'Settings');
    assert.equal(await p.evaluate(()=>document.activeElement.dataset.route),route);
   }
   assert.deepEqual(errors,[]);await p.close();
  }
  if(out)fs.writeFileSync(out+'/matrix.json',JSON.stringify(cells,null,2)+'\n');
  console.log('PASS #2096: 12 destinations + Settings root × 320/560 px × 18/20 px × both themes; collapsed/expanded geometry, opacity switching/rollback readback, notification IPC/overrides, complete access and operation review/Back; '+cells.length+' cells');
  console.log('PASS #1661 Settings content; #1713 inner routes, matching titles, Back focus and no operation on entry, light/dark at 320 px with/without capacity');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
