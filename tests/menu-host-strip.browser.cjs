// #1709 A1–A4 supersedes #1701's one-line filters/height budget. Keep whole host chips and Attention;
// catch horizontal scrolling, masks and clipped chips in the menu, View window and Settings.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const states=['doing','review','ask','failed','ready','waiting','later','held'];
const exec=(id,host,runtime)=>({id,attempt:id,host,runtime,role:'worker',state:'running',observed:990,live:true});
const hosts=[{name:'mac',updated:990,live:true},{name:'win',updated:990,live:true},{name:'hermes-fra1',updated:690,live:false,age:310,age_text:'5m'}];
const projects=['taskq','pilotq','agentq','dashq','coreq','serverq','specq','csgo'].map((name,i)=>({root:'/fixture/'+name,name,host:'mac',issues:'https://example.org/'+name+'/issues',board_kind:'github',alive:true,complete:true,age:1,tasks_age_seconds:0,tasks_live:true,observed:1000,hosts,focus:['all'],labels:[],
 ...(i===0?{error:'Fixture urgent fault'}:{}),
 tasks:[{n:i+1,state:states[i],title:name+' task '+(i+1),revision:1,...(i===0?{runtime:'claude',host:'mac',local_session:true,executions:[exec(10,'mac','claude')]}:i===1?{runtime:'codex',host:'win',executions:[exec(11,'win','codex')]}:{})}]}));
const navigation={schema:1,current:{page:'home'},dashboard:{selected:'all',views:[{id:'view-running',name:'Running',pinned:true,working:{states:['doing','review']}},{id:'view-answers',name:'Waiting for answer',pinned:true,working:{states:['ask']}}]}};
const snapshot={display:require('./fixtures/display-model.json').display,platform:'macos',font:18,theme:'dark',host:'mac',now:1000,version:'fixture',projects,notify:{host:{ask:'on',close:'on',failed:'on',gone:'on'},projects:{}},stop_modes:true,navigation,period:'1d',history:require('./fixtures/icon-history/day-dark.json')};
const html=fs.readFileSync('build/web/tray.html','utf8');
const overflow=page=>page.evaluate(()=>[...document.querySelectorAll('*')].filter(e=>e.checkVisibility()).flatMap(e=>{
 const style=getComputedStyle(e);
 return e.scrollWidth>e.clientWidth||/^(auto|scroll)$/.test(style.overflowX)?[{element:e.tagName+'.'+e.className,width:e.clientWidth,scroll:e.scrollWidth,overflow:style.overflowX}]:[];
}));
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const width of [320,360])for(const font of [18,16]){
  const page=await browser.newPage({viewport:{width,height:760}});
  page.on('pageerror',e=>{throw e;});
  await page.setContent(html);
  await page.evaluate(()=>{window.ipc={postMessage:()=>{}};});
  for(const theme of ['dark','light']){
   await page.evaluate(s=>taskq.receive(s),{...snapshot,font,theme});
   await page.evaluate(()=>document.fonts.ready);
   if(process.env.PANEL_EVIDENCE&&width===320&&font===18&&theme==='dark'){
    fs.mkdirSync(process.env.PANEL_EVIDENCE,{recursive:true});
    await page.screenshot({path:process.env.PANEL_EVIDENCE+'/filter-header.png'});
   }
   const summary=page.locator('.dashboard-status>summary');
   assert.deepEqual(await summary.locator('.host-chip').evaluateAll(cs=>cs.map(c=>c.getAttribute('aria-label'))),['hermes-fra1 · runs unknown · 5m old','mac · Claude 1 · Codex 0','win · Claude 0 · Codex 1'],'A1: every named host, stale stays explicit');
   assert.match(await summary.locator('.host-chip.unknown').innerText(),/\?[\s\S]*\?[\s\S]*stale 5m/,'A1: a stale host shows ? counts and its age');
   const line=await summary.locator('.host-chip').evaluateAll(cs=>cs.map(c=>{const r=c.getClientRects();return r.length===1&&c.scrollWidth<=c.clientWidth+1;}));
   assert.deepEqual(line,[true,true,true],'A1: each host chip stays on one line');
   assert.equal(await summary.locator('.host-chip [data-icon=claude], .host-chip svg').count()>=6,true,'A1: Claude/Codex marks');
   const attention=summary.locator('.attention');
   assert.equal(await attention.getAttribute('aria-label'),'Attention · 1');
   const a=await attention.boundingBox(),s=await summary.boundingBox();
   assert(a&&a.width>=20&&a.x+a.width<=s.x+s.width+1,'A2: Attention is visible inside the summary');
   assert.deepEqual(await summary.locator('.host-chip').evaluateAll((cs,left)=>cs.filter(c=>c.getBoundingClientRect().right>left).map(c=>c.getAttribute('aria-label')),a.x),[],'A2: no host chip runs under Attention');
   assert.equal(await page.locator('.dashboard-status .running-agents').isVisible(),false,'A1: resources stay behind disclosure');
   assert.deepEqual(await page.locator('.view-picks>button').allInnerTexts(),['All tasks','Completed','Running','Waiting for answer']);
   const picks=await page.locator('.view-picks>button').evaluateAll(bs=>new Set(bs.map(b=>Math.round(b.getBoundingClientRect().top))).size);
   assert.deepEqual(await overflow(page),[],'A2: menu '+width+' px / '+font+' px / '+theme);
   assert(picks>1,'A1: view picks wrap at '+width+' px / '+font+' px');
   assert.equal(await page.locator('.filter-header .tqp-counts button').count(),9,'A1: all eight states and All tasks remain');
   assert(await page.locator('.filter-header .tqp-counts button').evaluateAll(bs=>new Set(bs.map(b=>Math.round(b.getBoundingClientRect().top))).size)>1,'A1: counters wrap');
   assert.deepEqual(await page.locator('.view-picks,.filter-header .tqp-counts').evaluateAll(es=>es.filter(e=>getComputedStyle(e).maskImage!=='none').map(e=>e.className)),[],'A1: no edge mask');
   assert.deepEqual(await page.locator('.viewbar button, .filter-header .tqp-counts button').evaluateAll(bs=>bs.filter(b=>{
    const r=b.getBoundingClientRect(),p=b.parentElement.getBoundingClientRect();
    return b.scrollWidth>b.clientWidth||r.left<p.left||r.right>p.right||r.bottom>p.bottom;
   }).map(b=>b.innerText)),[],'A1: each chip is fully visible');
   assert.equal(await page.locator('.dashboard-status').evaluate(e=>parseFloat(getComputedStyle(e.closest('.menu')).fontSize)),font,'A1: text is not shrunk');
   await page.locator('[data-a=task-detail]').first().click();
   assert.deepEqual(await overflow(page),[],'A2: menu task details');
   await page.keyboard.press('Escape');
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
