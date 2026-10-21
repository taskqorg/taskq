// M3/M13: a separate scroll surface keeps the header stationary and all window controls reachable.
// Chromium instrument; native bounce, app switching and placement need separate macOS evidence.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const colorScheme of ['dark','light']){
   const page=await browser.newPage({viewport:{width:320,height:400},colorScheme});
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   const state={page:'home',font:16,size:[320,400],size_anchor:'fixed',pinned:false,top:true,host:'fixture',projects:Array.from({length:8},(_,i)=>({root:'p'+i,name:'Project '+i,alive:true,complete:true,focus:[],labels:[],tasks:[]})),ai:{target:'p0',history:[],draft:''}};
   await page.evaluate(s=>taskq.receive(s),state);
   const header=page.locator('.head'),content=page.locator('.content');
   const box=await header.boundingBox();assert(box);
   assert.equal(await content.evaluate(e=>e.scrollHeight>e.clientHeight),true);
   for(const y of [100,100000,-100]){
    await content.evaluate((e,y)=>e.scrollTop=y,y);
    assert.deepEqual(await header.boundingBox(),box,'header moved during scroll');
   }
   assert.equal(await page.locator('[data-route=home]').count(),1,'bottom Home remains');
   await page.locator('.shell-overflow > summary').click();
   for(const [selector,action] of [['[data-a=pin]','pin'],['[data-a=top]','top'],['[data-a=size-anchor]','size-anchor'],['[data-a=hide]','hide']]){
    const button=page.locator(selector);assert(await button.getAttribute('aria-label'));assert(await button.getAttribute('title'));
    await button.click();assert(await page.evaluate(a=>messages.some(m=>m.a===a),action));
   }
   for(const [mode,label] of [['fixed','Fixed size'],['bottom','Keep bottom edge'],['top','Keep top edge']]){
    await page.evaluate(s=>taskq.receive(s),{...state,size_anchor:mode});
    assert.equal(await page.locator('[data-a=size-anchor]').getAttribute('title'),label);
   }
   await page.setViewportSize({width:320,height:800});
   if(!await page.locator('.shell-overflow').evaluate(e=>e.open))await page.locator('.shell-overflow > summary').click();
   await page.locator('[data-route=settings]').click();
   assert(await page.evaluate(()=>messages.filter(m=>m.a==='size').at(-1).h<800),'content-fit cannot shrink after navigating to shorter content');
   await page.close();
  }
  console.log('PASS M3/M13 stationary header, unique navigation, named controls and size-mode presentation in both themes');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
