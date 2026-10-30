// #1709 A1–A4 supersedes #1701's one-line filters/height budget. Keep host identities, counters and Attention;
// catch horizontal scrolling, masks and clipped chips in the menu, View window and Settings.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const states=['doing','review','ask','failed','ready','waiting','later','held'];
const exec=(id,host,runtime)=>({id,attempt:id,host,runtime,role:'worker',state:'running',observed:990,live:true});
const longFresh='buildworker'+'0123456789'.repeat(5),longStale='staleworker'+'9876543210'.repeat(5);
const hosts=[{name:longFresh,updated:990,live:true},{name:longStale,updated:690,live:false,age:310,age_text:'5m'},{name:'mac',updated:990,live:true},{name:'win',updated:990,live:true},{name:'hermes-fra1',updated:690,live:false,age:310,age_text:'5m'}];
const projects=['taskq','pilotq','agentq','dashq','coreq','serverq','specq','csgo'].map((name,i)=>({root:'/fixture/'+name,name,host:'mac',issues:'https://example.org/'+name+'/issues',board_kind:'github',alive:true,complete:true,age:1,tasks_age_seconds:0,tasks_live:true,observed:1000,hosts,focus:['all'],labels:[],
 ...(i===0?{error:'Fixture urgent fault'}:{}),
 tasks:[{n:i+1,state:states[i],title:name+' task '+(i+1),revision:1,...(i===0?{runtime:'claude',host:'mac',local_session:true,executions:[exec(10,'mac','claude')]}:i===1?{runtime:'codex',host:'win',executions:[exec(11,'win','codex')]}:i===2?{executions:[exec(12,'pilot','codex')]}:{})}]}));
