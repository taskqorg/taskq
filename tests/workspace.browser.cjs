// A1-A4/C-A1,2,3,5,6,10: real workspace bundle, isolated IPC; no paid agent or live board.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const width of [860,360])for(const font of [16,20]){
   const page=await browser.newPage({viewport:{width,height:680},colorScheme:'dark'}),errors=[];
   page.on('pageerror',e=>errors.push(e.message));
   await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>messages.push(JSON.parse(s))};});
   const a={root:'p',project:'Project A',id:'1',first:'1',draft:'draft A',chain:[{id:'1',text:'Question A',answer:{state:'proposal',text:'Full answer\n'+('Retained long answer. '.repeat(100))+'END',rev:4,ops:[{op:'later',project:'A',task:44,revision:41,text:'Pause this task'}]}}]},b={root:'p',project:'Project B',id:'2',first:'2',draft:'draft B',chain:[{id:'2',text:'Question B',answer:{state:'result',text:'Answer B'}}]};
   const data={conversations:[a,b],selected:{root:'p',id:'1'},projects:[{root:'p',name:'Project A'}],history:['History fills only'],draft:''};
   await page.evaluate(s=>taskq.receive(s),{data,select:true,font});
   assert((await page.locator('.transcript').textContent()).includes('END'));
   assert.equal(await page.locator('#draft').inputValue(),'draft A');
   await page.locator('[data-a=confirm]').click();
   assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='confirm').map(({a,root,id,rev})=>({a,root,id,rev}))),[{a:'confirm',root:'p',id:'1',rev:4}]);
   assert(await page.locator('[data-a=confirm]').isDisabled());
   a.chain[0].answer={state:'apply failed',rev:5,text:'Partial',readback:['#44 applied','#45 unknown']};
   await page.evaluate(s=>taskq.receive(s),{data,receipt:true,error:'Changed while applying'});
   assert.equal(await page.locator('[data-a=confirm]').count(),0);assert(await page.locator('[data-a=send]').isDisabled());
   assert((await page.locator('.transcript').textContent()).includes('#45 unknown'));
   if(width<540)await page.locator('header [data-a=list]').click();
   await page.locator('[data-a=select][data-id="2"]').click();
   assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='send').length),0,'opening never sends');
   data.selected={root:'p',id:'2'};await page.evaluate(s=>taskq.receive(s),{data,select:true});
   assert.equal(await page.locator('#draft').inputValue(),'draft B');
   await page.locator('#draft').fill('Saved draft B');
   await page.evaluate(s=>taskq.receive(s),{data});
   assert.equal(await page.locator('#draft').inputValue(),'Saved draft B');
   await page.locator('summary').click();await page.locator('[data-a=history]').click();
   assert.equal(await page.locator('#draft').inputValue(),'History fills only');
   assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='send').length),0);
   await page.locator('#draft').dispatchEvent('keydown',{key:'Enter',isComposing:true});
   assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='send').length),0);
   await page.locator('[data-a=send]').click();await page.locator('[data-a=send]').evaluate(e=>e.click());
   const sent=await page.evaluate(()=>messages.filter(m=>m.a==='send'));assert.equal(sent.length,1);assert.equal(sent[0].id,'2');
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
   assert.deepEqual(errors,[]);await page.close();
  }
  console.log('PASS full replies, exact revisions, partial outcomes, drafts, selection/history without resend, IME and narrow/large text');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
