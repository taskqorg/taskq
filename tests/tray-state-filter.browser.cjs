// UI25–UI28 / #1350: state OR, project AND, single-click shortcuts, keyboard and refresh retention.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.FILTER_EVIDENCE;
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],closed:null,tasks:[{n:1,state:'doing',title:'Build the menu',revision:1},{n:2,state:'review',title:'Review the menu',revision:1},{n:3,state:'ready',title:'Next task',revision:1}],cap_saved:{error:'Fixture'}};
const snapshot={platform:'windows',font:16,theme:'dark',host:'fixture',now:1000,version:'fixture',projects:[project,{...project,root:'/fixture/beta',name:'Beta',issues:'https://example.org/beta/issues'}],ai:{draft:''},notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:360,height:760}});
  page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(s=>taskq.receive({...s,page:'/fixture/alpha'}),snapshot);
  const counter=s=>page.locator('.tqp-counts [data-state="'+s+'"]');
  const checkbox=s=>page.locator('input[data-filter-state="'+s+'"]');
  const rows=()=>page.locator('[data-card]').count();
  const shot=async name=>{if(out){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:out+'/'+name+'.png'});}};
  await counter('doing').click();
  assert.equal(await page.locator('details.state-filter').count(),1,'missing on-demand Filter disclosure');
  assert.equal(await page.locator('details.state-filter').getAttribute('open'),null);
  assert.equal(await checkbox('doing').isVisible(),false);
  await page.locator('.state-filter > summary').click();
  if(!await checkbox('doing').count())await shot('missing-checkboxes');
  assert.equal(await checkbox('doing').count(),1,'UI25: missing explicit state checkboxes');
  assert.equal(await rows(),1);assert.equal(await counter('doing').getAttribute('aria-pressed'),'true');
  await shot('single');
  await checkbox('review').click();assert.equal(await rows(),2);
  assert.equal(await counter('review').getAttribute('aria-pressed'),'true');
  assert.equal(await page.locator('[data-filter-all]').evaluate(e=>e.indeterminate),true);
  await shot('multi');
  await page.locator('.state-filter > summary').focus();await page.keyboard.press('Space');
  assert.equal(await checkbox('doing').isVisible(),false);
  assert.match(await page.locator('.state-filter > summary').innerText(),/Doing, Review/);
  await shot('folded');
  await page.keyboard.press('Enter');assert.equal(await checkbox('doing').isVisible(),true);
  await page.evaluate(s=>taskq.receive(s),snapshot); // native hide/show retains this WebView; the next snapshot must retain selection
  assert.equal(await checkbox('review').isChecked(),true);assert.equal(await rows(),2);
  await checkbox('review').focus();await page.keyboard.press('Space');assert.equal(await rows(),1);
  assert.equal(await checkbox('review').evaluate(e=>e===document.activeElement),true);
  await counter('ready').click();assert.equal(await rows(),1);assert.equal(await checkbox('doing').isChecked(),false);
  await page.getByRole('button',{name:'Clear all',exact:true}).click();assert.equal(await rows(),3);
  assert.equal(await page.locator('input[data-filter-state]:checked').count(),0);
  assert.match(await page.locator('.state-filter > summary').innerText(),/Filter: All/);await shot('empty');
  await page.locator('[data-filter-all]').click();assert.equal(await page.locator('input[data-filter-state]:checked').count(),8);
  await page.getByRole('button',{name:'Clear all',exact:true}).click();await checkbox('failed').click();assert.equal(await rows(),0);
  await page.getByRole('button',{name:'Reset',exact:true}).click();assert.equal(await rows(),3);
  await counter('closed').click();assert.match(await page.locator('.content').innerText(),/Closed.*unknown/);
  await counter('doing').click();await checkbox('review').click();
  await page.locator('[data-tqp=project]').click();await page.locator('[data-a=back]').click();assert.equal(await rows(),2);
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>!['size','ready','web-observation'].includes(m.a))),[],'view filters must not write queue/focus');
  await page.locator('.state-filter > summary').evaluate(e=>e.parentElement.open=true);
  for(const width of [320,360])for(const font of [14,16,18,20])for(const theme of ['light','dark']){
    await page.setViewportSize({width,height:760});await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme});
    assert.equal(await rows(),2);
    assert.equal(await page.locator('.state-filter').evaluate(e=>e.scrollWidth<=e.clientWidth),true,'filter fits width '+width+'/'+font+'/'+theme+' '+JSON.stringify(await page.locator('.state-filter').evaluate(e=>[e,...e.children].map(n=>({tag:n.tagName,w:n.clientWidth,scroll:n.scrollWidth,html:n.outerHTML.slice(0,100)})))));
    assert.equal(await page.locator('.filter-options label').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=32)),true,'fine-pointer targets');
  }
  console.log('PASS #1350 single/multi/empty, project scope, counters, keyboard, refresh, Back and no writes');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
