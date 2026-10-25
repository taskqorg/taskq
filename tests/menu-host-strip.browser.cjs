// #1701 S09: three hosts (one stale), eight projects, 18 px text at the 320 px menu width: one unbroken chip per host,
// Attention visible beside the strip, view and counter rows one line each, header plus filters within a fixed height.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const states=['doing','review','ask','failed','ready','waiting','later','held'];
const exec=(id,host,runtime)=>({id,attempt:id,host,runtime,role:'worker',state:'running',observed:990,live:true});
const hosts=[{name:'mac',updated:990,live:true},{name:'win',updated:990,live:true},{name:'hermes-fra1',updated:690,live:false,age:310,age_text:'5m'}];
const projects=['taskq','pilotq','agentq','dashq','coreq','serverq','specq','csgo'].map((name,i)=>({root:'/fixture/'+name,name,host:'mac',issues:'https://example.org/'+name+'/issues',board_kind:'github',alive:true,complete:true,age:1,tasks_age_seconds:0,tasks_live:true,observed:1000,hosts,focus:['all'],labels:[],
 ...(i===0?{error:'Fixture urgent fault'}:{}),
 tasks:[{n:i+1,state:states[i],title:name+' task '+(i+1),revision:1,...(i===0?{runtime:'claude',host:'mac',local_session:true,executions:[exec(10,'mac','claude')]}:i===1?{runtime:'codex',host:'win',executions:[exec(11,'win','codex')]}:{})}]}));
const navigation={schema:1,current:{page:'home'},dashboard:{selected:'all',views:[{id:'view-running',name:'Running',pinned:true,working:{states:['doing','review']}}]}};
const snapshot={display:require('./fixtures/display-model.json').display,platform:'macos',font:18,theme:'dark',host:'mac',now:1000,version:'fixture',projects,notify:{projects:{}},stop_modes:true,navigation};
const BUDGET=260; // px from the strip top to the counter row bottom at 18 px text
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:320,height:760}});
  page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.ipc={postMessage:()=>{}};});
  for(const theme of ['dark','light']){
   await page.evaluate(s=>taskq.receive(s),{...snapshot,theme});
   await page.evaluate(()=>document.fonts.ready);
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
   assert.deepEqual(await page.locator('.view-picks>button').allInnerTexts(),['All tasks','Completed','Running']);
   const picks=await page.locator('.view-picks>button').evaluateAll(bs=>new Set(bs.map(b=>Math.round(b.getBoundingClientRect().top))).size);
   assert.equal(picks,1,'A2: Running never wraps onto its own row');
   assert.equal(await page.locator('.filter-header .tqp-counts button').evaluateAll(bs=>new Set(bs.map(b=>Math.round(b.getBoundingClientRect().top))).size),1,'A2: counters stay one row');
   assert.deepEqual(await page.locator('.viewbar button, .filter-header .tqp-counts button').evaluateAll(bs=>bs.filter(b=>b.scrollWidth>b.clientWidth+1).map(b=>b.innerText)),[],'A2: no one-row button is squeezed below its text');
   assert.equal(await page.locator('.dashboard-status').evaluate(e=>parseFloat(getComputedStyle(e.closest('.menu')||document.body).fontSize)),18,'A2: text is not shrunk');
   const top=s.y,bottom=(await page.locator('.filter-header').boundingBox()).y+(await page.locator('.filter-header').boundingBox()).height;
   assert(bottom-top<=BUDGET,'A2: header and filter rows take '+Math.round(bottom-top)+' px, budget '+BUDGET);
   assert.equal(await page.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth),true,'no page-wide horizontal scroll');
   if(process.env.PANEL_EVIDENCE){fs.mkdirSync(process.env.PANEL_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.PANEL_EVIDENCE+'/host-strip-'+theme+'.png'});}
  }
  console.log('PASS #1701 host strip: three hosts, one stale, eight projects, Attention visible, one-line view and counter rows at 18 px, 320 px');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
