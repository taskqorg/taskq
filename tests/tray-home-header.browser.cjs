// #1967 A3/A5, M3.1/M1.2: scrolling must retain opaque host/filter controls above task rows.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const evidence=process.env.HOME_HEADER_EVIDENCE;
const project={root:'/fixture/taskq',name:'TaskQ',host:'mac',issues:'https://example.org/taskq/issues',board_kind:'github',alive:true,complete:true,age:1,tasks_age_seconds:0,tasks_live:true,observed:1000,focus:['all'],labels:[],
 hosts:[{name:'mac',live:true,age:1,age_text:'1s'},{name:'win',live:true,age:1,age_text:'1s'},{name:'stale',live:false,age:68400,age_text:'19h'}],
 tasks:Array.from({length:40},(_,i)=>({n:i+1,state:'ready',title:'Invented task '+(i+1)+' keeps Home scrolling',revision:1,executions:[]}))};
const snapshot={display:require('./fixtures/display-model.json').display,platform:'windows',font:16,theme:'dark',host:'mac',now:1000,version:'fixture',projects:[project],notify:{projects:{}},opacity_mode:'background',background_opacity:100};
const frames=page=>page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 for(const width of [320,420])for(const font of [20,14])for(const theme of ['dark','light'])for(const alpha of [100,0]){
  const page=await browser.newPage({viewport:{width,height:600}});page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.ipc={postMessage:()=>{}};});
  await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme,background_opacity:alpha});
  await page.evaluate(()=>document.fonts.ready);await frames(page);
  const host=page.locator('.dashboard-status>summary'),filter=page.locator('.filter-header'),content=page.locator('.content');
  const stripY=(await content.boundingBox()).y;
  await content.evaluate(e=>e.scrollTop=430);await frames(page);
  if(evidence&&width===320&&font===20&&theme==='dark'&&alpha===100){fs.mkdirSync(evidence,{recursive:true});await page.screenshot({path:evidence+'/home-scrolled.png'});}
  assert(Math.abs((await host.boundingBox()).y-stripY)<=1,'A3: Home host strip stays at the top after scrolling');
  for(const y of [700,1000]){
   await content.evaluate((e,y)=>e.scrollTop=y,y);await frames(page);
   const h=await host.boundingBox(),f=await filter.boundingBox();
   assert(Math.abs(h.y-stripY)<=1,'A3: host strip remains pinned');
   assert(Math.abs(f.y-h.y-h.height)<=1,'A3: filters pin directly below the wrapping host strip');
   for(const target of [host,filter])assert(await target.evaluate(e=>{
    const r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);
    return e.contains(hit)&&!e.contains(document.querySelector('.task-row'))&&getComputedStyle(e).backgroundColor!=='rgba(0, 0, 0, 0)';
   }),'A3: task rows cannot paint over host/filter controls');
  }
  // Disclosure content still belongs to the one menu scroll, even while its summary is pinned.
  await host.click();await frames(page);await content.evaluate(e=>e.scrollTop=e.scrollHeight);await frames(page);
  assert(await page.locator('.task-row').last().evaluate(e=>e.getBoundingClientRect().bottom<=document.querySelector('.content').getBoundingClientRect().bottom+1),'last task remains reachable with host details expanded');
  assert.deepEqual(await page.locator('.dashboard-status .running-agents').evaluateAll(es=>es.filter(e=>e.scrollHeight>e.clientHeight+1&&['auto','scroll'].includes(getComputedStyle(e).overflowY)).map(e=>e.className)),[],'no nested host-details scroll');
  for(const retired of ['stale','mac']){
   await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme,host:retired,background_opacity:alpha,projects:[{...project,hosts:project.hosts.filter(h=>h.name!==retired)}]});
   assert.equal(await page.locator('.host-chip[data-host='+retired+']').count(),0,'A4: a retired heartbeat leaves the strip, including on its own tray');
   assert.equal(await page.locator('.host-chip[data-host=win]').count(),1,'A4: open host identities remain');
  }
  await page.close();
 }
 console.log('PASS #1967 A3: pinned Home host/filter rows, stacking, wrapping, expanded details, both themes/background opacity');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
