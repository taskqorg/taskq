// M13/O1: fixed header/footer, reachable Close/Pin, direct growth modes, shrink-to-content and keyboard resize.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{for(const theme of ['dark','light']){
  const page=await browser.newPage({viewport:{width:320,height:400}});page.on('pageerror',e=>{throw e;});
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  const state={display:require('./fixtures/display-model.json').display,page:'home',theme,font:16,size:[320,400],size_anchor:'fixed',pinned:false,top:true,host:'fixture',now:1000,projects:[{root:'p',name:'Project',issues:'https://example.org/p/issues',alive:true,complete:true,focus:[],tasks:Array.from({length:15},(_,i)=>({n:i+1,state:'ready',title:'Long wrapped task title '+i}))}],ai:{draft:''}};
  await page.evaluate(s=>taskq.receive(s),state);
  const header=page.locator('.head'),footer=page.locator('.status-footer'),content=page.locator('.content'),box=await header.boundingBox(),bottom=await footer.boundingBox();
  assert(await content.evaluate(e=>e.scrollHeight>e.clientHeight));for(const y of [100,100000,-100]){await content.evaluate((e,y)=>e.scrollTop=y,y);assert.deepEqual(await header.boundingBox(),box);assert.deepEqual(await footer.boundingBox(),bottom);}
  assert.equal(await page.locator('[data-route=home]').count(),0,'no duplicated bottom navigation');
  for(const [selector,action] of [['[data-a=pin]','pin'],['[data-a=hide]','hide']]){const b=page.locator(selector);assert(await b.getAttribute('aria-label'));assert(await b.getAttribute('title'));await b.click();assert(await page.evaluate(a=>messages.some(m=>m.a===a),action));}
  await page.locator('[data-route=settings]').click();await page.locator('[data-route=appearance]').click();
  for(const mode of ['auto','top','bottom','fixed']){const b=page.locator('[data-a=size-anchor][data-value='+mode+']');await b.click();assert(await page.evaluate(value=>messages.some(m=>m.a==='size-anchor'&&m.value===value),mode));}
  await page.locator('.content [data-a=top]').click();assert(await page.evaluate(()=>messages.some(m=>m.a==='top')));
  await page.locator('[data-resize]').focus();await page.keyboard.press('ArrowDown');assert(await page.evaluate(()=>messages.some(m=>m.a==='resize')&&messages.some(m=>m.a==='resize-end')));
  await page.setViewportSize({width:320,height:800});await page.evaluate(s=>taskq.receive(s),{...state,page:'settings',size_anchor:'top'});assert(await page.evaluate(()=>messages.filter(m=>m.a==='size').at(-1).h<800),'content-fit shrinks on shorter routes');
  await page.close();
 }console.log('PASS M13 stationary chrome, Close/Pin/Top, direct Auto/Down/Up/Fixed, keyboard resize and content fit in both themes');}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
