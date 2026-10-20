// Bounded #1349 visual evidence; invented snapshot, no native bridge or board effects.
const fs=require('node:fs'),{chromium}=require('playwright');
const {project}=require('../../../../tests/tray-running.test.cjs');
(async()=>{
 const browser=await chromium.launch(require('../../../../tests/browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:360,height:760}});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>window.ipc={postMessage:()=>{}});
  const fixture={...project,name:'Example project',alive:true,focus:['all'],labels:[],closed:null,cap_saved:{error:'Fixture'}};
  const base={platform:'windows',font:16,host:'mac',now:1000,version:'fixture',projects:[fixture],ai:{draft:''},notify:{projects:{}},stop_modes:true};
  const screens=[];
  for(const theme of ['dark','light']){
   await page.emulateMedia({colorScheme:theme});await page.evaluate(s=>taskq.receive(s),{...base,theme});
   const block=page.locator('.running-agents');await block.scrollIntoViewIfNeeded();
   if(await block.count()!==1)throw Error('Running block missing');
   await block.locator('details').evaluateAll(nodes=>nodes.forEach(e=>e.open=false));
   await block.screenshot({path:`docs/design/evidence/1349/${theme}-compact.png`});
   await block.locator('details').evaluateAll(nodes=>nodes.forEach(e=>e.open=true));
   await block.screenshot({path:`docs/design/evidence/1349/${theme}-expanded.png`});
   await page.screenshot({path:`docs/design/evidence/1349/${theme}-home.png`});
   screens.push({theme,text:await block.innerText(),width:await block.evaluate(e=>e.getBoundingClientRect().width)});
  }
  if(errors.length)throw Error(errors.join('\n'));
  fs.writeFileSync('docs/design/evidence/1349/observations.json',JSON.stringify({renderer:await browser.version(),scope:'Browser fixture, no native or live-board qualification',screens},null,2)+'\n');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
