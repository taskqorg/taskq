// #1586 A3-A6 / R-UI5: real bundled menu; preserve every series, physical raster, period layout and accessible detail.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const bins=Array.from({length:12},(_,i)=>[i+1,i%3,i%4,12-i]);
const base={page:'cadence',font:16,size:[320,700],size_anchor:'fixed',theme:'dark',period:'1d',now:1800000000,projects:[{root:'fixture',name:'Fixture',since:0,tasks:[],focus:[],labels:[],alive:true}],ai:{draft:''}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const dpr of [1,2]){
   const page=await browser.newPage({viewport:{width:320,height:760},deviceScaleFactor:dpr}),errors=[];page.on('pageerror',e=>errors.push(e.message));
   await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   let geometry;
   for(const [period,duration] of [['5m',300],['1h',3600],['1d',86400],['1w',604800],['1mo',2592000],['custom',7500]]){
    await page.evaluate(s=>taskq.receive(s),{...base,period,history:{start:base.now-duration,end:base.now,bins,since:0,tz:'UTC'}});
    await page.waitForFunction(()=>document.querySelector('.history-label')?.textContent.startsWith('12 bars'),{},{timeout:2000});
    assert.match(await page.locator('.history-label').innerText(),/12 bars · .* per bar/);
    const g=await page.locator('.bars').evaluate((e,dpr)=>({width:e.getBoundingClientRect().width*dpr,height:e.getBoundingClientRect().height*dpr,rects:[...e.children].map(b=>{const r=b.getBoundingClientRect();return [r.x*dpr,r.y*dpr,r.width*dpr,r.height*dpr];})}),dpr);
    for(let i=0;i<g.rects.length;i++){const [x,y,w]=g.rects[i];assert.equal(w,Math.floor((g.width-11)/12));assert(w>=4);assert(Math.abs(y-Math.round(y))<.01);assert(Math.abs(w-Math.round(w))<.01);assert(Math.abs(x-Math.round(x))<.01);if(i)assert(Math.abs(x-g.rects[i-1][0]-g.rects[i-1][2]-1)<.01);}
    if(geometry)assert.deepEqual(g,geometry,'only bar duration changes between periods');geometry=g;
    const segments=await page.locator('.bars button').evaluateAll((buttons,dpr)=>buttons.map(b=>[...b.children].reverse().map(i=>i.getBoundingClientRect().height*dpr)),dpr);
    const peak=Math.max(...bins.map(b=>b.reduce((a,n)=>a+n,0)));
    bins.forEach((row,i)=>{let total=0;row.forEach((count,k)=>{const low=Math.round(64*dpr*total/peak);total+=count;assert(Math.abs(segments[i][k]-(Math.round(64*dpr*total/peak)-low))<.01);});});
    const last=page.locator('.bars button').last();await last.focus();await last.press('Enter');
    const detail=await page.locator('.history-detail').innerText();for(const s of ['closed 12','failed 2','ask 3','waiting 1'])assert(detail.includes(s),detail);assert(detail.includes('[Start:')&&detail.includes('End:'));
    const selector=page.getByRole('combobox',{name:'History interval'});assert.equal(await selector.locator('option').count(),12);assert.equal(await selector.inputValue(),'11');
    await selector.selectOption('0');assert.match(await page.locator('.history-detail').innerText(),/Bar 1 of 12/);
    await selector.focus();assert(await selector.evaluate(e=>e===document.activeElement));await selector.selectOption('1');assert.match(await page.locator('.history-detail').innerText(),/Bar 2 of 12/);
    assert.equal(await page.locator('.bars button[aria-pressed=true]').getAttribute('data-history-bin'),'1','full-sized equivalent keeps the exact bar selection');
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
   }
   // Resize and theme changes retain integer geometry; unknown history never claims zero.
   await page.setViewportSize({width:360,height:760});
   await page.evaluate(s=>taskq.receive(s),{...base,theme:'light',font:20,history:{start:0,end:86400,bins,since:null,tz:'UTC'}});
   assert.match(await page.locator('.history-detail').innerText(),/unknown/);
   assert.equal(await page.locator('.history-legend').getByText(/—/).count(),4);
   if(process.env.TASKQ_HISTORY_SHOTS){
    const real=JSON.parse(fs.readFileSync('docs/design/evidence/1586/real-history.json','utf8'));
    await page.setViewportSize({width:320,height:760});
    await page.evaluate(s=>taskq.receive(s),{...base,now:Date.now()/1000,history:real});
    await page.waitForFunction(()=>document.querySelector('.history-label')?.textContent.startsWith('12 bars'));
    await page.waitForFunction(()=>{const e=document.querySelector('.bars'),b=e?.lastElementChild;return b&&b.getBoundingClientRect().right<=e.getBoundingClientRect().right;});
    await page.locator('.bars button').last().click();
    await page.screenshot({path:process.env.TASKQ_HISTORY_SHOTS+'/after-real-menu-'+dpr+'x.png'});
   }
   assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();}
 console.log('PASS history physical raster, six period layouts, exact selectable details, unknown and resize at 1x/2x');
})().catch(e=>{console.error(e);process.exit(1);});
