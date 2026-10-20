// #1390 evidence: the real tray HTML renders one ask card without and with a PM pick (Playwright instrument, not native tray).

const fs=require('node:fs'),{chromium}=require('playwright');
const html=fs.readFileSync('build/web/tray.html','utf8');
const fx=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json','utf8'));
const out=process.argv[2];
(async()=>{
  const browser=await chromium.launch(require('../../../../tests/browser.cjs')());
  const row=fx.cases.find(c=>c.name.startsWith('pm pick')).rows[0];
  for(const [name,r] of [['tray-ask-star-only',{...row,pick:undefined}],['tray-ask-pm-pick',row]]){
    const page=await browser.newPage({viewport:{width:360,height:520},colorScheme:'light'});
    await page.setContent(html);
    await page.evaluate(()=>{window.ipc={postMessage:()=>{}};});
    const board='https://github.com/o/r/issues',root='project';
    await page.evaluate(s=>taskq.receive(s),{page:root,font:16,theme:'light',projects:[{root,name:'project',issues:board,alive:true,complete:true,focus:[],labels:[],tasks:[r],cap_saved:{error:'not configured'}}],ai:{draft:''},notify:{projects:{}}});
    console.log(name, await page.locator('.qac-pick').count());
    await page.locator('.qac').screenshot({path:out+'/'+name+'.png'});
    await page.close();
  }
  await browser.close();
})().catch(e=>{console.error(e);process.exit(1);});
