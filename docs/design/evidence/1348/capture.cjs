// Research instrument, not a regression gate. Run from the repository root after npm run build.
const fs=require('node:fs'),{chromium}=require('playwright');
const out='docs/design/evidence/1348';
const tasks=Array.from({length:12},(_,i)=>({n:100+i,state:i%2?'ready':'doing',title:'Review queued work and preserve the complete task title across menu navigation',revision:1}));
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture-host',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,version:'fixture',focus:['all'],labels:[],tasks,closed:null,cap_saved:{error:'Fixture: capacity unavailable'}};
const base={platform:'windows',font:16,theme:'dark',host:'fixture-host',now:1000,version:'source audit',projects:[project],ai:{draft:''},notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('../../../../tests/browser.cjs')());
 const observations={source:require('node:child_process').execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),renderer:await browser.version(),fixture:'invented; no native bridge or board writes',screens:[]};
 try{
  const page=await browser.newPage({viewport:{width:320,height:600},colorScheme:'dark'});
  page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.ipc={postMessage:()=>{}};});
  async function shot(name){
   await page.screenshot({path:out+'/'+name+'.png'});
   observations.screens.push({name,...await page.evaluate(()=>({
    controls:[...document.querySelectorAll('.head button,[data-a=back],.grip')].map(e=>{const r=e.getBoundingClientRect();return {label:e.getAttribute('aria-label')||e.textContent,x:r.x,y:r.y,width:r.width,height:r.height};}),
    scroll:{top:document.querySelector('.content').scrollTop,height:document.querySelector('.content').clientHeight,total:document.querySelector('.content').scrollHeight},
    boardLinks:[...document.querySelectorAll('[data-url]')].filter(e=>e.dataset.url==='https://example.org/alpha/issues').length
   }))});
  }
  await page.evaluate(s=>taskq.receive(s),base);await shot('home-dark-16');
  await page.evaluate(s=>taskq.receive({...s,page:'/fixture/alpha'}),base);await shot('project-dark-16');
  await page.locator('.content').evaluate(e=>e.scrollTop=e.scrollHeight);await shot('project-scrolled-dark-16');
  await page.evaluate(s=>taskq.receive({...s,page:'settings'}),base);await shot('settings-dark-16');
  await page.evaluate(s=>taskq.receive({...s,page:'/fixture/alpha',theme:'light',font:20}),base);
  await page.emulateMedia({colorScheme:'light'});await page.setViewportSize({width:360,height:600});await shot('project-light-20');
  const first=page.locator('.tqp-counts button[data-state=doing]').first();
  await first.click();await shot('filter-light-20');
  observations.filter={checkboxes:await page.locator('input[type=checkbox],[role=checkbox]').count(),heading:await page.locator('h1').innerText()};
  fs.writeFileSync(out+'/observations.json',JSON.stringify(observations,null,2)+'\n');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
