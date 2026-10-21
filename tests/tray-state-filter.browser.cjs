// R6 / K-A2/K-A6 / UI25–UI28: counter toggles replace checkboxes without losing OR,
// project scope, keyboard, refresh/Back retention or view-only protection. Invented data, no native/board claim.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.FILTER_EVIDENCE;
const states=['doing','review','ask','failed','ready','waiting','later','closed'];
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],closed:null,tasks:[{n:1,state:'doing',title:'Build the menu',revision:1},{n:2,state:'review',title:'Review the menu',revision:1},{n:3,state:'ready',title:'Next task',revision:1}],cap_saved:{error:'Fixture'}};
const snapshot={platform:'windows',font:16,theme:'dark',host:'fixture',now:1000,version:'fixture',projects:[project,{...project,root:'/fixture/beta',name:'Beta',issues:'https://example.org/beta/issues'},{...project,root:'/fixture/empty',name:'Empty',issues:'https://example.org/empty/issues',closed:0,tasks:[]}],ai:{draft:''},notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:360,height:760}}),observations=[];
  page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(s=>taskq.receive({...s,page:'/fixture/alpha'}),snapshot);
  await page.locator('.tqp-counts [data-state="doing"]').click();
  const strip=page.locator('.filter-header .tqp-counts'),counter=s=>strip.locator('[data-state="'+s+'"]');
  const rows=()=>page.locator('[data-card]').count();
  assert.deepEqual(await strip.locator('[data-state]').evaluateAll(es=>es.map(e=>e.dataset.state)),states,'K-A2: every state, including zeros, must stay available');
  assert.equal(await page.locator('details.state-filter,input[data-filter-state],input[data-filter-all]').count(),0,'UI25: obsolete checkbox filter removed');
  assert.equal(await counter('failed').innerText(),'','zero is icon-only');
  assert.equal(await counter('failed').locator('svg').count(),1);
  assert.match(await counter('failed').getAttribute('aria-label'),/Failed: 0/);
  assert.equal((await counter('closed').innerText()).trim(),'?');
  assert.equal(await rows(),1);assert.equal(await counter('doing').getAttribute('aria-pressed'),'true');
  assert.equal(await strip.locator('[data-a="all-tasks"]').innerText(),'All tasks 6','open count spans projects even in a project filter');
  await counter('review').click();assert.equal(await rows(),2,'selected states combine with OR');
  await page.evaluate(s=>taskq.receive(s),snapshot);
  assert.equal(await rows(),2);assert.equal(await counter('review').getAttribute('aria-pressed'),'true');
  await counter('review').focus();await page.keyboard.press('Space');assert.equal(await rows(),1);
  assert.equal(await counter('review').evaluate(e=>e===document.activeElement),true);
  assert.equal(await counter('review').getAttribute('aria-pressed'),'false');
  await page.keyboard.press('Enter');assert.equal(await rows(),2);
  await counter('doing').click();assert.equal(await rows(),1);
  await counter('review').click();assert.equal(await rows(),3,'clearing the last state removes restriction');
  await counter('failed').click();assert.equal(await rows(),0,'zero states can be selected');
  await page.getByRole('button',{name:'Reset',exact:true}).click();assert.equal(await rows(),3);
  await counter('closed').click();assert.equal(await rows(),0,'Closed remains its own filter');
  assert.match(await page.locator('.content').innerText(),/Closed.*unknown/);
  await counter('closed').click();await counter('doing').click();await counter('review').click();
  await page.locator('.filter-header [data-tqp="project"]').click();await page.locator('[data-a="back"]').click();assert.equal(await rows(),2);
  const shot=async name=>{if(out){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:out+'/'+name+'.png'});}};
  for(const width of [320,360])for(const font of [16,20])for(const theme of ['light','dark']){
    await page.setViewportSize({width,height:760});await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme});
    const at=width+'/'+font+'/'+theme;
    assert.equal(await rows(),2);
    const layout=await strip.evaluate(e=>({fits:e.scrollWidth<=e.clientWidth&&document.documentElement.scrollWidth<=innerWidth,order:[...e.querySelectorAll('[data-state]')].map(b=>b.dataset.state),pressed:[...e.querySelectorAll('[aria-pressed="true"]')].map(b=>b.dataset.state),outlined:[...e.querySelectorAll('[aria-pressed="true"]')].every(b=>parseFloat(getComputedStyle(b).outlineWidth)>0)}));
    assert.equal(layout.fits,true,at+' wraps without overflow');assert.equal(layout.outlined,true,at+' selected outline');
    assert.deepEqual(layout.order,states);assert.deepEqual(layout.pressed,['doing','review']);
    await shot(theme+'-'+font+'-'+width+'-toggles');
    await strip.getByRole('button',{name:'All tasks: 6 open',exact:true}).click();
    assert.equal(await rows(),6,'All tasks removes project/state restrictions');
    assert.deepEqual(await page.locator('.all-tasks-project > .tqp-project').evaluateAll(es=>es.map(e=>e.getAttribute('aria-label'))),['Alpha','Beta'],'nonzero projects keep their own counters and tasks');
    assert.equal(await page.locator('.all-tasks-project [data-state="failed"]').count(),0,'project summaries still hide zero');
    assert.equal(await strip.locator('[data-state][aria-pressed="true"]').count(),0);
    assert.equal(await strip.locator('[data-a="all-tasks"]').getAttribute('aria-pressed'),'true','TW8/#1455: unrestricted All tasks is selected');
    assert.equal(await page.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth),true,at+' all projects fit');
    await shot(theme+'-'+font+'-'+width+'-all');
    await counter('doing').click();assert.equal(await rows(),2,'global state filter spans projects');
    await page.locator('[data-a="back"]').click();assert.equal(await rows(),2,'Back restores project OR filter');
    observations.push({width,font,theme,...layout,allTasks:6,projects:['Alpha','Beta']});
  }
  await page.evaluate(s=>taskq.receive({...s,projects:s.projects.map(p=>({...p,complete:false}))}),snapshot);
  assert.equal((await counter('failed').innerText()).trim(),'?','partial is unknown, never zero');
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>!['size','ready','web-observation'].includes(m.a)&&!(m.a==='set'&&m.navigation&&Object.keys(m).every(k=>['a','context','navigation'].includes(k))))),[],'view filters must not write queue/focus');
  if(out)fs.writeFileSync(out+'/observations.json',JSON.stringify({browser:browser.version(),observations},null,2)+'\n');
  console.log('PASS #1438 K-A2/K-A6, UI25–UI28: toggles, zero/unknown, OR, All tasks, keyboard, refresh, Back, no writes; 8 theme/type/width cells');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
