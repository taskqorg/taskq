// #1881 A3: four compact browser screens; invented board data, no native or live-board effects.
// Run from the repository root after building. TASKQ_BEFORE_HTML names the saved base build/web/tray.html.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],tasks:[
 {n:1,state:'doing',title:'Keep active work visible',runtime:'codex',host:'fixture',local_session:true,session_url:'codex://threads/fixture',age:'5m'},
 {n:2,state:'review',title:'Review the exact candidate',runtime:'claude',host:'remote',local_session:false,session_url:'https://claude.ai/code/session_fixture',age:'2m'},
 {n:3,state:'ask',title:'Choose the release window',runtime:'codex',age:'1m',question:{parsed:true,problem:'Which window?',choices:[{id:'1',text:'Next window',recommended:true}]}},
 {n:4,state:'failed',title:'Inspect the failed check',age:'3m'},
 {n:5,state:'ready',title:'Wait for the next available slot',age:'<1m'}
].map((t,i)=>({...t,revision:10+i,since:900,rank:i,controls:[]}))};
const state={display:require('../../../../tests/fixtures/display-model.json').display,platform:'windows',font:16,host:'fixture',now:1000,version:'fixture',page:'home',projects:[project],ai:{draft:''},notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('../../../../tests/browser.cjs')()),observations=[];
 try{for(const version of ['before','after'])for(const theme of ['light','dark']){
  const page=await browser.newPage({viewport:{width:360,height:720},colorScheme:theme});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync(version==='before'?process.env.TASKQ_BEFORE_HTML:'build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  await page.evaluate(s=>taskq.receive(s),{...state,theme});
  const heads=await page.locator('.tqp-task-head').evaluateAll(es=>es.map(e=>({first:e.firstElementChild.className,order:[...e.querySelectorAll('[data-icon],.tqp-age')].map(x=>x.dataset.icon||x.textContent),left:e.firstElementChild.getBoundingClientRect().left,identity:e.querySelector('.task-identity').getBoundingClientRect().left})));
  assert.equal(heads.length,5);assert.deepEqual(errors,[]);
  assert(heads.every(h=>version==='before'?h.first==='task-identity':h.first.includes('task-stage')&&h.left<h.identity));
  assert(await page.locator('.content').evaluate(e=>e.scrollWidth<=e.clientWidth),'no horizontal overflow');
  await page.screenshot({path:path.join(__dirname,version+'-'+theme+'-compact.png')});
  await page.locator('.question-shortcut').click();
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='workspace').at(-1).open),{page:'answer',operation:null,root:project.root,board:project.issues,task:3,revision:12});
  observations.push({version,theme,heads,question:'exact root/board/task/revision preserved'});await page.close();
 }}finally{await browser.close();}
 fs.writeFileSync(path.join(__dirname,'observations.json'),JSON.stringify(observations,null,2)+'\n');
 console.log('PASS #1881: four before/after light/dark compact screens, state order and exact question target');
})().catch(e=>{console.error(e);process.exitCode=1;});
