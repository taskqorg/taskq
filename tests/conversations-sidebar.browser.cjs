// CS1–CS6 / #1808: actual bundle, invented native receipts. Risk: mixed sources duplicate or lose threads,
// local organization changes task/session truth, or refresh loses the selected draft/focus.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const a='https://example.org/a/issues',b='https://example.org/b/issues',key='e'.repeat(64),q='f'.repeat(64);
const draft=(id,root,updated)=>({id,root,updated,title:({'pm-a':'Daily PM','owner-a':'Release checks','owner-b':'Dashboard plans','native-b':'Dashboard PM',global:'Runtime comparison',missing:'Earlier setup'})[id]||id,text:'Draft for '+id,caret:[0,0],scroll:0,revision:1,files:[],archived:false});
const s={font:16,theme:'dark',palette:'--work:light-dark(#087f5b,#38e8a0);--ask:light-dark(#995200,#ffc247);--divider:light-dark(#dce0e5,#46505b)',now:2000,host:'fixture',ai:{history:[],runtime:{id:'codex',name:'Codex',available:true}},projects:[
 {root:'alpha',name:'Alpha',issues:a,alive:true,complete:true,tasks:[{n:12,title:'Choose the API',state:'ask',revision:5,updated:1800},{n:99,title:'Running without a question',state:'doing',revision:7,updated:1700}]},
 {root:'beta',name:'Beta',issues:b,alive:true,complete:true,tasks:[]}],
 conversations:{complete:true,sidebar:{pins:['pm-a'],collapsed:[],archived:{}},navigation:{selected:'pm-a',list:'recent'},items:[draft('pm-a','alpha',1500000),draft('owner-a','alpha',1600000),draft('owner-b','beta',1900000),draft('global','',1200000),draft('missing','unregistered',null)]},
 messages:{complete:true,entries:[{key:q,kind:'question',board:a,root:'alpha',project:'Alpha',task:12,revision:5,title:'Choose the API',state:'ask',read:false,archived:false},
 {key,kind:'request',id:'orphan',root:'beta',title:'Event thread',created:1300000,host:'fixture',state:'failed',read:false,archived:false,request:{reason:'Invented failure'}}]}};
