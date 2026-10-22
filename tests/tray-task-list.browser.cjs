// #1523 A3/A2/A6/A5: Tasks list rows follow chip order then priority then number; actions live in identity details;
// chips center their icon or icon+count group; content fit keeps the last row and footer visible. Invented data.
const assert=require('node:assert/strict'),fs=require('node:fs'),{chromium}=require('playwright');
const out=process.env.TASK_LIST_EVIDENCE;
const t=(n,state,labels=[])=>({n,state,title:'Task '+n,revision:n,labels,controls:state==='doing'?['stop']:state==='failed'?['retry','later']:state==='ready'?['later']:[]});
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],cap_saved:{error:'Fixture'},
  tasks:[t(9,'ready'),t(3,'later'),t(7,'ready',['p0']),t(5,'failed'),t(2,'ready',['p1']),t(8,'doing'),t(4,'ask'),t(6,'review'),t(11,'held'),...Array.from({length:12},(_,i)=>t(100+i,'later'))]};
const want=[8,6,4,5,7,2,9,3,100,101,102,103,104,105,106,107,108,109,110,111,11];
const snapshot={platform:'macos',font:16,theme:'light',host:'fixture',now:1000,version:'fixture',projects:[project],ai:{draft:''},notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const platform of ['macos','windows'])for(const theme of ['light','dark'])for(const font of [16,20])for(const anchor of ['top','bottom']){
   const page=await browser.newPage({viewport:{width:360,height:900},colorScheme:theme});
   page.on('pageerror',e=>{throw e;});
   await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   const state={...snapshot,platform,theme,font,size_anchor:anchor,page:'/fixture/alpha'};
   await page.evaluate(s=>taskq.receive(s),state);
   await page.locator('.tqp-counts [data-state="doing"]').first().click();await page.locator('[data-a="all-tasks"]').first().click();
   // A3: chip order, then p0/p1/none, then number.
   assert.deepEqual(await page.locator('[data-card]').evaluateAll(es=>es.map(e=>+e.querySelector('[data-n],[data-task]')?.dataset.n||+(e.textContent.match(/#(\d+)/)||[])[1])),want,'rows follow chip order, priority, number');
   // A2: no repeated action buttons; the row menu holds them and returns focus on Escape.
   assert.equal(await page.locator('[data-card] [data-a="control"],[data-card] [data-a="start"],[data-card] [data-page="answer"]').count(),0,'rows carry no action buttons');
   const menu=page.locator('[data-a="task-detail"][aria-label="Details for Alpha #5"]').first();
   assert.equal(await menu.getAttribute('aria-expanded'),'false');
   await menu.focus();await page.keyboard.press('Enter');
   assert.equal(await menu.getAttribute('aria-expanded'),'true');
   const actions=page.locator('#taskq-popover');
   assert.deepEqual(await actions.locator('button').evaluateAll(b=>b.map(x=>x.textContent.trim())).then(x=>x.length>0),true,'failed row menu offers its controls');
   if(out&&font===16&&anchor==='top'&&(platform==='macos'||theme==='light'))await page.screenshot({path:out+'/'+(platform==='macos'?'mac':'windows')+'-'+theme+'-row-menu.png'});
   await page.keyboard.press('Escape');
   assert.equal(await actions.count(),0);assert.equal(await menu.evaluate(e=>e===document.activeElement),true,'Escape returns focus to the menu button');
   // A6: chip icon alone (Blocked 0) and icon+count group (Later 13) and icon+count group are centered by bounding box.
   for(const chip of await page.locator('.filter-header .tqp-counts [data-state]').all()){
    const g=await chip.evaluate(b=>{const r=b.getBoundingClientRect(),kids=[...b.children].map(c=>c.getBoundingClientRect()),l=Math.min(...kids.map(k=>k.left)),rr=Math.max(...kids.map(k=>k.right));return {left:l-r.left,right:r.right-rr,top:kids[0].top-r.top,bottom:r.bottom-kids[0].bottom,text:b.textContent};});
    assert(Math.abs(g.left-g.right)<=1,'chip group centered horizontally '+JSON.stringify(g));
    assert(Math.abs(g.top-g.bottom)<=1,'chip icon centered vertically '+JSON.stringify(g));
   }
   // A5: fit height is the measured layout; with and without an added panel the last row and footer stay visible, no gap.
   for(const extra of [false,true]){
    await page.setViewportSize({width:360,height:900});
    await page.evaluate(s=>{messages.length=0;taskq.receive(s);},extra?{...state,page:undefined,projects:[{...project,complete:false,tasks:[...project.tasks,t(12,'held')]}]}:{...state,page:undefined});
    if(extra)assert.match(await page.locator('.content').innerText(),/Partial: refresh[\s\S]*Start 2 held/,'added panels render');
    const h=await page.evaluate(()=>messages.filter(m=>m.a==='size').at(-1).h);
    await page.setViewportSize({width:360,height:h});
    const fit=await page.evaluate(()=>{const c=document.querySelector('.content'),rows=document.querySelectorAll('[data-card]'),last=rows[rows.length-1].getBoundingClientRect(),grip=document.querySelector('.menu .grip').getBoundingClientRect(),menu=document.querySelector('.menu').getBoundingClientRect();return {scroll:c.scrollHeight-c.clientHeight,last:last.bottom,content:c.getBoundingClientRect().bottom,grip:grip.bottom,menu:menu.bottom,vh:innerHeight};});
    assert(fit.scroll<=0,'list needs no scroll at fit height '+JSON.stringify(fit));
    assert(fit.last<=fit.content&&fit.grip<=fit.vh+0.5&&fit.vh-fit.menu<=1,'last row and footer visible, no gap '+JSON.stringify({extra,anchor,font,fit}));
    if(out&&platform==='macos'&&theme==='light'&&anchor==='bottom'&&extra)await page.screenshot({path:out+'/mac-'+font+'-fit-panel.png'});
   }
   await page.close();
  }
  console.log('PASS #1523 A2/A3/A5/A6: chip-order rows, identity details with Escape focus return, centered chips, layout fit with an added panel; macOS/Windows snapshots, light/dark 16/20 px, top/bottom anchors');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
