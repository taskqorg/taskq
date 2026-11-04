// R6/R12, #2351 A3–A5: nine copies of a board outage must not replace data or grow either page.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fixture=require('./fixtures/errors.js'),base=require('./fixtures/task-workspace.json').state;
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 for(const surface of ['tray','workspace']){
  const page=await browser.newPage({viewport:{width:surface==='tray'?620:1000,height:800}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));
  const f=fixture(base);
  await page.evaluate(s=>{window.ipc={postMessage(){}};taskq.receive(s);},f.fresh);
  if(surface==='workspace')await page.evaluate(()=>taskq.receive({open:{page:'conversations',filter:'all'}}));
  const rows=surface==='tray'?'.task-row':'.conversation-row';
  const before=await page.locator(rows).count();assert(before>0);
  await page.evaluate(s=>taskq.receive(s),f.failed);
  assert.equal(await page.locator('.errors-panel').count(),1,'one steady header errors icon');
  assert.equal(await page.locator('.errors-entry .badge').innerText(),'1','one cause, not nine projects');
  assert.equal(await page.locator(rows).count(),before,'outage retains rows');
  assert.equal(await page.locator('.content').getByText(/HTTP 403|board requests paused/).count(),0,'no per-project error prose');
  if(surface==='tray'){assert.match(await page.locator('[data-host=fixture]').innerText(),/9/);assert.match(await page.locator('[data-host=fixture]').getAttribute('aria-label'),/Last known.*15m/i);}
  const height=await page.locator('.shell').evaluate(e=>e.getBoundingClientRect().height);
  await page.locator('.errors-entry').click();
  assert.equal(await page.locator('.errors-list>li').count(),1);
  assert.match(await page.locator('.errors-list>li').innerText(),/GitHub limit until .*9 projects/);
  await page.locator('.errors-list>li>details>summary').click();assert.equal(await page.locator('.error-projects li').count(),9);
  assert.equal(await page.locator('.shell').evaluate(e=>e.getBoundingClientRect().height),height,'errors overlay does not grow page');
  await page.evaluate(s=>taskq.receive(s),f.fresh);
  assert.equal(await page.locator('.errors-entry .badge').innerText(),'0','recovery removes cause without acknowledgement');
  assert.equal(await page.locator('.errors-panel').count(),1,'icon never disappears');
  if(surface==='workspace'){await page.locator('.errors-entry').click();await page.locator('.conversation-view>summary').click();await page.locator('[data-c=result-filter][data-filter=needs]').click();assert.match(await page.locator('.conversation-row').innerText(),/Keep this question/);}
  await page.evaluate(s=>taskq.receive({...s,settings_error:'tray.json: corrupt (invented fixture); file kept'}),f.fresh);
  assert.match(await page.locator('.errors-entry [role=status]').innerText(),/tray.json: corrupt/,'corrupt settings remain readable and announced in the header');
  assert.equal(await page.locator('.content').getByText(/tray.json: corrupt/).count(),0);
  await page.evaluate(()=>taskq.receive({viewStateError:'Invented navigation save failure'}));
  await page.evaluate(s=>taskq.receive(s),f.fresh);
  assert.match(await page.locator('.errors-entry [role=status]').innerText(),/navigation save failure/,'a status poll cannot clear an unconfirmed save');
  await page.evaluate(()=>taskq.receive({viewStateError:null}));assert.equal(await page.locator('.errors-entry .badge').innerText(),'0');
  assert.deepEqual(errors,[]);await page.close();
  if(surface==='tray'){
   const cold=await browser.newPage({viewport:{width:420,height:800}});await cold.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   await cold.evaluate(s=>{window.ipc={postMessage(){}};taskq.receive(s);},f.failed);
   assert.match(await cold.locator('[data-host=fixture]').innerText(),/9/,'cold open retains dated snapshot counts');
   assert.match(await cold.locator('[data-host=fixture]').getAttribute('aria-label'),/Last known.*15m/);
   const height=await cold.locator('.content-body').evaluate(e=>e.scrollHeight);
   const noisy=structuredClone(f.failed);noisy.projects.forEach((p,i)=>p.error='Independent invented failure '+i);
   await cold.evaluate(s=>taskq.receive(s),noisy);
   assert.equal(await cold.locator('.content-body').evaluate(e=>e.scrollHeight),height,'page height does not grow with distinct errors');
   assert.equal(await cold.locator('.errors-list>li').count(),10,'independent causes stay separate');await cold.close();
   const narrow=await browser.newPage({viewport:{width:320,height:600}});await narrow.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   await narrow.evaluate(s=>{window.ipc={postMessage(){}};taskq.receive({...s,page:s.projects[0].root,stop_modes:true});document.querySelector('.menu').style.fontSize='40px';},f.failed);
   for(const selector of ['[data-a=back]','[data-a=hide]','.errors-entry']){const r=await narrow.locator(selector).boundingBox();assert(r.x>=0&&r.x+r.width<=321&&r.y>=0&&r.y+r.height<=601,'header controls remain reachable at 200% text');}
   await narrow.close();
  }
 }
 console.log('PASS nine-project outage: one cause, retained rows/host ages, stable height, recovery and Needs me');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
