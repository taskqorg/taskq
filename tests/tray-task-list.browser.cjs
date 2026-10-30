// #1523 A3/A2/A6/A5: Tasks list rows follow chip order then priority then number; actions live in identity details;
// chips center their icon or icon+count group; content fit keeps the last row and footer visible. Invented data.
const assert=require('node:assert/strict'),fs=require('node:fs'),{chromium}=require('playwright');
const out=process.env.TASK_LIST_EVIDENCE;
const t=(n,state,labels=[])=>({n,state,title:'Task '+n,revision:n,labels,controls:state==='doing'?['stop']:state==='failed'?['retry','later']:state==='ready'?['later']:[]});
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],cap_saved:{error:'Fixture'},
  tasks:[t(9,'ready'),t(3,'later'),t(7,'ready',['p0']),t(5,'failed'),t(2,'ready',['p1']),t(8,'doing'),t(4,'ask'),t(6,'review'),t(11,'held'),...Array.from({length:12},(_,i)=>t(100+i,'later'))]};
const want=[8,6,4,5,7,2,9,3,100,101,102,103,104,105,106,107,108,109,110,111,11];
project.tasks.forEach(t=>t.rank=want.indexOf(t.n));
const snapshot={display:require('./fixtures/display-model.json').display,platform:'macos',font:16,theme:'light',host:'fixture',now:1000,version:'fixture',projects:[project],ai:{draft:''},notify:{projects:{}},stop_modes:true};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const platform of ['macos','windows'])for(const theme of ['light','dark'])for(const font of [16,20])for(const anchor of ['top','bottom']){
   const page=await browser.newPage({viewport:{width:360,height:900},colorScheme:theme});
   page.on('pageerror',e=>{throw e;});
   await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   const state={...snapshot,platform,theme,font,size_anchor:anchor,page:'home'};
   await page.evaluate(s=>taskq.receive(s),state);
   await page.locator('.tqp-counts [data-state="doing"]').first().click();await page.locator('[data-a="all-tasks"]').first().click();
   // A3: chip order, then p0/p1/none, then number.
   assert.deepEqual(await page.locator('[data-card]').evaluateAll(es=>es.map(e=>+e.querySelector('[data-n],[data-task]')?.dataset.n||+(e.textContent.match(/#(\d+)/)||[])[1])),want,'rows follow chip order, priority, number');
   // Chain: M3.9 / #1881 A2-A3. Risk: compact rows put state back beside runtime/time instead of first.
   assert(await page.locator('.tqp-task-head').evaluateAll(es=>es.length>0&&es.every(e=>e.firstElementChild.matches('.task-stage')&&e.firstElementChild.getBoundingClientRect().left<=e.querySelector('.task-identity').getBoundingClientRect().left)),'state icon is the first row-head element at the left edge');
   // A2: no repeated action buttons; the row menu holds them and returns focus on Escape.
   assert.equal(await page.locator('[data-card] [data-a="control"],[data-card] [data-a="start"]').count(),0,'rows carry no lifecycle buttons');
   const menu=page.locator('[data-a="task-detail"][aria-label="Details for Alpha #5"]').first();
   assert.equal(await menu.getAttribute('aria-expanded'),'false');
   await menu.focus();await page.keyboard.press('Enter');
   assert.equal(await menu.getAttribute('aria-expanded'),'true');
   const actions=page.locator('.task-inline-details');
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
    await page.evaluate(s=>{messages.length=0;taskq.receive(s);},extra?{...state,page:undefined,projects:[{...project,complete:false,partial:['Alpha'],tasks:[...project.tasks,t(12,'held')]}]}:{...state,page:undefined});
    // #1775 M2: the held shortcut is a green play icon; its label stays the accessible name.
    if(extra){assert.match(await page.locator('.content').innerText(),/Alpha: Project snapshot incomplete or stale/,'named project cause renders (#1840 A2)');assert.equal(await page.getByRole('button',{name:'Start 2 held'}).count(),1,'held shortcut renders');}
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
  // #1740 A1-A5 / M15: a long question must never own scrolling or obscure browsing. Real wheel input,
  // including small trackpad-like deltas, protects the boundary that scrollTop assignments cannot exercise.
  const board=project.issues,key=board+'/4',question={...t(4,'ask'),title:'Task question '+ 'unbroken_identifier_'.repeat(12),controls:['stop','later'],question:{parsed:true,problem:'Choose the compatible release window. '.repeat(160),choices:[{id:'7',text:'Keep compatibility. '.repeat(80),recommended:true}]}};
  const s={...snapshot,page:'home',projects:[{...project,tasks:[...Array.from({length:8},(_,i)=>({...t(20+i,'doing'),title:'Task before '+i})),question,...Array.from({length:20},(_,i)=>t(100+i,'ready'))].map((t,rank)=>({...t,rank}))}]};
  const frames=page=>page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  for(const width of [320,360])for(const theme of ['light','dark'])for(const font of [14,16,18,20]){
   const coarse=font===20&&width===320,page=await browser.newPage({viewport:{width,height:560},colorScheme:theme,hasTouch:coarse});
   page.on('pageerror',e=>{throw e;});await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await page.evaluate(s=>taskq.receive(s),{...s,theme,font});
   const row=page.locator('[data-card="'+key+'"]'),identity=row.locator('[data-a=task-detail]');
   await identity.click();
   const summary=page.locator('.task-card-text').first();await summary.scrollIntoViewIfNeeded();
   const box=await summary.boundingBox();await page.mouse.move(box.x+box.width/2,Math.max(150,Math.min(420,box.y+box.height/2)));
   const before=await page.locator('.content').evaluate(e=>e.scrollTop);await page.mouse.wheel(0,90);
   await page.waitForFunction(y=>document.querySelector('.content').scrollTop>y+1,before,{timeout:1500});
   assert.equal(await page.locator('#taskq-popover').count(),0,'question details join the menu flow');
   await page.keyboard.press('Escape');assert.equal(await identity.getAttribute('aria-expanded'),'false');assert(await identity.evaluate(e=>e===document.activeElement));
   // #1781: the amber question glyph is the one direct question action (input-scroll 04); the closed row holds no question text.
   const cue=row.locator('.question-shortcut');assert.equal(await row.locator('.question-shortcut[data-a=workspace]').count(),1,'one direct question action');assert.equal(await cue.count(),1,'the action is the question glyph');
   assert((await cue.boundingBox()).height>=(coarse?44:32),'existing pointer target size retained');
   assert.equal(await row.locator('ol,[data-qac],.question-cue').count(),0,'question text and choices stay in Conversations');
   for(const [target,edge] of [[identity],[row.locator('.tqp-task-title')],[cue],[row,'top'],[row,'bottom'],[row,'left'],[row,'right']])for(const delta of [90,12]){
    // Center by actual scrolling, then test both directions at text, action and outer preview boundaries.
    await target.scrollIntoViewIfNeeded();let r=await target.boundingBox();await page.mouse.move(edge==='left'?r.x+1:edge==='right'?r.x+r.width-1:r.x+r.width/2,edge==='top'?r.y+1:edge==='bottom'?r.y+r.height-1:r.y+Math.min(r.height/2,20));
    for(const sign of [1,-1]){const y=await page.locator('.content').evaluate(e=>e.scrollTop);await page.mouse.wheel(0,sign*delta);await page.waitForFunction(({y,sign})=>sign*(document.querySelector('.content').scrollTop-y)>1,{y,sign},{timeout:1500});await frames(page);}
   }
   assert.deepEqual(await row.evaluate(e=>[e,...e.querySelectorAll('*')].filter(n=>['auto','scroll'].includes(getComputedStyle(n).overflowY)&&n.scrollHeight>n.clientHeight).map(n=>n.className)),[],'no question scroll container');
   await page.mouse.wheel(0,20000);await page.waitForFunction(()=>{const c=document.querySelector('.content');return c.scrollHeight-c.clientHeight-c.scrollTop<=1;});
   assert(await page.locator('[data-card]').last().evaluate(e=>e.getBoundingClientRect().bottom<=document.querySelector('.content').getBoundingClientRect().bottom+1),'last row remains reachable');
   await page.mouse.wheel(0,-20000);await page.waitForFunction(()=>document.querySelector('.content').scrollTop===0);
   await page.locator('[data-a=search-open]').click();await page.locator('[data-task-query]').fill('Task');await cue.scrollIntoViewIfNeeded();await frames(page);
   const position=await page.locator('.content').evaluate(e=>e.scrollTop);await cue.click();
   const open=await page.evaluate(()=>messages.filter(m=>m.a==='workspace').at(-1).open);assert.deepEqual([open.root,open.board,open.task,open.revision],['/fixture/alpha',board,4,4]);
   // The native workspace is a separate window. Closing/returning and a status refresh leave menu context intact.
   await page.evaluate(s=>taskq.receive(s),{...s,page:undefined,theme,font});await frames(page);
   assert.equal(await page.locator('[data-task-query]').inputValue(),'Task');assert(Math.abs(await page.locator('.content').evaluate(e=>e.scrollTop)-position)<=2,'return preserves scroll');
   assert.equal(await page.locator('[data-a=view-list]').first().innerText(),'All tasks *','return preserves working view');
   assert.equal(await page.evaluate(()=>messages.filter(m=>['answer','control','start','message-action','ai-send'].includes(m.a)).length),0,'navigation has no side effect');
   const overflow=await page.locator('.content,.content-body,[data-card],.question-shortcut').evaluateAll(es=>es.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.className));assert.deepEqual(overflow,[],'narrow/large text reflows without root clipping');
   await identity.click();const detail=row.locator('.question-task-details');
   // #1781 G5: the details card is the neutral raised surface (amber edge only) and its Open question icon is amber (input-scroll 05)
   const look=await detail.evaluate(e=>{const c=getComputedStyle(e),q=e.querySelector('.popover-heading>[data-a=workspace]');return {bg:c.backgroundColor,edge:c.borderLeftColor,icon:q&&getComputedStyle(q).color,ask:getComputedStyle(q).color};});
   assert.equal(look.bg,theme==='dark'?'rgb(48, 58, 67)':'rgb(239, 242, 245)','neutral card');assert.equal(look.icon,look.ask,'amber Open question icon');assert.equal(look.edge,look.ask,'amber edge kept');
   assert.equal(await detail.locator('[data-a=control]').count(),2,'question lifecycle controls retained');await detail.getByRole('button',{name:'Pin task',exact:true}).click();assert.match(await page.locator('.task-pins').innerText(),/#4/);await page.locator('.task-pins [data-a=task-detail]').click();await page.locator('.task-pins').getByRole('button',{name:'Unpin task',exact:true}).click();assert.equal(await page.locator('.task-pins').count(),0,'question pin and unpin retained');
   if(out&&width===320&&font===20){await page.evaluate(s=>taskq.receive(s),{...s,page:undefined,theme,font,projects:[{...s.projects[0],tasks:s.projects[0].tasks.map(t=>t.n===4?{...t,title:'Task question: choose a compatible release window'}:t)}]});await row.scrollIntoViewIfNeeded();await page.screenshot({path:out+'/compact-question-'+theme+'.png'});}
   await page.setViewportSize({width,height:430});assert(await cue.isVisible(),'short menu keeps the question glyph');
   await page.close();
  }
  console.log('PASS #1740 A1-A5: real wheel/small-delta gestures, inline question controls, exact thread target and preserved menu context; 320/360px, 14/16/18/20px, both themes. Native macOS/Windows: Not tested.');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
