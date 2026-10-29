// #1808 A1: review-only CS1–CS6 fixture. Protect grouping, identity, search and local-action semantics;
// this does not qualify production metadata, native persistence or provider activity timestamps.
const fs=require('node:fs'),path=require('node:path'),http=require('node:http'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const root=path.resolve(__dirname,'..'),out=path.join(root,'docs/design/evidence/conversations-1808');
(async()=>{
 const server=http.createServer((req,res)=>{const file=path.resolve(root,'.'+decodeURIComponent(req.url.split('?')[0]));if(!file.startsWith(root+path.sep)){res.writeHead(403).end();return;}fs.readFile(file,(err,data)=>{res.writeHead(err?404:200,{'Content-Type':file.endsWith('.html')?'text/html':file.endsWith('.json')?'application/json':'text/plain'});res.end(err?'Not found':data);});});
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:1100,height:850}}),errors=[];page.on('pageerror',e=>errors.push(e.message));fs.mkdirSync(out,{recursive:true});
  const url='http://127.0.0.1:'+server.address().port+'/docs/design/exploration/conversations-sidebar.html';
  await page.goto(url+'?baseline=1');await page.waitForFunction(()=>window.sidebarReady);let baseline=page.frames().find(f=>f.url().endsWith('/workspace.html'));
  assert.equal(await baseline.locator('[data-group]').count(),0,'before: current workspace has no groups');assert.equal(await baseline.locator('.conversation-row summary').count(),0);await page.screenshot({path:out+'/00-before.png'});
  console.log('BEFORE: source workspace has 0 groups and 0 row-action menus (CS1/CS4 gap).');
  await page.goto(url);await page.waitForFunction(()=>window.sidebarReady);
  const frame=page.frames().find(f=>f.url().endsWith('/workspace.html'));
  const order=()=>frame.locator('[data-group]').evaluateAll(es=>es.map(e=>e.dataset.group)),ids=()=>frame.locator('[data-row]').evaluateAll(es=>es.map(e=>e.dataset.row));
  assert.deepEqual(await order(),['pinned','project:taskq','project:dashq','other']);assert.equal((await ids()).length,new Set(await ids()).size);
  assert.deepEqual(await frame.locator('[data-group="project:taskq"] [data-row]').evaluateAll(es=>es.map(e=>e.dataset.row)),['taskq-question','taskq-release','taskq-long']);
  await frame.locator('[data-row="taskq-question"] summary').focus();await page.keyboard.press('Enter');await page.screenshot({path:out+'/01-menu.png'});await frame.getByRole('button',{name:'Pin',exact:true}).click();
  assert.equal(await frame.locator('[data-group=pinned] [data-row="taskq-question"]').count(),1);assert.equal(await frame.locator('[data-row="taskq-question"]').count(),1);
  await frame.locator('[data-row="taskq-question"] summary').click();await frame.getByRole('button',{name:'Unpin',exact:true}).click();
  await frame.locator('[data-row="taskq-question"] summary').focus();await page.keyboard.press('Enter');await page.keyboard.press('Escape');assert(await frame.locator('[data-row="taskq-question"] summary').evaluate(e=>e===document.activeElement&&!e.parentElement.open));
  await frame.locator('[data-group="project:taskq"]>.sidebar-group-title').click();assert.equal(await frame.locator('[data-group="project:taskq"] [data-row]').count(),0);
  await page.reload();await page.waitForFunction(()=>window.sidebarReady);const f=page.frames().find(f=>f.url().endsWith('/workspace.html'));
  assert.equal(await f.locator('[data-group="project:taskq"] [data-row]').count(),0,'collapse survives reopening');
  await f.getByRole('searchbox').fill('old API');assert.equal(await f.locator('[data-row="taskq-question"]').count(),1,'search reveals collapsed group');await page.screenshot({path:out+'/03-search.png'});
  await f.getByRole('searchbox').fill('');assert.equal(await f.locator('[data-group="project:taskq"] [data-row]').count(),0,'clear restores collapse');
  await f.locator('[data-group="project:taskq"]>.sidebar-group-title').click();
  await page.locator('#update').click();assert.deepEqual(await f.locator('[data-group]').evaluateAll(es=>es.map(e=>e.dataset.group)),['pinned','project:dashq','project:taskq','other']);assert.equal(await f.locator('[aria-current=page][data-select]').innerText().then(t=>t.split('\n')[0]),'Daily PM');
  await f.locator('[data-row="pm-taskq"] summary').click();await f.locator('[data-row="pm-taskq"]').getByRole('button',{name:'Archive',exact:true}).click();assert.equal(await f.locator('[data-row="pm-taskq"]').count(),0);assert.equal(await f.locator('.conversation-composer:visible').count(),0,'archived selected conversation has no reply composer');
  await f.locator('.list-tabs').getByRole('button',{name:'Archive',exact:true}).click();assert.equal(await f.locator('[data-row="pm-taskq"]').count(),1);await f.locator('[data-row="pm-taskq"] summary').click();await page.screenshot({path:out+'/04-archive.png'});await f.getByRole('button',{name:'Restore',exact:true}).click();
  await f.getByRole('button',{name:'Recent',exact:true}).click();assert.equal(await f.locator('[data-group=pinned] [data-row="pm-taskq"]').count(),1,'restore retains pin');
  await page.locator('#reset').click();await page.locator('#variant').selectOption('inline');await f.locator('[data-row="taskq-question"] [data-select]').focus();await page.keyboard.press('Tab');assert.equal(await f.evaluate(()=>document.activeElement.getAttribute('aria-label')),'Pin','inline actions reachable through Tab');await page.screenshot({path:out+'/02-inline.png'});
  // Same state/fixture, both themes and text sizes: no horizontal content overflow or unreachable row actions.
  for(const width of [1100,500])for(const font of [16,20])for(const mode of ['menu','inline'])for(const theme of ['dark','light']){
   await page.setViewportSize({width,height:850});await page.locator('#variant').selectOption(mode);
   if(width===500&&!await f.locator('.conversation-list').isVisible())await f.locator('[data-c=list]').click();
   assert(await f.locator('.conversation-list').isVisible(),'geometry checks the visible list');
   await page.locator('#theme').selectOption(theme);await f.evaluate(font=>document.querySelector('.menu').style.setProperty('--size',font+'px'),font);
   assert(await f.locator('.conversation-list').evaluate(e=>e.scrollWidth<=e.clientWidth+1),'sidebar fits '+[width,font,mode]);
  }
  await page.locator('#theme').selectOption('light');await page.screenshot({path:out+'/05-narrow-light.png'});
  assert.deepEqual(errors,[]);
  console.log('PASS #1808 design fixture: unique grouping/order, pin/unpin, collapse/reopen/search, event reorder, retained selection, archive/restore/pin, menu keyboard, 16 geometry cells; native/runtime Not tested');
 }finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(e=>{console.error(e);process.exitCode=1;});
