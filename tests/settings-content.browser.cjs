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
   const shot=async name=>{if(out)await p.screenshot({path:out+'/'+name+'-320-'+theme+'.png',fullPage:true});};
   // R28: one compact widget per route; a label and its value stay whole at 320 px
   await go('settings');
   const widgets=await p.locator('.settings-widget').evaluateAll(es=>es.map(e=>({label:e.children[1].textContent,meta:e.querySelector('.meta').textContent,cut:[...e.children].some(c=>c.scrollWidth>c.clientWidth+1)})));
   assert.deepEqual(widgets.map(w=>w.label),['Appearance','Notifications','Activity history','Hosts and resources','Work mode','Agents and access','Projects','Delivery','Services and support','Emergency stop','Recovery','About and updates']);
   // #1775 R43: route marks follow the visual-style control icon table, not near-miss substitutes
   assert.deepEqual(await p.locator('.settings-widget').evaluateAll(es=>es.map(e=>e.querySelector('.ui-icon svg')?.getAttribute('class').replace('lucide lucide-',''))),['type','bell','chart-no-axes-column','network','focus','bot','folder','git-pull-request','activity','octagon-x','rotate-ccw','info']);
   assert.deepEqual(widgets.filter(w=>w.cut).map(w=>w.label),[],'no clipped widget text');
   assert.equal(widgets.find(w=>w.label==='Hosts and resources').meta,'fra · runs unknown · 5m old; mac · Claude 1 · Codex 1 · Codex expired; win · Codex 1','a global count is never one host; #1729 zero counts hide');
   assert.equal(widgets.find(w=>w.label==='Agents and access').meta,'Claude signed in · Codex expired');
   assert.equal(widgets.find(w=>w.label==='Delivery').meta,'auto-merge (3)','equal values group');
   assert.equal(widgets.find(w=>w.label==='Work mode').meta,'Unknown: all 3 projects · Intake: 3 configured · capacity unavailable','R32: admission first, capacity apart');
   await shot('settings');
   // R16/R17: one line per host, Claude and Codex apart; stale never 0; this host's expansion holds units and age
   await go('hosts');
   const lines=await p.locator('.host-line > summary').evaluateAll(es=>es.map(e=>e.innerText));
   assert.deepEqual(lines,['fra · runs unknown · 5m old','mac · Claude 1 · Codex 1 · Codex expired','win · Claude 0 · Codex 1']);
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
     if(out&&resources&&theme==='dark'&&operation)await s.screenshot({path:out+'/settings-'+route+'-320-dark.png',fullPage:true});
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
  console.log('PASS #1661 Settings content; #1713 inner routes, matching titles, Back focus and no operation on entry, light/dark at 320 px with/without capacity');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
