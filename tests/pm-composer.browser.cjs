// #1615 A1-A4: released component against an invented adapter; no real native or board effects.
const {chromium}=require('playwright'),fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  const page=await browser.newPage({viewport:{width:600,height:500}}),f=require('./fixtures/pm-requests.json');
  await page.setContent('<style>body{margin:0;background:#17191d;color:#eee;font:16px system-ui}'+fs.readFileSync('src/pm-composer.css','utf8')+'</style><main id="pm"></main>');
  await page.addScriptTag({path:path.resolve('build/web/pm-composer.js')});
  await page.evaluate(f=>{
   window.saved=null;window.sent=[];window.checked=[];window.observations=[];window.ipc={postMessage:m=>observations.push(m)};
   const adapter={context:()=>f.request,load:()=>saved,save:v=>{saved=structuredClone(v);},id:()=>f.request.op_id,now:()=>f.request.issued_at,send:v=>{sent.push(v);return Promise.reject(Error('invented outage'));},reconcile:v=>{checked.push(v);return Promise.resolve({schema:f.schema,actor_id:v.actor_id,op_id:v.op_id,revision:v.revision,outcome:'accepted',request_id:'a'.repeat(64),native:{host:'invented-host',runtime:'claude',state:'launched',session:'invented-session',url:'https://claude.ai/code/session_invented'}});}};
   window.remount=()=>TaskqPmComposer.mount(document.querySelector('#pm'),adapter);remount();
  },f);
  // #1929 A3: the exported composer shares natural growth without a field grip or inner scroll.
  await page.locator('textarea').fill('Long owner draft\n'.repeat(100));
  assert(await page.locator('textarea').evaluate(e=>e.clientHeight>=e.scrollHeight-1&&getComputedStyle(e).resize==='none'));
  await page.evaluate(()=>remount());assert(await page.locator('textarea').evaluate(e=>e.clientHeight>=e.scrollHeight-1));
  await page.locator('textarea').fill(f.request.text);await page.getByRole('button',{name:'Send',exact:true}).click();
  await page.getByText('unknown',{exact:true}).waitFor();
  assert.equal(await page.locator('textarea').inputValue(),f.request.text);assert(await page.getByRole('button',{name:'Send',exact:true}).isDisabled());
  await page.evaluate(()=>remount());await page.getByRole('button',{name:'Check delivery'}).click();await page.getByRole('link',{name:'Open session'}).waitFor();
  const value=await page.evaluate(()=>({sent,checked,saved,observations}));assert.equal(value.sent.length,1);assert.deepEqual(value.sent,value.checked);assert.equal(value.saved.receipt.native.session,'invented-session');
  if(process.env.TASKQ_PM_EVIDENCE){fs.mkdirSync(process.env.TASKQ_PM_EVIDENCE,{recursive:true});await page.screenshot({path:path.join(process.env.TASKQ_PM_EVIDENCE,'pm-native-handoff.png')});}
  await page.evaluate(()=>{window.saved={text:'Unsaved request'};window.unsavedSent=0;TaskqPmComposer.mount(document.querySelector('#pm'),{context:()=>({actor_id:'a',grant_id:'g',revision:'r',target:{installation_id:'i',generation:1,project_id:'p',runtime_id:'claude'}}),load:()=>saved,save:v=>{if(v.request)throw Error('invented persistence failure');saved=v;},id:()=>'op',now:()=>100,send:()=>{unsavedSent++;},reconcile:()=>{unsavedSent++;}});});
  await page.getByRole('button',{name:'Send',exact:true}).click();assert.equal(await page.getByRole('button',{name:'Check delivery'}).count(),0,'unsaved input must never become reconcilable dispatch');assert.equal(await page.evaluate(()=>unsavedSent),0);
  await page.evaluate(f=>{const r=f.request;TaskqPmComposer.mount(document.querySelector('#pm'),{load:()=>({text:r.text,request:r,receipt:{schema:r.schema,actor_id:r.actor_id,op_id:'request-B',revision:r.revision,outcome:'accepted',request_id:'b'.repeat(64),native:{host:'wrong-host',runtime:'claude',session:'wrong-session',url:'https://claude.ai/code/session_wrong'}}}),context:()=>({}),save:()=>{},reconcile:()=>Promise.resolve(null)});},f);
  assert.equal(await page.getByRole('link',{name:'Open session'}).count(),0,'reopening cannot display an unrelated saved handoff');
  console.log('PASS shared PM send/outage/reopen/same-ID readback/native handoff and failed persistence');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
