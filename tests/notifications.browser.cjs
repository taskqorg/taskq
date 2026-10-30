// #1964 M14/M-A24–26: current work stays visible; reading/clearing never resolves questions.
// Risk: results displace work, grouping drops events, or bulk read consumes later arrivals.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/task-workspace.json'),out=process.env.TASKQ_NOTIFICATION_EVIDENCE;
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());let cells=0;try{
 for(const theme of ['dark','light'])for(const font of [16,20]){
  const page=await browser.newPage({viewport:{width:320,height:750}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));await page.addScriptTag({content:fs.readFileSync('tests/fixtures/notifications-shots.js','utf8')});
  await page.evaluate(s=>taskqNotificationsInstall(s),{...fx.state,theme,font});
  const fit=async()=>assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth&&document.querySelector('.content').scrollWidth<=document.querySelector('.content').clientWidth),'No horizontal scroll at 320/'+theme+'/'+font);
  const shot=async name=>{if(out&&font===16){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:path.join(out,name+'-'+theme+'.png')});}};
  assert.equal(await page.locator('.report-row,.reports-section,.notification-row').count(),0,'Home keeps work without result/event rows');
  assert.equal(await page.locator('[data-card]').count(),2);assert.equal(await page.locator('.question-shortcut').count(),1);await fit();await shot('home');
  await page.getByRole('button',{name:'Notifications · 12 unread',exact:true}).click();
  assert.equal(await page.locator('[data-notification-group]').count(),1);assert.equal(await page.locator('.notification-row:visible').count(),2);await fit();await shot('panel');
  assert.equal(await page.evaluate(()=>notificationSent.filter(m=>m.a==='reports'||m.a==='notification-state').length),0,'Panel/group navigation acknowledges nothing');
  await page.locator('[data-notification-group]>summary').click();assert.equal(await page.locator('.notification-row:visible').count(),12);await fit();
  await page.locator('[data-notification-group] .notification-row').first().click();await page.locator('.report-card').waitFor();
  assert.match(await page.locator('.report-card').innerText(),/Delivered series item 10/);await fit();await shot('result');
  await page.locator('[data-report=back]').click();assert.match(await page.locator('[data-notification-group]>summary').innerText(),/10 events · 9 unread/);
  await page.evaluate(()=>{notificationDelayed=true;notificationFail=true;});await page.getByRole('button',{name:'Mark all read',exact:true}).click();
  await page.evaluate(()=>{while(notificationReplies.length)notificationReplies.shift()();});
  await page.getByText('Notification state was not saved; previous state retained',{exact:true}).waitFor();
  assert.equal(await page.locator('[data-notification-group]').count(),1,'Failed read keeps group');
  await page.evaluate(()=>{notificationFail=false;notificationDelayed=true;});await page.getByRole('button',{name:'Mark all read',exact:true}).click();
  // Arrives while result read receipts are still in flight: bulk read binds only its displayed members.
  await page.evaluate(()=>{const s=notificationFixture;s.projects[0].events.push([s.now+1,'failed','new',21]);taskq.receive({...s,page:undefined});while(notificationReplies.length)notificationReplies.shift()();notificationDelayed=false;});
  await page.getByRole('button',{name:'Notifications · 1 unread',exact:true}).waitFor();
  await page.getByRole('button',{name:'Clear read',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.notification-row').length===1);
  assert.match(await page.locator('.notification-row').innerText(),/Unread · Failed/);await fit();await shot('cleared');
  // Native persisted marker projection is restored independently of the navigation state.
  await page.evaluate(()=>taskq.receive({...notificationFixture,page:'home'}));assert.equal(await page.locator('.question-shortcut').count(),1,'Read/clear cannot answer or age out question');assert.equal(await page.locator('[data-card]').count(),2);
  await page.locator('.question-shortcut').click();assert.equal(await page.evaluate(()=>notificationSent.filter(m=>m.a==='workspace').at(-1).open.task),20,'Find what needs me reaches exact question');
  await page.locator('[data-route=notifications]').click();await page.locator('.notification-row').click();await page.locator('[data-card$="/21"]').waitFor();
  assert.equal(await page.evaluate(()=>notificationSent.filter(m=>m.a==='answer').length),0);
  // Large count and partial observation: numeric badge remains numeric; exact accessible count survives the cap.
  await page.evaluate(()=>{const s=notificationFixture,p=s.projects[0];p.events=Array.from({length:105},(_,i)=>[s.now+i,'gone','bulk-'+i,21]);p.reports.reports=[];s.notify.inbox={};taskq.receive({...s,page:'home'});});
  assert.equal(await page.locator('.notifications-entry .badge').textContent(),'99+');assert.equal(await page.locator('.notifications-entry').getAttribute('aria-label'),'Notifications · 105 unread');
  await page.evaluate(()=>{notificationFixture.projects[0].reports.complete=false;taskq.receive({...notificationFixture,page:undefined});});assert.equal(await page.locator('.notifications-entry .badge').textContent(),'99+');assert.match(await page.locator('.notifications-entry').getAttribute('aria-label'),/105 loaded unread · history incomplete/);assert(!await page.locator('.notifications-entry').innerText().then(t=>t.includes('?')));
  assert.deepEqual(errors,[]);cells++;await page.close();
 }
 console.log('PASS M-A24/M-A25/M-A26: '+cells+' Chromium cells, dark/light × 16/20px at 320px; Home, exact targets, group, failures, race, clear, question retention, 99+, partial history');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
