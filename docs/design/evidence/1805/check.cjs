// Stage 1 development instrument: invented data; no board, folder or native actions.
const fs=require('node:fs'),path=require('node:path'),{pathToFileURL}=require('node:url'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const root=path.resolve(__dirname,'../../../..'),evidence=process.argv.includes('--evidence');
(async()=>{const browser=await chromium.launch(require(path.join(root,'tests/browser.cjs'))());try{
 const page=await browser.newPage({viewport:{width:390,height:850}}),errors=[];page.on('pageerror',e=>errors.push(String(e)));
 const file=pathToFileURL(path.join(__dirname,'mockup.html'));
 for(const variant of ['a','b']){
  await page.goto(file.href+'?variant='+variant);await page.evaluate(()=>localStorage.clear());await page.reload();
  const group=page.locator('[data-project=taskq]'),toggle=group.locator('.toggle');
  assert.equal(await group.locator('.group-rows').isVisible(),true);await toggle.focus();await page.keyboard.press('Space');assert.equal(await group.locator('.group-rows').isVisible(),false);assert.equal(await toggle.getAttribute('aria-expanded'),'false');
  await page.reload();assert.equal(await group.locator('.group-rows').isVisible(),false,'prototype collapse survives reopen');await toggle.press('Enter');assert.equal(await group.locator('.group-rows').isVisible(),true);
  for(const [selector,text] of [['.group-name','Project overview'],['.group-actions button:first-child','github board'],['.group-actions button:last-child','Registered project folder']]){await group.locator(selector).click();assert.match(await page.locator('dialog').innerText(),new RegExp(text));await page.getByText('Back to task list',{exact:true}).click();assert.equal(await group.locator(selector).evaluate(e=>e===document.activeElement),true,'Back restores action focus');}
  assert.match(await page.locator('[data-project=unknown] .counts').getAttribute('aria-label'),/unknown.*unavailable/,'unknown retains a reason');assert.equal(await page.locator('[data-project=unknown] .note').isVisible(),false,'collapsed group shows only its header');
  assert.match(await page.locator('[data-project=agentq] .counts').innerText(),/2 shown\s*· 2 total/,'equal counts keep both labels');
  const checks=[];
  for(const theme of ['light','dark'])for(const width of [320,360,470])for(const font of [14,16,18,20]){
   await page.setViewportSize({width:width+24,height:1200});await page.evaluate(({width,font,theme})=>{document.documentElement.style.colorScheme=theme;document.querySelector('.menu').style.width=width+'px';document.querySelector('.menu').style.setProperty('--size',font+'px');},{width,font,theme});
   const overflow=await page.locator('.menu *').evaluateAll(es=>es.filter(e=>e.clientWidth&&e.scrollWidth>e.clientWidth+1).map(e=>e.className));assert.deepEqual(overflow,[],variant+'/'+theme+'/'+width+'/'+font+' overflow');
   const bad=await page.locator('.group-header button').evaluateAll(es=>es.filter(e=>e.getBoundingClientRect().width<31.9||e.getBoundingClientRect().height<31.9).map(e=>e.className));assert.deepEqual(bad,[],'32px pointer targets');checks.push(theme+'/'+width+'/'+font);
  }
  if(evidence){await page.setViewportSize({width:390,height:850});await page.evaluate(()=>{document.documentElement.style.colorScheme='dark';document.querySelector('.menu').style.width='360px';document.querySelector('.menu').style.setProperty('--size','16px');});await page.screenshot({path:path.join(__dirname,'variant-'+variant+'.png'),fullPage:true});}
  console.log('PASS prototype '+variant+': '+checks.length+' width/font/theme cells; collapse/reopen, distinct targets, focus return and equal counts');
 }
 if(process.argv.includes('--baseline')){
 // Failure before: exact product renderer, unchanged baseline source, same group scenario.
 const task=(n,state)=>({n,state,title:'Keep project tasks readable and actions direct',revision:n+1,since:900,created:500,controls:[]});
 const p={root:'/fixture/taskq',name:'taskq',host:'fixture',issues:'https://example.org/org/taskq/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],tasks:[task(1805,'ready'),task(1799,'ready'),task(1800,'doing'),task(1801,'held')]};
 const state={platform:'macos',font:16,theme:'dark',host:'fixture',now:1000,version:'fixture',size_anchor:'auto',attached_edge:'top',top:true,service:{state:'running'},projects:[p],ai:{draft:''},notify:{projects:{}},stop_modes:true,display:require(path.join(root,'tests/fixtures/display-model.json')).display,messages:{complete:true,count:0},navigation:{schema:1,current:{page:'home'},dashboard:{views:[{id:'all',name:'All tasks',working:{states:['ready']}}]}}};
 await page.setViewportSize({width:360,height:760});await page.setContent(fs.readFileSync(path.join(root,'build/web/tray.html'),'utf8'));await page.evaluate(s=>taskq.receive(s),state);
 const before=page.locator('.project-group');assert.equal(await before.count(),1);assert.equal(await before.locator('[aria-expanded]').count(),0,'before: no collapse control');assert.equal(await before.locator('.counts').count(),0,'before: no matched/total counts');assert.deepEqual(await before.locator(':scope > *').evaluateAll(es=>es.map(e=>e.className)),['project-board compact-action','project-overview','compact-action project-folder'],'before: actions split around name');
 if(evidence)await page.screenshot({path:path.join(__dirname,'before.png')});
 console.log('Observed baseline failure A2/A4/A5; production remains unchanged.');}
 assert.deepEqual(errors,[]);console.log('Prototype passes are design evidence only.');
 }finally{await browser.close()}})().catch(e=>{console.error(e);process.exitCode=1});
