// M3.26: actual bundle/Preact -> one native fit per refresh, across restored menu views.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const project={root:'fixture-alpha',name:'Alpha',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:0,tasks_age_seconds:0,poll:60,focus:['all'],tasks:[{n:1,state:'doing',revision:1,title:'Invented stable task'}]};
const base={display:require('./fixtures/display-model.json').display,platform:'macos',font:16,theme:'dark',now:1000,version:'fixture',projects:[project],notify:{projects:{}},ai:{draft:''}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 for(const selected of ['all','completed','view-running']){
  const page=await browser.newPage({viewport:{width:360,height:720}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:text=>messages.push(JSON.parse(text))};});
  const state={...base,navigation:{schema:1,current:{page:'home'},dashboard:{selected,views:[{id:'view-running',name:'Running',pinned:true,working:{states:['doing','review']}}]}}};
  await page.evaluate(s=>taskq.receive({updates:[s,{settingsWindow:true,viewsWindow:true}]}),state);
  assert.equal(await page.locator('.head h1').innerText(),'Tasks');
  await page.evaluate(()=>{messages=[];});
  const partial={...state,projects:[{...project,complete:false,error:'Invented intermediate read',tasks_age_seconds:121}]};
  await page.evaluate(({partial,state})=>taskq.receive({updates:[partial,state,{viewStateError:null}]}),{partial,state});
  assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='size').length),1,'one settled DOM fit, never intermediate native heights');
  assert.doesNotMatch(await page.locator('.menu').innerText(),/Invented intermediate read|Data from /);
  assert.equal(await page.locator('[data-card]').count(),selected==='completed'?0:1);
  if(process.env.REFRESH_EVIDENCE){fs.mkdirSync(process.env.REFRESH_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.REFRESH_EVIDENCE+'/'+selected+'.png'});}
  await page.evaluate(s=>taskq.receive({updates:[{...s,projects:[{...s.projects[0],complete:false,error:'Invented persistent failure',tasks_age_seconds:121}]}]}),state);
  assert.match(await page.locator('.stale').innerText(),/Data from /);
  assert.deepEqual(errors,[]);await page.close();
 }
 console.log('PASS M3.26 one fit across All, Completed and Running; persistent unknown remains visible');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