const navigation={schema:1,current:{page:'home'},dashboard:{selected:'all',views:[{id:'view-running',name:'Running',pinned:true,working:{states:['doing','review']}},{id:'view-answers',name:'Waiting for answer',pinned:true,working:{states:['ask']}}]}};
const snapshot={display:require('./fixtures/display-model.json').display,platform:'macos',font:18,theme:'dark',host:'mac',now:1000,version:'fixture',projects,notify:{host:{ask:'on',close:'on',failed:'on',gone:'on'},projects:{}},stop_modes:true,navigation,period:'1d',history:require('./fixtures/icon-history/day-dark.json')};
const html=fs.readFileSync('build/web/tray.html','utf8');
const overflow=page=>page.evaluate(()=>[...document.querySelectorAll('*')].filter(e=>e.checkVisibility()).flatMap(e=>{
 const style=getComputedStyle(e);
 return e.scrollWidth>e.clientWidth+1||/^(auto|scroll)$/.test(style.overflowX)?[{element:e.tagName+'.'+e.className,width:e.clientWidth,scroll:e.scrollWidth,overflow:style.overflowX}]:[];
}));
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const width of [320,360,420])for(const font of [14,16,18,20]){
  const page=await browser.newPage({viewport:{width,height:760}});
  page.on('pageerror',e=>{throw e;});
  await page.setContent(html);
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>messages.push(JSON.parse(s))};});
  for(const theme of ['dark','light']){
   await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme});
   await page.evaluate(()=>document.fonts.ready);
   if(process.env.PANEL_EVIDENCE&&width===320&&font===20&&theme==='dark'){
    fs.mkdirSync(process.env.PANEL_EVIDENCE,{recursive:true});
    await page.screenshot({path:process.env.PANEL_EVIDENCE+'/filter-header.png'});
   }
   const summary=page.locator('.dashboard-status>summary');
   assert.deepEqual(await summary.locator('.host-chip').evaluateAll(cs=>cs.map(c=>[c.dataset.host,c.querySelector('.host-number').textContent])),[[longFresh,'0'],['hermes-fra1','?'],['mac','1'],[longStale,'?'],['win','1']],'UX30-06 A1: per-host totals, idle 0, stale ?');
   assert.deepEqual(await summary.locator('.host-runtime-totals [data-runtime]').evaluateAll(es=>es.map(e=>[e.dataset.runtime,e.querySelector('.host-number').textContent])),[['Codex','1+?'],['Claude','1+?']],'A3/A4: execution-only host retains overall uncertainty');
   assert(await summary.locator('.host-chip').evaluateAll(cs=>cs.every(c=>c.title===c.getAttribute('aria-label')&&c.title.startsWith(c.dataset.host))),'full name, counts and reasons stay accessible');
   assert.deepEqual(await summary.locator('.host-chip strong').allTextContents(),['bui','her','mac','sta','win']);
   const line=await summary.locator('.host-chip').evaluateAll(cs=>cs.map(c=>{const r=c.getClientRects();return r.length===1&&c.scrollWidth<=c.clientWidth+1;}));
   assert.deepEqual(line,[true,true,true,true,true],'#1724 A1: every host chip fits');
   assert.deepEqual(await summary.locator('.host-chip').evaluateAll(cs=>cs.flatMap(c=>{
    const r=c.getBoundingClientRect();
    return [...c.children].filter(e=>{const b=e.getBoundingClientRect();return b.left<r.left-1||b.right>r.right+1||b.top<r.top-1||b.bottom>r.bottom+1||e.scrollWidth>e.clientWidth+1;}).map(e=>e.textContent);
   })),[],'#1724 A1: names, runtime marks/counts and freshness fit inside their chip');
   assert.equal(await summary.locator('.host-chip [data-runtime]').count(),0,'A1: runtimes appear only in overall totals');
   const real=['hermes-fra1','mac','win','pilot'].map(name=>({name,updated:990,live:true}));
   await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme,projects:projects.map(p=>({...p,hosts:real}))});
   assert.deepEqual(await summary.locator('.host-chip').evaluateAll(cs=>cs.map(c=>c.innerText.replace(/\s+/g,' ').trim())),['her 0','mac 1','pil 1','win 1'],'#1729 A2: the owner hosts read her, mac, win, pil');
   await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme});
   const attention=summary.locator('.attention');
   assert.equal(await attention.getAttribute('aria-label'),'Attention · 1');
   const a=await attention.boundingBox(),s=await summary.boundingBox();
   assert(a&&a.width>=20&&a.x+a.width<=s.x+s.width+1,'A2: Attention is visible inside the summary');
   assert.deepEqual(await summary.locator('.host-chip').evaluateAll((cs,left)=>cs.filter(c=>c.getBoundingClientRect().right>left).map(c=>c.getAttribute('aria-label')),a.x),[],'A2: no host chip runs under Attention');
   assert.equal(await page.locator('.dashboard-status .running-agents').isVisible(),false,'A1: resources stay behind disclosure');
   if(process.env.PANEL_EVIDENCE&&width===320&&font===20&&theme==='dark')await page.screenshot({path:process.env.PANEL_EVIDENCE+'/hosts-collapsed.png'});
   await summary.click();
   assert.equal(await page.locator('.host-breakdown').count(),6,'A2: first expansion includes execution-only hosts');
   const axes=await page.locator('.host-breakdown').evaluateAll(es=>es.map(e=>[...e.querySelectorAll('.host-number')].map(n=>{const r=n.getBoundingClientRect();return r.x+r.width/2;})));
   assert(axes.every(row=>row.every((x,i)=>Math.abs(x-axes[0][i])<1)),'A2: stable Codex/Claude/Total numeric axes');
   assert.deepEqual(await overflow(page),[],'A2: expanded hosts '+width+'/'+font+'/'+theme);
   if(process.env.PANEL_EVIDENCE&&width===320&&font===20&&theme==='dark')await page.screenshot({path:process.env.PANEL_EVIDENCE+'/hosts-expanded.png'});
   await summary.click();
   if(width===420||![16,18].includes(font)){console.log('PASS UX30-06 host geometry '+width+'/'+font+'/'+theme);continue;}
   assert.deepEqual(await page.locator('.view-picks>button').allInnerTexts(),['All tasks','Completed','Running','Waiting for answer']);
   const picks=await page.locator('.view-picks>button').evaluateAll(bs=>new Set(bs.map(b=>Math.round(b.getBoundingClientRect().top))).size);
   assert.deepEqual(await overflow(page),[],'A2: menu '+width+' px / '+font+' px / '+theme);
   assert(picks>1,'A1: view picks wrap at '+width+' px / '+font+' px');
   assert.equal(await page.locator('.filter-header .tqp-counts button').count(),9,'A1: all eight states and All tasks remain');
   assert(await page.locator('.filter-header .tqp-counts button').evaluateAll(bs=>new Set(bs.map(b=>Math.round(b.getBoundingClientRect().top))).size)>1,'A1: counters wrap');
   assert.deepEqual(await page.locator('.view-picks,.filter-header .tqp-counts').evaluateAll(es=>es.filter(e=>getComputedStyle(e).maskImage!=='none').map(e=>e.className)),[],'A1: no edge mask');
   assert.deepEqual(await page.locator('.viewbar button, .filter-header .tqp-counts button').evaluateAll(bs=>bs.filter(b=>{
    const r=b.getBoundingClientRect();let parent=b.parentElement;while(getComputedStyle(parent).display==='contents')parent=parent.parentElement;const p=parent.getBoundingClientRect();
    return b.scrollWidth>b.clientWidth+1||r.left<p.left||r.right>p.right||r.bottom>p.bottom;
   }).map(b=>b.innerText)),[],'A1: each chip is fully visible');
   assert.equal(await page.locator('.dashboard-status').evaluate(e=>parseFloat(getComputedStyle(e.closest('.menu')).fontSize)),font,'A1: text is not shrunk');
   await page.locator('[data-a=task-detail]').first().click();
   assert.deepEqual(await overflow(page),[],'A2: menu task details');
   await page.keyboard.press('Escape');
   await page.locator('[data-a=view-select][data-id=completed]').click();
   const requests=await page.evaluate(()=>messages.filter(m=>m.a==='completed'));
   for(const request of requests)await page.evaluate(r=>taskq.receive({completed:{...r,more:false,tasks:[{n:99,title:'Invented completed task',url:'https://example.org/issues/99',closed:950,outcome:'success'}]}}),request);
   assert.equal(await page.locator('[data-completed]').count(),8,'#1724 A4: Completed remains populated beside long host names');
   assert.equal(await summary.locator('.host-chip').count(),5,'#1724 A2: Completed keeps every host');
   assert.deepEqual(await overflow(page),[],'#1724 A4: Completed has no horizontal overflow');
   if(process.env.PANEL_EVIDENCE&&width===320&&font===20&&theme==='dark')await page.screenshot({path:process.env.PANEL_EVIDENCE+'/completed.png'});
   await page.locator('[data-a=view-select][data-id=all]').click();
   for(const surface of ['views','settings']){
    const window=await browser.newPage({viewport:{width,height:760}});
    window.on('pageerror',e=>{throw e;});
    await window.setContent(html.replace('<head>','<head><script>window.taskqSurface='+JSON.stringify(surface)+';</script>'));
    await window.evaluate(()=>{window.ipc={postMessage:()=>{}};});
    await window.evaluate(s=>taskq.receive(s),{...snapshot,font,theme});
    if(surface==='views'){
     await window.evaluate(d=>taskq.receive({dashboard:d,initial:true}),navigation.dashboard);
     assert.equal(await window.locator('legend').count(),4,'A2: the View window is populated');
    }
    assert.deepEqual(await overflow(window),[],'A2: '+surface+' '+width+' px / '+font+' px / '+theme);
    if(surface==='settings')for(const route of ['appearance','hosts','delivery','cadence','notifications','agents','services','about']){
     await window.evaluate(page=>taskq.receive({settingsOpen:{page}}),route);
     assert.deepEqual(await overflow(window),[],'A2: Settings/'+route+' '+width+' px / '+font+' px / '+theme);
    }
    await window.close();
   }
   console.log('PASS #1709 menu/View/Settings '+width+' px / '+font+' px / '+theme);
  }
  await page.close();
  }
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
