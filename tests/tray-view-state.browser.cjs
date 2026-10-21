// M8/UI02: actual bundle -> shared state -> native settings IPC -> a fresh page.
// The bridge is substituted; Rust settings tests cover the real tray.json adapter.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const project={root:'fixture-alpha',name:'Alpha',issues:'https://example.org/alpha/issues',alive:true,complete:true,focus:['all'],labels:[],cap_saved:{error:'Fixture'},tasks:Array.from({length:35},(_,i)=>({n:i+1,state:i%2?'ready':'doing',title:'Persist this filtered task list across restart '+(i+1),revision:1}))};
const snapshot={font:16,theme:'dark',version:'old',projects:[project],ai:{draft:''},notify:{projects:{}}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const errors=[],html=fs.readFileSync('build/web/tray.html','utf8');let stored=null;
  const open=async state=>{
   const page=await browser.newPage({viewport:{width:360,height:600}});page.on('pageerror',e=>errors.push(e.message));
   await page.exposeFunction('saveView',m=>{if(m.a==='set'&&m.navigation)stored=m.navigation;});
   await page.setContent(html);
   await page.evaluate(()=>{window.ipc={postMessage:s=>window.saveView(JSON.parse(s))};});
   await page.evaluate(s=>taskq.receive(s),state);return page;
  };
  let page=await open(snapshot);
  await page.locator('[data-tqp="project"]').first().click();
  await page.locator('.service > summary').click();
  await page.locator('.tqp-counts [data-state="doing"]').first().click();
  await page.locator('.content').evaluate(e=>{e.scrollTop=350;});
  await page.waitForTimeout(250); // flush the documented 100 ms scroll debounce
  assert(stored,'navigation must reach the shared native settings adapter');
  assert.deepEqual(stored.current.taskFilter,['doing']);assert.equal(stored.current.aiRoot,null,'request selection now belongs to the workspace; tray filters still persist');
  const scroll=await page.locator('.content').evaluate(e=>e.scrollTop),saved=structuredClone(stored);
  assert(scroll>0);await page.close();
  for(const version of ['old','new']){
   page=await open({...snapshot,version,navigation:saved});
   assert.equal(await page.locator('h1').innerText(),'Tasks');
   assert.equal(await page.locator('.filter-header [data-state="doing"]').getAttribute('aria-pressed'),'true');
   assert.equal(await page.locator('[data-card]').count(),18);
   assert.equal(await page.locator('.content').evaluate(e=>e.scrollTop),scroll);
   // Service stop/restart changes observations, never resets the live view from an older saved snapshot.
   await page.evaluate(s=>taskq.receive(s),{...snapshot,service:{state:'exited'},navigation:null});
   assert.equal(await page.locator('[data-card]').count(),18);
   if(process.env.VIEW_EVIDENCE){fs.mkdirSync(process.env.VIEW_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.VIEW_EVIDENCE+'/restored-'+version+'.png'});await page.locator('.content').evaluate(e=>e.scrollTop=0);await page.screenshot({path:process.env.VIEW_EVIDENCE+'/restored-'+version+'-top.png'});await page.locator('.content').evaluate((e,s)=>e.scrollTop=s,scroll);}
   await page.locator('.shell-overflow > summary').click();await page.locator('[data-route="settings"]').click();await page.locator('[data-a="back"]').click();
   assert.equal(await page.locator('.content').evaluate(e=>e.scrollTop),scroll);
   await page.locator('[data-a=back]').click();assert.equal(await page.locator('.service').evaluate(d=>d.open),true);
   await page.close();
  }
  page=await open({...snapshot,navigation:{...saved,current:{...saved.current,page:'removed-project',project:'removed-project',taskFilter:['obsolete'],aiRoot:'removed-project'}}});
  assert.equal(await page.locator('h1').innerText(),'Projects');await page.close();
  page=await open({...snapshot,navigation:{...saved,current:{...saved.current,page:'settings/removed',project:null}}});
  assert.equal(await page.locator('h1').innerText(),'Settings');await page.close();
  page=await open({...snapshot,navigation:saved});
  await page.evaluate(s=>taskq.receive({...s,projects:[]}),snapshot);
  assert.equal(await page.locator('h1').innerText(),'Projects','live project removal uses the same fallback');
  await page.evaluate(()=>taskq.receive({viewStateError:'tray.json: save failed; view state is not persisted'}));
  assert.match(await page.locator('[role=alert]').innerText(),/Settings unknown.*save failed/);assert.doesNotMatch(await page.locator('[role=alert]').innerText(),/defaults shown/);
  await page.evaluate(s=>taskq.receive({...s,projects:[]}),snapshot);
  assert.match(await page.locator('[role=alert]').innerText(),/save failed/,'a poll cannot clear a failed save');
  await page.evaluate(()=>taskq.receive({viewStateError:null}));assert.equal(await page.locator('[role=alert]').count(),0);await page.close();
  assert.deepEqual(errors,[]);console.log('PASS M8/UI02 save, service/tray restart, newer release, filters/scroll/Back and removed-value fallback');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
