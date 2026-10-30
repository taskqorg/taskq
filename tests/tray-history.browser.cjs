// UX30-20..22 / M6: the canonical event stack and per-series coverage agree on every surface.
// #1598 A4/A6/A8: native-generated SVG -> bundled menu; exact accessible detail and time-identity selection.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fixtures=process.env.TASKQ_ICON_EVIDENCE||'tests/fixtures/icon-history';
const base={display:require('./fixtures/display-model.json').display,page:'cadence',font:16,size:[360,760],size_anchor:'fixed',period:'1d',projects:[{root:'fixture',name:'TaskQ frozen capture',tasks:[],focus:[],labels:[],alive:true}],ai:{draft:''}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const dpr of [1,2]){
   const page=await browser.newPage({viewport:{width:360,height:800},deviceScaleFactor:dpr}),errors=[];page.on('pageerror',e=>errors.push(e.message));
   await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   for(const name of ['day','hour','incomplete','paused','synthetic-zero','synthetic-failed','synthetic-unknown','synthetic-ask','synthetic-stack','synthetic-partial-new','synthetic-rare','synthetic-overflow'])for(const theme of ['light','dark']){
    const H=JSON.parse(fs.readFileSync(path.join(fixtures,name+'-'+theme+'.json')));
    const s={...base,theme,palette:H.palette,now:H.captured,history:H,icons:{full:H.icon_svg||H.svg,icon:H.compact}};
    await page.evaluate(s=>taskq.receive(s),s);
    assert.equal(await page.locator('.hist>summary .meta').evaluate(e=>e.getBoundingClientRect().right<=e.closest('summary').getBoundingClientRect().right),true,'History period stays inside its summary beside the bundled icon');
    assert.equal(await page.locator('[data-view-key=history-source]').evaluate(e=>e.open),false,'R34: exact timestamps and source diagnostics start disclosed');
    const targets=page.locator('[data-history-bin]');await targets.first().waitFor();assert.equal(await targets.count(),6);
    const chart=page.locator('.history-plot #chart');assert.equal(await chart.locator('[data-start]').count(),6);
    assert.equal(await chart.locator('[data-mark=ask]').count(),H.bins.filter(b=>b.counts[2]>0&&!b.missing.length&&(!H.bins.some(b=>!b.incoming_missing.length)||!b.incoming_missing.length)).length,'every observed Ask bin has a mark');
    const askTotal=H.bins.reduce((sum,b)=>sum+b.counts[2],0);
    const askQuantity=H.bins.some(b=>b.missing.length)?(askTotal?'≥'+askTotal:'? (0 observed)'):String(askTotal);
    assert((await page.locator('.history-legend').textContent()).includes('ask '+askQuantity),'legend keeps event totals and lower bounds');
    // DOM paints the original groups, not JS-created rectangles or a resized bitmap.
    assert.equal(await page.locator('.history-plot img, .history-plot canvas').count(),0);
    for(const [i,b] of H.bins.entries()){
     const target=targets.nth(i);await target.focus();await target.press('Enter');
     const detail=await page.locator('.history-exact').textContent();
     for(const [k,j] of [['completed',0],['failed',1],['ask',2],['blocked',3]])assert(detail.includes(k+' '+(b.missing.length?(b.counts[j]?'≥'+b.counts[j]:'? (0 observed)'):b.counts[j])),detail);
     assert(detail.includes('[')&&detail.includes(')')&&detail.includes('UTC')||detail.includes('Asia/'),detail);
     const newQuantity=b.missing.length||b.incoming_missing?.length?(b.incoming?'≥'+b.incoming:'? (0 observed)'):String(b.incoming);
     assert(detail.includes('new '+newQuantity),detail);
     if(b.counts[2]&&!b.missing.length&&(!H.bins.some(b=>!b.incoming_missing.length)||!b.incoming_missing.length)){
      const ask=chart.locator('[data-start]').nth(i).locator('[data-mark=ask]');
      assert.equal(await ask.evaluate(e=>getComputedStyle(e).visibility),'visible','every surface shows quantitative Ask');
      assert.equal(await ask.evaluate(e=>getComputedStyle(e).fill),await page.locator('.history-legend i.ask').evaluate(e=>getComputedStyle(e).backgroundColor),'Ask uses the legend color');
      assert((await ask.boundingBox()).height>0,'Ask has visible geometry');
      assert((await target.getAttribute('aria-label')).includes('ask '+(b.missing.length?'≥':'')+b.counts[2]));
     }
     if(b.ongoing)assert(detail.includes('ongoing'));
     if(b.missing.length)assert(detail.includes('incomplete')&&b.missing.every(source=>detail.includes(source)));
     for(const mark of ['zero','ongoing'])assert.equal(await chart.locator('[data-start]').nth(i).locator('[data-mark='+mark+']').count(),0,'no detached ticks or underlines');
    }
    const selected=H.bins[4].start+'/'+H.bins[4].end;
    assert.equal(await page.locator('[data-history-select]').count(),0,'one interval selector: the six chart bins');
    await page.locator('[data-history-bin]').nth(4).click();
    await page.evaluate(s=>taskq.receive(s),s);
    assert.equal(await page.locator('[data-history-bin][aria-pressed=true]').getAttribute('data-history-bin'),selected);
    if(name==='day'||name==='synthetic-overflow'){
     const originalChart=await chart.innerHTML();
     await page.evaluate(s=>taskq.receive({...s,page:'appearance'}),s);
     assert.equal(await page.getByRole('button',{name:'Custom',exact:true}).count(),0);
     assert.equal(await page.getByRole('button',{name:'Icon',exact:true}).locator('img').count(),1,'compact picker identity remains visible');
     assert.equal(await page.locator('.summary-preview svg #chart').innerHTML(),originalChart);
     for(const mark of await page.locator('.summary-preview [data-mark=ask]').all())assert.equal(await mark.evaluate(e=>getComputedStyle(e).visibility),'visible','picker shares the quantitative Ask stack');
     assert.equal(await page.locator('.summary-preview svg [id^=counter-]').count(),name==='day'?2:3,'frozen Work 1 / Ready 1 must retain both counter glyph groups');
     await page.evaluate(s=>taskq.receive(s),s);
    }
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    if(process.env.TASKQ_HISTORY_SHOTS&&dpr===2&&theme==='light'&&['day','synthetic-stack','synthetic-unknown','synthetic-overflow'].includes(name)){fs.mkdirSync(process.env.TASKQ_HISTORY_SHOTS,{recursive:true});await page.screenshot({path:path.join(process.env.TASKQ_HISTORY_SHOTS,name+'-'+theme+'.png')});}
   }
   // Geometry/reflow matrix: actual Rust overflow composition must keep six bins at every width and text size.
   const dense=JSON.parse(fs.readFileSync(path.join(fixtures,'synthetic-overflow-dark.json')));
   for(const width of [320,360,480])for(const font of [14,16,18,20]){
    await page.setViewportSize({width,height:800});
    await page.evaluate(s=>taskq.receive(s),{...base,font,theme:'dark',palette:dense.palette,now:dense.captured,history:dense,icons:{full:dense.icon_svg||dense.svg,icon:dense.compact}});
    assert.equal(await page.locator('.history-plot [data-start]').count(),6);
    assert.equal(await page.locator('.history-plot svg').getAttribute('viewBox'),'1 10.5 20 11','widened counter canvas cannot break History projection');
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'no horizontal overflow at '+width+'/'+font);
   }
   // #1712 A3: Source names each project read and why a missing one is unknown
   const H=JSON.parse(fs.readFileSync(path.join(fixtures,'hour-dark.json')));
   await page.evaluate(s=>taskq.receive(s),{...base,theme:'dark',palette:H.palette,now:H.captured,history:H,icons:{full:H.icon_svg||H.svg,icon:H.compact},projects:[{...base.projects[0],since:H.bins[0].start},{root:'csgo',name:'csgo',tasks:[],focus:[],labels:[],alive:true,since:null,events_error:'no events file'}]});
   const sources=await page.locator('.history-sources').textContent();
   assert(sources.includes('TaskQ frozen capture events since ')&&sources.includes('csgo unknown: no events file'),sources);
   assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();}
 console.log('PASS six canonical vector bins, exact/incomplete details, keyboard selection, refresh identity, themes/DPR and Custom removal');
})().catch(e=>{console.error(e);process.exit(1)});
