// TW1/V02/V06/V09/V23: real bundled shells, invented snapshots, no native actions.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const project={root:'fixture-alpha',name:'Alpha',issues:'https://example.org/alpha/issues',alive:true,complete:true,age:1,focus:[],labels:[],cap_saved:{error:'Invented unavailable capacity'},tasks:Array.from({length:45},(_,i)=>({n:i+1,state:'ready',revision:1,title:'Keep navigation reachable while reading the full task title '+i}))};
const snapshot={platform:'windows',font:20,theme:'dark',host:'fixture',version:'fixture',projects:[project],ai:{draft:''},notify:{status:'unknown',host:{ask:'off',close:'off',failed:'off',gone:'off'},projects:{}},stop_modes:true,period:'1d',history:{start:0,end:1000,since:null,tz:'UTC',bins:Array.from({length:10},()=>[0,0,0,0])}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:320,height:600}}),errors=[],measured={};
  page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(s=>taskq.receive(s),{...snapshot,page:project.root});
  assert.equal(await page.locator('.breadcrumbs').innerText(),'Projects','TW1: project breadcrumbs end at their parent');
  measured.before=await page.locator('[data-a=back]').boundingBox();
  await page.locator('.content').evaluate(e=>e.scrollTop=e.scrollHeight);
  measured.scrolled=await page.locator('[data-a=back]').boundingBox();
  if(process.env.SHELL_EVIDENCE){fs.mkdirSync(process.env.SHELL_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.SHELL_EVIDENCE+'/scrolled.png'});}
  await page.evaluate(()=>document.querySelector('.menu').style.fontSize='40px');
  measured.large=await page.locator('[data-a=hide]').boundingBox();
  measured.backLarge=await page.locator('[data-a=back]').boundingBox();
  if(process.env.SHELL_EVIDENCE){await page.screenshot({path:process.env.SHELL_EVIDENCE+'/200pct.png'});fs.writeFileSync(process.env.SHELL_EVIDENCE+'/geometry.json',JSON.stringify(measured,null,2)+'\n');}
  assert(Math.abs(measured.before.y-measured.scrolled.y)<=1,'G01/V02: Back drifts by more than 1 CSS px');
  for(const r of [measured.large,measured.backLarge])assert(r.x>=0&&r.x+r.width<=321&&r.y>=0&&r.y+r.height<=601,'G02/V23: essential Back/Close clipped at 320px / 200% text');
  await page.evaluate(s=>taskq.receive(s),{...snapshot,page:project.root,projects:[{...project,name:'Project'.repeat(30)}]});
  await page.evaluate(()=>document.querySelector('.menu').style.fontSize='40px');
  const longTitle=await page.evaluate(()=>{const c=document.querySelector('.content'),s=getComputedStyle(c),grip=document.querySelector('.grip').getBoundingClientRect();return {available:c.clientHeight-parseFloat(s.paddingTop)-parseFloat(s.paddingBottom),gripBottom:grip.bottom};});
  assert(longTitle.available>=32&&longTitle.gripBottom<=601,'V23/V26: a long route title must leave usable content and a reachable resize control');
  await page.evaluate(s=>taskq.receive(s),{...snapshot,page:project.root});
  await page.evaluate(()=>document.querySelector('.menu').style.fontSize='20px');
  await page.locator('.tqp-counts [data-state=ready]').first().click();
  assert.equal(await page.locator('h1').innerText(),'Tasks');
  assert.equal(await page.locator('.view-criteria').innerText().then(s=>s.includes('Alpha')),true,'O3: working project criteria remain visible on dashboard');
  await page.locator('[data-route=fixture-alpha]').click();
  assert.equal(await page.locator('h1').innerText(),'Alpha');
  assert.equal(await page.locator('.breadcrumbs').innerText(),'Projects');
  // V23/V26: notification labels must fit in both host and project toggle groups.
  await page.evaluate(s=>taskq.receive({...s,notify:{...s.notify,projects:{[s.projects[0].root]:Object.fromEntries(['ask','close','failed','gone'].map(k=>[k,[null,'off']]))}}}),snapshot);
  await page.locator('details[data-view-key^="notifications:"]').evaluate(e=>e.open=true);
  for(const route of [project.root,'notifications']){
   if(route==='notifications')await page.evaluate(s=>taskq.receive({...s,page:'notifications'}),snapshot);
   for(const font of [14,16,18,20,40]){
    await page.locator('.menu').evaluate((e,font)=>e.style.fontSize=font+'px',font);
    const clipped=await page.locator('[data-a=notify]').evaluateAll(es=>es.filter(e=>e.checkVisibility()&&e.scrollWidth>e.clientWidth+1).map(e=>({text:e.textContent,overflow:e.scrollWidth-e.clientWidth})));
    assert.deepEqual(clipped,[],`V23/V26: ${route} notification labels clip at ${font}px`);
   }
  }
  assert.deepEqual(errors,[]);
  await page.close();
  console.log('PASS G01/G02: fixed Back and reachable Back/Close at 320px / 200% text');
  const tray=await browser.newPage({viewport:{width:320,height:600}});
  await tray.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  // Reviewer #1470: the tray renders route components; any HTML template parse means the string adapter came back.
  await tray.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};window.parses=0;const d=Object.getOwnPropertyDescriptor(Element.prototype,'innerHTML');Object.defineProperty(HTMLTemplateElement.prototype,'innerHTML',{set(v){parses++;d.set.call(this,v);},get(){return d.get.call(this);}});});
  await tray.evaluate(s=>taskq.receive(s),snapshot);
  assert.equal(await tray.evaluate(()=>document.activeElement.tagName),'H1','first tray entry focuses its route heading');
  assert.equal(await tray.locator('h1').evaluate(e=>getComputedStyle(e).outlineStyle),'none','TW1: programmatic heading focus has no box');
  await tray.evaluate(()=>taskq.receive({workspaceError:'Invented construction failure'}));assert.match(await tray.locator('[role=alert]').innerText(),/construction failure/);
  await tray.evaluate(()=>taskq.receive({workspaceError:null}));assert.equal(await tray.locator('[role=alert]').count(),0,'successful workspace retry clears its construction error');
  await tray.locator('[data-route=settings]').click();await tray.locator('[data-route=projects]').first().focus();await tray.locator('[data-route=projects]').first().click();
  const shellRoute=()=>tray.locator('[data-shell-route]').evaluate(e=>e.dataset.shellRoute+' '+e.dataset.shellDepth);assert.equal(await shellRoute(),'/projects 3','A3: navigation pushes a Wouter history entry');
  await tray.locator('[data-a=back]').click();assert.equal(await shellRoute(),'/settings 2','A3: Back pops the Wouter history entry');assert.equal(await tray.evaluate(()=>document.activeElement.dataset.route),'projects','V06: restore the invoking control');
  assert.equal(await tray.evaluate(()=>parses),0,'tray pages render as Preact route components without template parsing');

  await tray.locator('[data-route=appearance]').click();assert.equal(await tray.locator('.breadcrumbs').innerText(),'Settings','TW1: omit the current settings page from breadcrumbs');
  assert.equal(await tray.locator('.breadcrumbs a').count(),1,'TW1: each ancestor segment stays linked');
  await tray.locator('[data-a=back]').click();assert.equal(await tray.evaluate(()=>document.activeElement.dataset.route),'appearance');
  const bar=await tray.locator('.head').elementHandle();await tray.evaluate(s=>taskq.receive({...s,now:1001}),snapshot);assert.equal(await tray.locator('.head').evaluate((e,old)=>e===old,bar),true,'snapshot updates retain the bar DOM');
  await tray.keyboard.press('Alt+ArrowLeft');assert.equal(await tray.locator('h1').innerText(),'Tasks');
  await tray.locator('[data-a=view-editor]').click();await tray.keyboard.press('Escape');
  assert.equal(await tray.locator('#taskq-popover').count(),0,'Escape closes anchored editor first');
  assert.equal(await tray.evaluate(()=>document.activeElement.dataset.a),'view-editor');
  await tray.keyboard.press('Escape');assert.equal(await tray.evaluate(()=>messages.some(m=>m.a==='hide')),true,'Escape then closes only the window');
  const routeRows=[];
  for(const hasTouch of [false,true]){
   const context=await browser.newContext({viewport:{width:320,height:600},hasTouch});
   const p=await context.newPage();p.on('pageerror',e=>errors.push(e.message));await p.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   for(const route of ['home','projects',project.root,'attention','settings','appearance','window','tray-icon','delivery','cadence','notifications','agents','services','about','stop','recovery']){
    await p.evaluate(s=>taskq.receive(s),{...snapshot,page:route});
    assert.equal(await p.locator('h1').count(),1);assert.equal(await p.locator('.head h1').count(),1,route+' has a single fixed title');
    const small=await p.evaluate(min=>{
     // UI06: tiny quantitative bars have an equivalent full-sized interval selector.
     const selector=document.querySelector('[data-history-select]'),r=selector?.getBoundingClientRect(),equivalent=selector?.checkVisibility()&&!selector.disabled&&r.width>=min&&r.height>=min&&selector.options.length===document.querySelectorAll('.bars button').length;
     return [...document.querySelectorAll('button,a,summary,input,select,textarea,[role=button]')].filter(e=>!e.disabled&&e.checkVisibility()&&!(equivalent&&e.matches('.bars button'))).map(e=>({label:e.getAttribute('aria-label')||e.innerText||e.labels?.[0]?.innerText||e.id,rect:(e.matches('input[type=checkbox]')&&e.labels?.[0]?e.labels[0]:e).getBoundingClientRect()})).filter(e=>e.rect.width<min-.1||e.rect.height<min-.1).map(e=>({label:e.label,width:e.rect.width,height:e.rect.height}));
    },hasTouch?44:32);
    await p.keyboard.press('Tab');
    const controls=await p.evaluate(()=>{
     const elements=[...document.querySelectorAll('button,a,summary,input,select,textarea,[role=button]')].filter(e=>!e.disabled&&e.checkVisibility()),unnamed=[],focus=[],overlaps=[];
     const boxes=elements.map(e=>e.getBoundingClientRect());
     for(let i=0;i<elements.length;i++){
      const e=elements[i],label=e.getAttribute('aria-label')||e.labels?.[0]?.innerText||e.innerText||e.title;
      if(!label?.trim())unnamed.push(e.outerHTML.slice(0,100));
      for(let j=0;j<i;j++){
       const a=boxes[i],b=boxes[j];
       if(e.closest('.anchored-popover')!==elements[j].closest('.anchored-popover')||elements[j].contains(e)||e.contains(elements[j])||a.bottom<=0||b.bottom<=0||a.top>=innerHeight||b.top>=innerHeight)continue;
       const clip=e.closest('.content')?.getBoundingClientRect(),other=elements[j].closest('.content')?.getBoundingClientRect();
       if(clip&&(a.top<clip.top||a.bottom>clip.bottom)||other&&(b.top<other.top||b.bottom>other.bottom))continue;
       if(Math.min(a.right,b.right)-Math.max(a.left,b.left)>1&&Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)>1)overlaps.push([label,elements[j].innerText||elements[j].getAttribute('aria-label')]);
      }
      e.focus();if(document.activeElement!==e||parseFloat(getComputedStyle(e).outlineWidth)<1)focus.push(label||e.tagName);
     }
     return {unnamed,focus,overlaps};
    });
    routeRows.push({route,pointer:hasTouch?'coarse':'fine',small,...controls,text:await p.locator('.content').innerText()});
   }
   await context.close();
  }
  if(process.env.SHELL_EVIDENCE)fs.writeFileSync(process.env.SHELL_EVIDENCE+'/routes.json',JSON.stringify(routeRows,null,2)+'\n');
  assert.deepEqual(routeRows.filter(r=>r.small.length),[],'V09: all enabled targets meet both dimensions');
  assert.deepEqual(routeRows.filter(r=>r.unnamed.length||r.focus.length||r.overlaps.length),[],'V09/V11/V13: named, focusable controls with visible focus and no overlap');
  await tray.close();
  const workspace=await browser.newPage({viewport:{width:320,height:600}});
  workspace.on('pageerror',e=>errors.push(e.message));await workspace.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
  await workspace.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await workspace.evaluate(s=>taskq.receive(s),snapshot);assert.equal(await workspace.locator('[data-a=back]').count(),0,'workspace is its own root');
  assert.equal(await workspace.evaluate(()=>document.activeElement.tagName),'H1','first workspace entry focuses its route heading');
  assert.equal(await workspace.locator('h1').evaluate(e=>getComputedStyle(e).outlineStyle),'none','TW1: workspace heading focus has no box');
  if(process.env.SHELL_EVIDENCE)await workspace.screenshot({path:process.env.SHELL_EVIDENCE+'/workspace.png'});
  const space=await workspace.locator('.content button').evaluateAll(es=>{const [a,b]=es.map(e=>e.getBoundingClientRect());return Math.abs(a.top-b.top)<1?b.left-a.right:b.top-a.bottom;});
  assert(space>=4,'V09/V20: workspace route controls need at least 4px separation');
  await workspace.locator('.content [data-route=sessions]').click();assert.match(await workspace.locator('.content').innerText(),/Recorded workers and reviewers/);
  assert.equal(await workspace.locator('.content [data-url]').count(),0,'C-A8: absent native identities cannot fabricate session targets');
  await workspace.evaluate(s=>taskq.receive({...s,projects:[{...s.projects[0],tasks:[{n:7,title:'Invented local attempt',session:'codex:fixture-thread fixture',runtime:'codex',host:'fixture',role:'worker',state:'doing',session_url:'codex://threads/fixture-thread'},{n:8,title:'Invented remote attempt',session:'codex:remote-thread other-host',runtime:'codex',host:'other-host',role:'reviewer',state:'review',session_reason:'Codex session opens only on other-host'}]}]}),snapshot);
  assert.equal(await workspace.getByText('Open recorded session',{exact:true}).count(),1,'C-A8: open only the supplied verified target');
  assert.match(await workspace.locator('.content').innerText(),/opens only on other-host/);
  await workspace.getByText('Open recorded session',{exact:true}).click();assert.equal(await workspace.evaluate(()=>messages.filter(m=>m.a==='open').at(-1).url),'codex://threads/fixture-thread');
  await workspace.locator('[data-a=back]').click();assert.equal(await workspace.evaluate(()=>document.activeElement.dataset.route),'sessions');
  await workspace.locator('.content [data-route=agents]').click();
  const saved=await workspace.evaluate(()=>messages.filter(m=>m.a==='workspace-set').at(-1).navigation);
  await workspace.setContent(fs.readFileSync('build/web/workspace.html','utf8'));await workspace.evaluate(s=>taskq.receive(s),{...snapshot,workspace_navigation:saved});
  assert.equal(await workspace.locator('h1').innerText(),'Agent settings');assert.equal(await workspace.evaluate(()=>document.activeElement.tagName),'H1','hydrated child focuses its route heading');await workspace.locator('[data-a=back]').click();assert.equal(await workspace.locator('h1').innerText(),'Inbox');
  assert.deepEqual(errors,[]);await workspace.close();
  console.log('PASS TW1/V06/V09: route titles, invoker focus, stable bar, keyboard Back, fine/coarse targets and independent workspace reload/Back');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
