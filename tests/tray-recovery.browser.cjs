// E3: actual bundled controls retain unknown fences, selection, evidence and same-payload reconciliation.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try {
  for(const [theme,font] of [['dark',16],['light',20]]){
   const page=await browser.newPage({viewport:{width:720,height:1000},colorScheme:theme});
   const errors=[];page.on('pageerror',e=>errors.push(e.message));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>window.messages.push(JSON.parse(s))};});
   await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   const root='/fixture/project';
   const state={page:'recovery',theme,font,now:1000,stop_modes:true,projects:[{root,name:'Fixture project',host:'mac',alive:true,age:0,tasks:[],focus:['all'],labels:[]}],ai:{draft:''},notify:{projects:{}}};
   await page.evaluate(s=>taskq.receive({...s,page:'stop',stop_hold:{state:'held',mode:'full'}}),state);
   await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'recovery'}}));
   assert(!await page.locator('.content').innerText().then(t=>t.includes(root)),'R28: exact paths disclose on demand');
   await page.getByText('Targets',{exact:true}).click();
   assert(await page.locator('.content').innerText().then(t=>t.includes(root)),'the reviewed exact target remains available');
   await page.getByText('Targets',{exact:true}).click();
   await page.getByRole('button',{name:'Read current recovery'}).click();
   const read=await page.evaluate(()=>messages.find(m=>m.a==='recovery-read'));assert(read);
   const review={state:'review',root,id:read.id,stop:'invented-stop',board:'https://example.invalid/issues/0',focus:['all'],mode:'Mixed: unselected work stays held',limits:{limits:{codex:2,claude:1}},access:[{runtime:'codex',effective:'Workspace'},{runtime:'claude',effective:'Full'}],rows:[
    {task:7,title:'Preserve edited files and resume selected work',url:'https://example.invalid/issues/7',execution:41,revision:42,native_exit:'released exit',runtime:'codex',host:'mac',eligible:true,workspace:{fingerprint:'a'.repeat(64),state:'dirty/untracked work retained'}},
    {task:8,title:'Unknown native release remains held',url:'https://example.invalid/issues/8',execution:51,revision:52,native_exit:'unknown',runtime:'claude',host:'mac',eligible:false,blocker:'native release unknown; reconcile exact execution',workspace:{state:'clean; preserved'}}]};
   await page.evaluate(r=>taskq.receive({capmsg:r}),review);
   assert(await page.getByRole('checkbox',{name:/#8/}).isDisabled());assert(!await page.getByRole('checkbox',{name:/#7/}).isChecked());
   await page.getByRole('button',{name:'Confirm selected recovery',exact:true}).click();
   assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='recovery-apply').length),0);
   await page.getByRole('checkbox',{name:/#7/}).check();
   await page.getByRole('textbox',{name:'Effect evidence for task 7'}).fill('Invented target readback confirms the remote write completed once');
   await page.evaluate(s=>taskq.receive(s),state);
   assert.equal(await page.getByRole('textbox',{name:'Effect evidence for task 7'}).inputValue(),'Invented target readback confirms the remote write completed once');
   await page.locator('h1').click();
   await page.evaluate(()=>scrollTo(0,0));
   if(process.env.TASKQ_RECOVERY_SHOTS)await page.screenshot({path:process.env.TASKQ_RECOVERY_SHOTS+'/'+theme+'-review.png',fullPage:true});
   await page.getByRole('button',{name:'Confirm selected recovery',exact:true}).click();
   const request=await page.evaluate(()=>messages.find(m=>m.a==='recovery-apply'));
   assert.deepEqual(request.request.rows.map(r=>r.task),[7]);assert.equal(request.request.rows[0].effects_known,true);assert.equal(request.request.rows[0].revision,42);
   await page.evaluate(r=>taskq.receive({capmsg:{root:r.root,id:r.id,state:'unknown',reason:'Lost receipt; reconcile same selection'}}),request);
   await page.getByRole('button',{name:'Reconcile same selection'}).click();
   const retry=await page.evaluate(()=>messages.filter(m=>m.a==='recovery-apply').at(-1));assert.deepEqual(retry.request,request.request);
   await page.evaluate(r=>taskq.receive({capmsg:{root:r.root,id:r.id,state:'mixed',receipts:[{task:7,outcome:'accepted'}]}}),retry);
   assert.match(await page.locator('body').innerText(),/Mixed; unselected work stays held/);assert(await page.getByRole('checkbox',{name:/#7/}).isDisabled());
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);assert.deepEqual(errors,[]);
   await page.locator('h1').click();
   await page.evaluate(()=>scrollTo(0,0));
   if(process.env.TASKQ_RECOVERY_SHOTS)await page.screenshot({path:process.env.TASKQ_RECOVERY_SHOTS+'/'+theme+'-mixed.png',fullPage:true});
   await page.close();
  }
 } finally {await browser.close();}
 console.log('PASS bundled recovery selection, evidence, unknown and same-payload reconciliation; native effects not exercised');
})().catch(e=>{console.error(e);process.exitCode=1;});
