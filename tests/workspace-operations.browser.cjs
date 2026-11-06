// #1465 A1–A7 / V15/V28: placement, target, single-use Stop and unknown receipts.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const root='/fixture/project',review={id:'review',action:'stop',instance:'original',roots:[root],issued_at:1000,expires_at:1300};
const snapshot={display:require('./fixtures/display-model.json').display,stop_modes:true,font:16,theme:'dark',host:'fixture',now:1000,version:'invented',service:{state:'running',pid:123,instance:'original',fresh:true},stop_review:review,full_stop_review:{...review,mode:'full'},projects:[{root,name:'Fixture project',mode:'run',host:'mac',alive:true,age:0,service_pid:123,host_instance:'original',restart_waits:['codex worker'],stop_progress:[],tasks:[],focus:['all'],labels:[],alive:true}],ai:{draft:'',history:[]},notify:{projects:{}},history:{bins:Array.from({length:6},(_,i)=>({start:i*1000/6,end:(i+1)*1000/6,counts:[0,0,0,0],missing:["fixture"],ongoing:false})),start:0,end:1000,tz:'UTC',since:null}};
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:800,height:600}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  const load=async surface=>{await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await page.evaluate(s=>taskq.receive(s),snapshot);};
  await load('tray');await page.evaluate(s=>taskq.receive({...s,page:'stop'}),snapshot);
  assert.equal(await page.getByRole('button',{name:'Confirm Full stop',exact:true}).count(),0,'A1: no emergency confirmation remains in menu');
  await page.locator('[data-page=operations][data-operation=stop]').first().click();
  assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='stop').length),0,'launcher never stops work');
  assert.equal(await page.evaluate(()=>messages.find(m=>m.a==='workspace').open.operation),'stop');
  await load('workspace');
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'stop'}}));
  assert.match(await page.locator('.content').innerText(),/Fixture project/);
  assert.match(await page.locator('.content').innerText(),/mac|fixture/);
  assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='stop').length),0,'opening performs no effect');
  await page.locator('[data-a=back]').click();assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='stop').length),0,'Back performs no effect');
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'stop'}}));
  if(process.env.TASKQ_OPERATIONS_SHOTS){fs.mkdirSync(process.env.TASKQ_OPERATIONS_SHOTS,{recursive:true});await page.screenshot({path:process.env.TASKQ_OPERATIONS_SHOTS+'/stop-workspace.png'});}
  await page.getByRole('button',{name:'Confirm Full stop',exact:true}).click();
  const sent=await page.evaluate(()=>messages.filter(m=>m.a==='stop'));assert.equal(sent.length,1);assert.equal(sent[0].mode,'full');assert.deepEqual(sent[0].roots,[root]);assert.equal(sent[0].instance,'original');
  assert(await page.getByRole('button',{name:'Confirm Full stop',exact:true}).isDisabled());
  await page.evaluate(id=>taskq.receive({capmsg:{id,state:'observed',action:'stop'}}),sent[0].id);
  assert.match(await page.locator('.content').innerText(),/incomplete|unknown/);
  assert(await page.getByRole('button',{name:'Confirm Full stop',exact:true}).isDisabled(),'incomplete receipt never allows replay');
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'service',root:'/removed'}}));
  assert.match(await page.locator('.content').innerText(),/unavailable/);assert.equal(await page.locator('[data-a=uninstall]').count(),0,'missing target never falls back to another project');
  // Update readback belongs to the original project, not an equal version elsewhere.
  const updated=structuredClone(snapshot);updated.projects[0].update='invented-next';
  await page.evaluate(s=>taskq.receive(s),updated);
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'update'}}));
  await page.locator('[data-a=update]').click();assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='update').length),1);
  updated.projects.push({...updated.projects[0],root:'/fixture/other',name:'Other',version:'invented-next'});
  await page.evaluate(s=>taskq.receive(s),updated);assert.doesNotMatch(await page.locator('.content').innerText(),/Updated to/);
  updated.projects[0].version='invented-next';updated.projects[0].alive=false;
  await page.evaluate(s=>taskq.receive(s),updated);assert.doesNotMatch(await page.locator('.content').innerText(),/Updated to/);
  updated.projects[0].alive=true;await page.evaluate(s=>taskq.receive(s),updated);assert.match(await page.locator('.content').innerText(),/read from status.json/);
  // Setup never selects a folder from JavaScript or registers it through navigation.
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'setup'}}));
  assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='add').length),0);
  await page.locator('[data-a=add]').click();assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='add').map(({context,...m})=>m)),[{a:'add'}]);
  const selection={...updated,add:{state:'picked',kind:'taskq',path:'/fixture/new-project',generation:42}};
  await page.evaluate(s=>taskq.receive(s),selection);await page.locator('[data-a=add-ask]').click();
  assert.match(await page.locator('.content').innerText(),/Register \/fixture\/new-project/);
  await page.locator('[data-a=add-back]').click();assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='add-install').length),0,'cancel review registers nothing');
  await page.locator('[data-a=add-ask]').click();await page.locator('[data-a=add-install]').click();await page.locator('[data-a=add-ask]').click();await page.locator('[data-a=add-install]').click();
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='add-install').map(({context,...m})=>m)),[{a:'add-install',generation:42}],'single use binds the native picker generation');
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'admission',root:'/fixture/project'}}));
  assert.match(await page.locator('.content').innerText(),/current tasks finish/);
  await page.locator('[data-a=pause]').click();assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='pause').length),1);
  const pause=await page.evaluate(()=>messages.find(m=>m.a==='pause'));assert.equal(pause.root,root);
  await page.keyboard.press('Alt+ArrowLeft');assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='pause').length),1);
  await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='hide').length),1);
  // #1580 regression: both new draft controls and existing reviews must coexist under the same collector budget.
  await page.evaluate(()=>taskq.receive({open:{page:'conversations'}}));await page.locator('[data-c=new]').click();
  await page.locator('[data-c-field=text]').fill('Independent owner draft');
  await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'setup'}}));
  assert.match(await page.locator('.content').innerText(),/Choose a folder/);
  assert.deepEqual(errors,[]);
  // #2096 A4/A7/A8: the existing consequential review remains reachable and effect-free in every geometry cell.
  const resources=require('./fixtures/resource-controls.json').cases.find(c=>c.name.startsWith('host commands')).groups[0];
  for(const theme of ['light','dark'])for(const font of [18,20])for(const width of [320,560]){
   const p=await browser.newPage({viewport:{width,height:960}}),issues=[];p.on('pageerror',e=>issues.push(e.message));
   await p.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   const s={...snapshot,theme,font,projects:[{...snapshot.projects[0],resources,access:[{runtime:'codex',effective:'workspace',enforcement:'sandbox'}],capacity:{gate:'open',reasons:[]}}]};
   await p.evaluate(s=>taskq.receive(s),s);
   for(const operation of ['stop','recovery','resources','admission']){
    await p.evaluate(operation=>taskq.receive({open:{page:'operations',operation}}),operation);
    for(const expanded of [false,true]){
     if(expanded)await p.locator('.content details').evaluateAll(es=>es.forEach(e=>e.open=true));
     const clipping=await p.locator('.content').evaluate(c=>{const bounds=c.getBoundingClientRect();return [...c.querySelectorAll('button,a,input,select,summary')].filter(e=>e.checkVisibility()).filter(e=>{const r=e.getBoundingClientRect();return r.left<bounds.left-1||r.right>bounds.right+1||e.scrollWidth>e.clientWidth+1;}).map(e=>e.textContent.slice(0,70));});
     assert.deepEqual(clipping,[],operation+' review '+width+'/'+font+'/'+theme+' expanded='+expanded);
     assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth>innerWidth||document.querySelector('.content').scrollWidth>document.querySelector('.content').clientWidth+1),false);
    }
   }
   assert.deepEqual(await p.evaluate(()=>messages.filter(m=>['stop','restart','uninstall','capacity','pause','resume','focus','recovery-apply','recovery-read','notify-test'].includes(m.a))),[],'opening/expanding reviews sends no effects');
   assert.deepEqual(issues,[]);await p.close();
  }
  console.log('PASS #2096: 32 operation-review cells × collapsed/expanded, no clipping, horizontal scroll or effects');
  console.log('PASS workspace operation launch/Back, exact Full stop, single-use and unknown receipt fencing; native effects not exercised');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
