// R11/E2: bundled Stop review -> one scoped mode -> acknowledgement, with stale observations kept unknown.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try {
  for(const [theme,font,width] of [['dark',16,320],['light',20,360]]) {
   const page=await browser.newPage({viewport:{width,height:1000},colorScheme:theme});
   const errors=[];page.on('pageerror',e=>errors.push(e.message));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>window.messages.push(JSON.parse(s))};});
   await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   const root='/fixture/project',review={id:'review',action:'stop',instance:'original',roots:[root],issued_at:1000,expires_at:1300};
   const state={stop_modes:true,page:'stop',theme,font,now:1000,service:{state:'running',pid:123,instance:'original',fresh:true},stop_review:review,full_stop_review:{...review,mode:'full'},projects:[{root,name:'Fixture project',host:'mac',alive:true,age:0,service_pid:123,host_instance:'original',restart_waits:['codex worker'],stop_progress:[],tasks:[],focus:['all'],labels:[],alive:true}],ai:{draft:''},notify:{projects:{}}};
   await page.evaluate(s=>taskq.receive(s),state);
   await page.evaluate(()=>taskq.receive({open:{page:'operations',operation:'stop'}}));
   await page.getByRole('button',{name:'Confirm Drain',exact:true}).waitFor();
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'Stop review must fit');
   if(process.env.TASKQ_STOP_SHOTS)await page.screenshot({path:process.env.TASKQ_STOP_SHOTS+'/'+theme+'-review.png',fullPage:true});
   await page.getByRole('button',{name:'Confirm Full stop',exact:true}).click();
   const sent=await page.evaluate(()=>messages.filter(m=>m.a==='stop'));assert.equal(sent.length,1);assert.equal(sent[0].mode,'full');assert.deepEqual(sent[0].roots,[root]);
   await page.evaluate(r=>taskq.receive({capmsg:{id:r.id,state:'queued'}}),sent[0]);
   state.stop_hold={id:sent[0].id,state:'held',mode:'full'};state.projects[0].stop_progress=[{task:7,rt:'codex',state:'unknown',reason:'native exit unavailable'}];
   await page.evaluate(s=>taskq.receive(s),state);
   assert.match(await page.locator('body').innerText(),/native exit unavailable/);
   assert.match(await page.locator('body').innerText(),/queued/);
   assert(await page.getByRole('button',{name:'Confirm Full stop',exact:true}).isDisabled());
   if(process.env.TASKQ_STOP_SHOTS)await page.screenshot({path:process.env.TASKQ_STOP_SHOTS+'/'+theme+'-unknown.png',fullPage:true});
   state.projects[0].alive=false;state.projects[0].stop_progress[0].state='stopped';
   await page.evaluate(s=>taskq.receive(s),state);assert.match(await page.locator('body').innerText(),/Process state unknown/);
   assert.doesNotMatch(await page.locator('body').innerText(),/#7 codex: stopped/);
   state.projects[0].alive=true;state.projects[0].host_instance='previous';
   await page.evaluate(s=>taskq.receive(s),state);assert.match(await page.locator('body').innerText(),/Process state unknown/);
   assert.doesNotMatch(await page.locator('body').innerText(),/#7 codex: stopped/);assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();}
 console.log('PASS Stop review and unknown progress in bundled browser UI; native effects not exercised');
})().catch(e=>{console.error(e);process.exitCode=1;});