s.conversations.items.push({...draft('native-b','alpha',null),sent:{text:'Start',root:'beta',runtime:'codex',files:[]},request:{id:'native-b',state:'launched',runtime:'codex'}});
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:1100,height:850}}),errors=[];page.on('pageerror',e=>errors.push(e.message));let pause=false,nativeAt=1400000;const evidence=process.env.TASKQ_EVIDENCE_1808;async function shot(name){if(evidence){fs.mkdirSync(evidence,{recursive:true});await page.screenshot({path:path.join(evidence,name+'.png')});}}
 await page.exposeFunction('receipt',async m=>{
  if(m.a==='conversation-navigation'&&m.sidebar){
   if(pause){await page.evaluate(id=>taskq.receive({conversationSidebarReceipt:{id,state:'unknown',reason:'Invented write refusal'}}),m.id);return;}
   const v=s.conversations.sidebar,{action,key,value,root}=m.sidebar;
   if(action==='pin'||action==='collapse'){const k=action==='pin'?'pins':'collapsed';v[k]=v[k].filter(x=>x!==key);if(value)v[k].push(key);}
   if(action==='archive')v.archived[key]=root||'';if(action==='restore')delete v.archived[key];
   await page.evaluate(r=>taskq.receive({conversationSidebarReceipt:r}),{id:m.id,state:'accepted',sidebar:v});
  }else if(m.a==='conversation-dialogue'&&m.action==='activity'){await page.evaluate(r=>taskq.receive({dialogueReceipt:r}),{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:{updated:new Date(nativeAt).toISOString()}});
  }else if(m.a==='conversation-navigation')s.conversations.navigation=m.navigation;
  else if(m.a==='conversation-action'){
   const c=s.conversations.items.find(c=>c.id===m.conversation);assert(c,'only the named existing owner record');
   if(m.action==='save')Object.assign(c,m.draft);if(m.action==='archive')c.archived=true;if(m.action==='restore')c.archived=false;c.revision++;
   await page.evaluate(r=>taskq.receive({conversationReceipt:r}),{id:m.id,state:'accepted',snapshot:s.conversations});
  }else if(m.a==='message-action'){
   for(const e of s.messages.entries)if(m.keys.includes(e.key))e.archived=m.action==='archive';
   await page.evaluate(r=>taskq.receive({messageReceipt:r}),{id:m.id,outcomes:m.keys.map(key=>({key,state:'accepted',action:m.action}))});
  }
 });
 async function boot(){await page.goto('about:blank');await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));await page.evaluate(()=>{window.sent=[];window.ipc={postMessage:t=>{const m=JSON.parse(t);sent.push(m);window.receipt(m);}};});await page.evaluate(s=>taskq.receive(s),s);}
 const row=id=>page.locator('.conversation-item').filter({has:page.locator('[data-id="'+id+'"],[data-thread="'+id+'"]')});
 async function click(id,name){await row(id).hover();await row(id).getByRole('button',{name,exact:true}).click();}
 const groupOrder=()=>page.locator('[data-conversation-group]').evaluateAll(es=>es.map(e=>e.getAttribute('aria-label')));
 await boot();
 assert.deepEqual(await groupOrder(),['Pinned','Beta','Alpha','Other'],'CS1: all three sources share ordered groups');
 const all=await page.locator('.conversation-row').evaluateAll(es=>es.map(e=>e.dataset.id||e.dataset.thread));assert.equal(all.length,new Set(all).size,'one identity, one group');
 assert.equal(await page.locator('[data-conversation-group][aria-label=Beta] [data-c=select][data-id=native-b]').count(),1,'launched chat stays in its original project when next-draft destination differs');
 assert.match(await row('missing').innerText(),/Project unavailable/);assert.match(await row('missing').innerText(),/Time unknown/);await row('owner-a').hover();await shot('01-inline-groups');
 const input=page.locator('[data-c-field=text]');await input.fill('Retained current draft');
 await click('owner-a','Pin');await row('owner-a').getByRole('button',{name:'Unpin',exact:true}).waitFor();
 assert.equal(await page.locator('[data-conversation-group=pinned]').locator('.conversation-row').count(),2);assert.equal(await input.inputValue(),'Retained current draft');assert.equal(await page.locator('.conversation-row[aria-current=page]').getAttribute('data-id'),'pm-a');
 await click('owner-a','Unpin');await row('owner-a').getByRole('button',{name:'Pin',exact:true}).waitFor();
 const alpha=page.getByRole('button',{name:'Alpha 2',exact:true});await alpha.click();await page.waitForFunction(()=>document.querySelector('[data-conversation-group][aria-label=Alpha]>button').getAttribute('aria-expanded')==='false');
 await page.waitForFunction(()=>sent.some(m=>m.a==='conversation-action'&&m.action==='save'&&m.draft?.text==='Retained current draft'));await boot();assert.equal(await page.locator('[data-conversation-group][aria-label=Alpha]>button').getAttribute('aria-expanded'),'false','collapse survives the native preference round trip and reopening');assert.equal(await input.inputValue(),'Retained current draft');
 await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('Choose');assert.equal(await page.locator('[data-thread="task:'+a+'/12"]').count(),1,'search reveals collapsed task group');await shot('02-search');
 await page.locator('[data-c=clear-search]').click();assert.equal(await page.locator('[data-thread="task:'+a+'/12"]').count(),0,'clear restores collapse');
 await page.getByRole('button',{name:'Alpha 2',exact:true}).click();await page.locator('[data-thread="task:'+a+'/12"]').waitFor();
 // #2110 CS2: arrivals do not move rows under the pointer while reading; explicit view changes can re-sort.
 const frozen=await groupOrder();
 s.projects[0].tasks[0].updated=2100;await page.evaluate(s=>taskq.receive(s),s);assert.deepEqual(await groupOrder(),frozen);assert.equal(await input.inputValue(),'Retained current draft');
 const target=row('owner-b').getByRole('button',{name:'Pin',exact:true});await target.focus();s.conversations.items.find(c=>c.id==='owner-b').updated=2200000;await page.evaluate(s=>taskq.receive(s),s);assert.equal(await target.evaluate(e=>e===document.activeElement),true,'focus follows the same row when groups reorder');
 s.conversations.items.find(c=>c.id==='owner-b').updated=2000000;await page.evaluate(s=>taskq.receive(s),s);assert.deepEqual(await groupOrder(),frozen);
 nativeAt=2400000;await page.waitForFunction(()=>sent.some(m=>m.action==='activity'));assert.deepEqual(await groupOrder(),frozen);assert.equal(await input.inputValue(),'Retained current draft','new native message reorders the group without stealing the draft');
 s.conversations.items.find(c=>c.id==='owner-b').updated=2500000;await page.evaluate(s=>taskq.receive(s),s);assert.deepEqual(await groupOrder(),frozen);await page.locator('.conversation-view>summary').click();await page.locator('[data-c=result-filter][data-filter=all]').click();assert.deepEqual(await groupOrder(),['Pinned','Beta','Alpha','Other'],'explicit view refresh applies source recency');await page.locator('[data-c=select][data-id=pm-a]').click();
 const before=await groupOrder();await page.evaluate(s=>taskq.receive(s),s);assert.deepEqual(await groupOrder(),before);
 pause=true;await click('global','Pin');await page.getByText('Invented write refusal',{exact:false}).first().waitFor();assert(!s.conversations.sidebar.pins.includes('global'));pause=false;
 await page.evaluate(()=>taskq.receive({workspaceOwner:{id:'main',detached:['owner-a']}}));
 await click('owner-a','Archive');await page.waitForFunction(()=>!document.querySelector('[data-c=select][data-id="owner-a"]'));assert.equal(await input.inputValue(),'Retained current draft');
 await page.locator('[data-c=archive-list]').click();await click('owner-a','Restore');await page.locator('[data-c=recent]').click();await row('owner-a').getByRole('button',{name:'Pin',exact:true}).waitFor();
 await click('task:'+a+'/12','Archive');await page.waitForFunction(()=>!document.querySelector('[data-thread="task:https://example.org/a/issues/12"]'));assert(s.messages.entries.find(e=>e.key===q).archived,'task archive uses existing message marks');
 // Ordinary task threads without a message still archive locally and remain reachable after reopening.
 await page.evaluate(board=>taskq.receive({open:{page:'conversations',root:'alpha',board,task:99}}),a);
 await click('task:'+a+'/99','Archive');await page.waitForFunction(()=>!document.querySelector('[data-thread="task:https://example.org/a/issues/99"]'));
 await page.locator('[data-c=archive-list]').click();await row('task:'+a+'/99').getByRole('button',{name:'Restore',exact:true}).waitFor();await row('task:'+a+'/99').hover();await shot('03-archive');
 await boot();await page.locator('[data-c=archive-list]').click();await click('task:'+a+'/99','Restore');await page.locator('[data-c=recent]').click();
 assert(!s.conversations.sidebar.archived['task:'+a+'/99']);
 assert.equal(await page.evaluate(()=>sent.filter(m=>['answer','control','start','ai-send','conversation-window'].includes(m.a)||['send','reply','cancel'].includes(m.action)).length),0,'organization never sends, stops or changes task/session state');
 await page.locator('[data-c=search-open]').click();await page.locator('[data-c-field=search]').fill('not present');assert.match(await page.locator('.conversation-rows').innerText(),/No matches/);await page.locator('[data-c=clear-search]').click();
 for(const width of [1100,500])for(const font of [16,20])for(const theme of ['dark','light']){
  await page.setViewportSize({width,height:850});await page.evaluate(({s,font,theme})=>taskq.receive({...s,font,theme}),{s,font,theme});
  if(!await page.locator('.conversation-list').isVisible())await page.locator('[data-c=list]').click();
  assert(await page.locator('.conversation-group-head').evaluateAll(es=>es.every(e=>{const s=getComputedStyle(e);return parseFloat(s.marginTop)>=Math.max(16,parseFloat(s.fontSize))&&parseFloat(s.marginBottom)===4;})),'UX30-02: sidebar uses the shared heading rhythm');
  assert(await page.locator('.conversation-list').isVisible());assert(await page.locator('.conversation-list').evaluate(e=>e.scrollWidth<=e.clientWidth+1));
  await page.locator('[data-c=select][data-id=owner-b]').focus();await page.keyboard.press('Tab');assert.equal(await page.evaluate(()=>document.activeElement.dataset.c),'row-pin','Tab reaches Inline Pin');
 }
 await shot('04-narrow-list');
 // #2159: real manager names must not lose their distinguishing suffix at the native large-text size.
 s.conversations.items.find(c=>c.id==='owner-b').title='Invented dedicated manager for project Beta: questions, reviewed results and continuation';
 for(const width of [1100,900,500])for(const font of [18,20]){
  await page.setViewportSize({width,height:850});await page.evaluate(s=>taskq.receive(s),{...s,font});
  if(!await page.locator('.conversation-list').isVisible())await page.locator('[data-c=list]').click();
  const title=row('owner-b').locator('.conversation-row>span').first();await title.scrollIntoViewIfNeeded();await shot('2159-title-'+width+'-'+font);
  assert(await title.evaluate(e=>e.scrollHeight<=e.clientHeight+1),'#2159: complete manager title is visible, without a two-line clamp');
  assert(await title.evaluate(e=>e.getBoundingClientRect().width>=e.closest('.conversation-item').clientWidth-20),'#2159: inactive row actions do not reserve title width');
  assert.equal(await row('owner-b').locator('.conversation-row').getAttribute('title'),s.conversations.items.find(c=>c.id==='owner-b').title);
  assert.equal(await row('owner-b').getByRole('img',{name:'Draft',exact:true}).count(),1,'compact status retains its accessible meaning');
  await row('owner-b').getByRole('button',{name:'Pin',exact:true}).focus();
  assert(await row('owner-b').evaluate(e=>{const title=e.querySelector('.conversation-row>span').getBoundingClientRect(),actions=e.querySelector('.conversation-row-actions').getBoundingClientRect();return actions.top>=title.bottom;}),'actions do not cover a full title on keyboard focus');
 }
 assert.deepEqual(errors,[]);console.log('PASS #1808/#2159 actual bundle: grouping/receipts/recovery, complete large-text manager titles, retained drafts/focus; no execution effects');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
