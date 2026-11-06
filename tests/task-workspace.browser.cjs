// #1645 A3: replay the published narrow, large-text and keyboard journeys on the released bundle, hosted as DashQ hosts it.
// Risk: the published component breaks outside the tray page, or a journey DashQ pins no longer holds.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/task-workspace.json'),css=fs.readFileSync('build/web/task-workspace.css','utf8');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const j of fx.journeys){
   const p=await browser.newPage({viewport:{width:j.viewport[0],height:j.viewport[1]}});p.on('pageerror',e=>{throw e;});
   await p.setContent('<!doctype html><html><head><meta charset="utf-8"><style>'+css+'</style></head><body><div class="menu"></div></body></html>');
   await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
   await p.addScriptTag({path:'build/web/task-workspace.js'});
   await p.evaluate(s=>{const w=TaskqTaskWorkspace;w.create(w.createState(),{}).mount();taskq.receive(s);},{...fx.state,font:j.font});
   for(const [i,s] of j.steps.entries()){
    const at=j.id+' step '+i;
    if(s.click)await p.locator(s.click).first().click();
    if(s.press&&!s.until)await p.keyboard.press(s.press);
    if(s.until){let n=0;while(!await p.evaluate(sel=>document.activeElement?.matches(sel),s.until)){assert(++n<=s.max,at+': '+s.until+' not reached');await p.keyboard.press(s.press);}}
    const e=s.expect;if(!e)continue;
    for(const [sel,n] of Object.entries(e.count||{}))await p.waitForFunction(([sel,n])=>document.querySelectorAll(sel).length===n,[sel,n],{timeout:2000}).catch(()=>assert.fail(at+': '+sel+' count is not '+n));
    if('open' in e)await p.waitForFunction(o=>!!document.querySelector('#taskq-popover:popover-open,.task-inline-details')===o,e.open,{timeout:2000}).catch(()=>assert.fail(at+': popover open is not '+e.open));
    if(e.focus)assert(await p.evaluate(sel=>!!document.activeElement?.closest(sel),e.focus),at+': focus is not in '+e.focus);
    for(const [sel,text] of Object.entries(e.text||{}))assert((await p.locator(sel).innerText()).includes(text),at+': '+sel+' lacks '+text);
    if(e.fits)assert(await p.evaluate(()=>document.scrollingElement.scrollWidth<=innerWidth&&[...document.querySelectorAll('#taskq-popover:popover-open,.task-inline-details')].every(x=>x.getBoundingClientRect().right<=innerWidth)),at+': horizontal overflow');
    if(e.sent)await p.waitForFunction(a=>messages.some(m=>m.a===a),e.sent,{timeout:2000}).catch(()=>assert.fail(at+': no '+e.sent+' action sent'));
   }
   await p.close();
  }
  console.log('PASS task workspace journeys on the published bundle: '+fx.journeys.map(j=>j.id).join(', '));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
