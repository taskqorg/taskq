// #1465 A1–A7 / V15/V28: placement, target, single-use Stop and unknown receipts.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const root='/fixture/project',review={id:'review',action:'stop',instance:'original',roots:[root],issued_at:1000,expires_at:1300};
const snapshot={stop_modes:true,font:16,theme:'dark',host:'fixture',now:1000,version:'invented',service:{state:'running',pid:123,instance:'original',fresh:true},stop_review:review,full_stop_review:{...review,mode:'full'},projects:[{root,name:'Fixture project',host:'mac',alive:true,age:0,service_pid:123,host_instance:'original',restart_waits:['codex worker'],stop_progress:[],tasks:[],focus:['all'],labels:[],alive:true}],ai:{draft:'',history:[]},notify:{projects:{}},history:{bins:[[0,0,0,0]],start:0,end:1000,tz:'UTC',since:null}};
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
  assert.deepEqual(errors,[]);
  console.log('PASS workspace operation launch/Back, exact Full stop, single-use and unknown receipt fencing; native effects not exercised');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
