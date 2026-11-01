// Chain: §7 Reports -> Menu/Conversations buttons -> board receipts -> visible report. Invented IPC, real bundles.
// Risk: one surface drifts, unavailable writes hide a report, or narrow/large-text cards lose actions.
// Product #1977: Results now opens the exact Conversations thread; shared cards retain the old unknown/held-only protection.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/task-workspace.json'),out=process.env.TASKQ_REPORT_EVIDENCE;
const sha='a'.repeat(40),board='https://github.com/invented/research/issues';
const report={n:12,sha,title:'Recovery options for interrupted requests',kind:'report',url:board+'/12',closed:200,read:false,decision:'none',visible:true,body:'# Question\n\nHow should interrupted requests recover?\n\n# Findings\n\nImmutable requests retain their first receipt.\n\n# Options\n\nReuse the identity: avoids duplicate work; requires immutable input.\n\n# Recommendation\n\nReuse the original identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```',tasks:[{title:'Document request recovery',goal:'Make recovery steps visible.',acceptance:['Help names the original request identity.']}]};
const task={...report,n:13,title:'Recover interrupted requests once',kind:'task',url:board+'/13',body:'Changed: Requests recover once.\nChecks: replay -- passed\nChecks: identity -- passed\nOpen: Native recovery unverified.'};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());let cells=0;try{
 for(const surface of ['workspace','tray'])for(const theme of ['dark','light'])for(const width of [350,1100])for(const font of [16,20]){
  const page=await browser.newPage({viewport:{width,height:850}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));
  const state={...fx.state,font,theme,projects:[{...fx.state.projects[0],root:'fixture',name:'Research',issues:board,tasks:[],alive:true,complete:true,reports:{complete:true,observed:200,reports:[report,task],history:[{...task,n:14,title:'Earlier accepted result',url:board+'/14',read:true,decision:'accepted',visible:false}]}}],messages:{complete:true,count:0,entries:[]},conversations:{complete:true,items:[]},ai:{history:[],runtime:{id:'invented',name:'Fixture',available:false}}};
  await page.evaluate(state=>{window.sent=[];window.locals={};window.refuse=false;window.hold=false;window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='answer-local'){locals[m.key]=m.local;return;}if(m.a!=='reports')return;if(hold)return;
   const p=state.projects[0],row=[...p.reports.reports,...p.reports.history].find(r=>r.n===m.n);if(refuse){queueMicrotask(()=>taskq.receive({reportReceipt:{id:m.id,root:m.root,reason:'Board unavailable; decision unknown'}}));return;}
   if(m.action!=='open')row.read=true;if(m.action==='accept')row.decision='accepted';if(m.action==='later'){assertFutureDate(m.until);row.decision='later';row.later_until=m.until;}if(m.action==='drop'){row.decision='drop';row.visible=false;}if(m.action==='make'){row.decision='made';row.visible=false;row.tasks_made=[21];}
   queueMicrotask(()=>taskq.receive({reportReceipt:{id:m.id,root:m.root,report:{...row},...(['reopen','followup'].includes(m.action)?{successor:22,successor_url:p.issues+'/22'}:{})}}));
  }};function assertFutureDate(until){if(!(Date.parse(until)>Date.now()))throw Error('Later needs a future revisit date');}window.fixtureState=state;taskq.receive(state);},state);
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='reports').length),0,'initial snapshot does not acknowledge read');
  if(surface==='tray'){
   await page.locator('.notifications-entry').click();
   const open=await page.evaluate(()=>sent.find(m=>m.a==='workspace'));assert.equal(open.open.page,'conversations');assert.equal(open.open.filter,'new','bell enters the common New view; exact source links are covered by conversations-centre.browser.cjs');assert.equal(await page.locator('.report-card').count(),0,'Notifications only deep-links');
  }else{
   if(width<621)await page.locator('[data-c=list]').click();
   const filter=async name=>{await page.locator('.conversation-view').evaluate(e=>e.open=true);await page.locator('[data-c=result-filter][data-filter='+name+']').click();};
   assert.equal(await page.locator('[data-report=toggle]').count(),0,'no separate Results surface');
   await page.locator('[data-report=open][data-n="12"]').click();await page.locator('[data-result-card=decision] [data-choice="1"]').waitFor();
   assert(await page.locator('.report-card').isVisible());assert.equal(await page.locator('.reports-section').count(),0);
   if(width>620)assert(await page.locator('.conversation-list').isVisible(),'reader preserves sidebar');
   assert.match(await page.locator('.report-card').innerText(),/Recommendation/);
   await page.locator('[data-result-card=decision] [data-choice="2"]').click();await page.getByLabel('Revisit date').waitFor();
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='reports'&&m.action==='later').length),0,'Later waits for the date and Send');
   await page.getByLabel('Revisit date').fill('2000-01-01');assert.equal(await page.locator('[data-report=date-send]').isEnabled(),false,'past dates cannot send');
   await page.getByLabel('Revisit date').fill('2099-01-02');await page.evaluate(()=>taskq.receive(fixtureState));assert.equal(await page.getByLabel('Revisit date').inputValue(),'2099-01-02','refresh keeps the date draft');
   await page.evaluate(()=>{refuse=true;});await page.locator('[data-report=date-send]').click();await page.locator('[data-result-card=decision] [data-qac=retry]').waitFor();
   const dateBefore=await page.evaluate(()=>sent.find(m=>m.a==='reports'&&m.action==='later'));assert.equal(dateBefore.until,'2099-01-02T12:00:00Z');assert.equal(await page.getByLabel('Revisit date').isEnabled(),false,'unknown delivery freezes the date');
   await page.evaluate(()=>{refuse=false;});await page.locator('[data-result-card=decision] [data-qac=retry]').click();await page.waitForFunction(()=>sent.filter(m=>m.a==='reports'&&m.action==='later').length===2);
   const dateAfter=await page.evaluate(()=>sent.filter(m=>m.a==='reports'&&m.action==='later').at(-1));assert.equal(dateAfter.id,dateBefore.id);assert.equal(dateAfter.until,dateBefore.until);await page.getByLabel('Revisit date').waitFor({state:'detached'});
   if(width<621)await page.locator('[data-c=list]').click();await filter('decision');assert.equal(await page.locator('[data-report=open][data-n="12"]').count(),0,'future Later stays outside due attention');
   await filter('finished');assert(await page.locator('[data-report=open][data-n="14"]').isVisible(),'decided history retained');
   await page.locator('[data-report=open][data-n="13"]').click();assert.equal(await page.locator('[data-result-card=decision]').count(),0,'ordinary completions need no Accept');
   assert.match(await page.locator('.report-card').innerText(),/identity -- passed/);
   const other=page.locator('.result-other');if(!await other.evaluate(e=>e.open))await other.locator('summary').click();
   await page.locator('[data-result-card=correction] [data-choice="1"]').click();await page.locator('#result-reason').fill('Preserve the retry receipt');
   assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='reports'&&m.action==='reopen').length),0,'choice waits for reason submit');
   await page.locator('[data-report=reason-send]').click();await page.getByText(/1. Read held draft #22/).waitFor();
   const op=await page.evaluate(()=>sent.find(m=>m.a==='reports'&&m.action==='reopen'));assert.equal(op.text,'Preserve the retry receipt');assert.equal(op.sha,sha);assert.equal(op.n,13);assert(await page.evaluate(()=>Object.values(locals).some(v=>v.request?.action==='reopen'&&v.receipt?.state==='accepted'&&v.successor===22)),'controller receipt save preserves immutable request and held link');
   if(out&&width===1100&&font===16&&theme==='dark'){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:path.join(out,'finished-conversations.png')});}
   // Unknown response retains the same operation and blocks other result cards. A late readback updates its source.
   if(width<621)await page.locator('[data-c=list]').click();await page.locator('[data-report=open][data-n="12"]').click();await page.evaluate(()=>{refuse=true;});
   await page.locator('[data-result-card=decision] [data-choice="1"]').click();await page.locator('[data-result-card=decision] [data-qac=retry]').waitFor();
   const before=await page.evaluate(()=>sent.filter(m=>m.a==='reports'&&m.action==='accept').at(-1));await page.evaluate(()=>{refuse=false;});await page.locator('[data-result-card=decision] [data-qac=retry]').click();
   await page.waitForFunction(()=>sent.filter(m=>m.a==='reports'&&m.action==='accept').length===2);const after=await page.evaluate(()=>sent.filter(m=>m.a==='reports'&&m.action==='accept').at(-1));assert.equal(after.id,before.id);assert.equal(after.sha,before.sha);
   await page.waitForFunction(()=>document.querySelector('.report-card').textContent.includes('Decision recorded: accepted'));
   const overflow=await page.evaluate(()=>[document.documentElement,...document.querySelectorAll('.report-card,.conversation-reader,.conversation-list')].filter(e=>e.clientWidth&&e.scrollWidth>e.clientWidth+2).map(e=>({class:e.className,scroll:e.scrollWidth,width:e.clientWidth})));assert.deepEqual(overflow,[],JSON.stringify({surface,theme,width,font,overflow}));
   if(width===1100&&font===16&&theme==='dark'){
    await page.evaluate(()=>{const p=fixtureState.projects[0],template=p.reports.reports[1];p.reports.history=Array.from({length:250},(_,i)=>({...template,n:100+i,closed:100,read:true,decision:'accepted',visible:false,title:'History '+(100+i)}));taskq.receive(fixtureState);});
    await page.locator('[data-c=results-more]').click();assert.equal(await page.locator('[data-n="250"]').count(),1,'numeric task tie break keeps close-time boundary');assert.equal(await page.locator('[data-n="249"]').count(),0);
    await page.evaluate(()=>{const p=fixtureState.projects[0];p.reports.history.unshift({...p.reports.history[0],n:999,closed:500});taskq.receive(fixtureState);});assert.equal(await page.locator('[data-n="250"]').count(),1,'new collection does not displace loaded boundary');
    await page.locator('[data-c=results-more]').click();assert.equal(await page.locator('[data-n="150"]').count(),1,'More continues strictly after prior close-time/task cursor');
    await page.evaluate(()=>{const p=fixtureState.projects[0];p.reports.history=[];p.reports.reports.push({...p.reports.reports[0],n:400,title:'PM outcome',conversation:'pm-result',read:true,decision:'none'});p.tasks=[{n:401,title:'Linked question',state:'ask',revision:5,updated:600,conversation:{id:'pm-question',brief:'Choose'}}];fixtureState.conversations.items=['pm-result','pm-question'].map((id,i)=>({id,root:'fixture',title:id,text:'Preserved PM draft',caret:[0,0],revision:1,files:[],updated:700000-i*100000}));fixtureState.messages.entries=[{key:'f'.repeat(64),kind:'question',board:p.issues,root:p.root,task:401,revision:5,title:'Linked question',state:'ask'}];taskq.receive(fixtureState);});
    await filter('reply');assert.equal(await page.locator('.conversation-row[data-id="pm-question"]').count(),1,'linked question projects Needs reply onto PM');assert.equal(await page.locator('[data-thread="task:'+board+'/401"]').count(),0,'unopened PM task has no duplicate row');
    await filter('all');await page.locator('[data-report=open][data-n="13"]').click();await page.locator('[data-report=unresolved]').click();assert.equal(await page.locator('.conversation-row[aria-current=page]').getAttribute('data-id'),'pm-result','Next resolves a composed PM sidebar row');
    await page.getByRole('button',{name:'Read result and decide',exact:true}).click();assert(await page.locator('.report-card').isVisible(),await page.locator('main').innerText());assert.match(await page.locator('.conversation-detail').getAttribute('data-thread-id'),/^result:/,'PM result reading position uses its own source identity');assert.equal(await page.locator('[data-thread="task:'+board+'/400"]').count(),0,'reading linked outcome stays in its PM identity');await page.locator('[data-a=result-return]').click();assert.equal(await page.locator('[data-c-field=text]').inputValue(),'Preserved PM draft');
    await page.evaluate(({board,sha})=>{fixtureState.conversations.items.find(c=>c.id==='pm-result').archived=true;taskq.receive(fixtureState);taskq.receive({open:{page:'conversations',root:'fixture',board,task:400,sha}});},{board,sha});assert(await page.locator('.report-card').isVisible(),'archived PM result displays before read');assert.equal(await page.locator('[data-c=archive-list]').getAttribute('aria-pressed'),'true','exact result opens owning PM archive');

    await page.evaluate(({board})=>taskq.receive({open:{page:'results',root:'fixture',board,task:12,sha:'b'.repeat(40)}}),{board});assert.equal(await page.locator('.report-card').count(),0,'old-SHA link never shows current result');assert.match(await page.locator('.conversation-status').innerText(),/requested result SHA is unavailable/);
   }

  }
  assert.deepEqual(errors,[]);await page.close();cells++;
 }
 console.log('PASS Reports in Conversations: '+cells+' surface/theme/width/font cells; shared decisions, held successor, source-bound retry and notification deep-links');
 }finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
