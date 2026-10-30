// #1940 owner expansion / §7 Results: one cached result flow on Menu and Conversations.
// Risk: ordinary completions are invisible, navigation fetches a board, or Reopen loses its reason/readback.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/task-workspace.json'),out=process.env.TASKQ_RESULT_EVIDENCE,baseline=process.env.TASKQ_RESULT_BASELINE;
const root='fixture',sha='a'.repeat(40),task={n:13,sha,kind:'task',title:'Recover interrupted requests once',url:'https://github.com/invented/research/issues/13',closed:210,read:false,visible:true,decision:'none',body:'Changed: Interrupted requests reuse their original identity.\nChecks: retry -- passed\nChecks: duplicate work -- prevented\nOpen: macOS native observation not tested.',verdict:'Checks: A1 ok; merge-result CI green',pr:'https://github.com/invented/research/pull/20'};
const report={...task,n:12,kind:'report',title:'Request recovery research',closed:200,body:'# Question\n\nHow should interrupted requests recover?\n\n# Recommendation\n\nReuse the request identity.',tasks:[]};
const state=(theme,font)=>({...fx.state,platform:'windows',theme,font,projects:[{...fx.state.projects[0],root,name:'Research',issues:'https://github.com/invented/research/issues',tasks:[],reports:{complete:true,reports:[task,report]}}],messages:{complete:true,count:0,entries:[]},conversations:{complete:true,items:[]},ai:{history:[],runtime:{id:'invented',name:'Fixture',available:false}}});
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 let cells=0;
 for(const surface of ['tray','workspace'])for(const theme of ['dark','light'])for(const width of [350,940])for(const font of [16,20]){
  const page=await browser.newPage({viewport:{width,height:surface==='tray'?850:600}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));
  await page.evaluate(s=>{window.sent=[];window.rows=s.projects[0].reports.reports;window.fail=false;window.emptyHistory=false;window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a==='completed'){queueMicrotask(()=>taskq.receive({completed:{...m,source:'service',tasks:emptyHistory?[]:[{...rows[0],state:'closed',outcome:'success'}],reason:'Collection stale',page:1,more:false}}));return;}if(m.a!=='reports')return;
   const prior=rows.find(r=>r.n===m.n),row={...prior,read:true,decision:m.action==='accept'?'accepted':prior?.decision};
   const result=fail?{reason:'Board unavailable; acknowledgement unknown'}:{report:row,...(['reopen','followup'].includes(m.action)?{successor:25,successor_url:'https://github.com/invented/research/issues/25'}:{})};
   queueMicrotask(()=>taskq.receive({reportReceipt:{id:m.id,root:m.root,...result}}));}};taskq.receive(s);},state(theme,font));
  if(surface==='tray'){assert((await page.locator('[data-a=workspace][data-page=conversations]').getAttribute('aria-label')).includes('0 new'));await page.locator('[data-route=notifications]').click();}
  else {if(await page.locator('[data-c=list]').isVisible())await page.locator('[data-c=list]').click();assert((await page.locator('.conversation-rows').innerText()).includes('New result'));}
  if(surface==='workspace')await page.locator('[data-report=toggle]').click();await page.locator(surface==='tray'?'.notification-row':'.reports-section [data-n="13"]').first().click();
  await page.locator('[data-report=accept]').waitFor();assert((await page.locator('.report-card').innerText()).includes('duplicate work -- prevented'));assert((await page.locator('.report-card').innerText()).includes('macOS native observation not tested.'));
  assert.equal(await page.locator('.report-card a').filter({hasText:'Open PR'}).getAttribute('href'),task.pr);
  await page.locator('[data-report=next]').click();assert((await page.locator('.report-card').innerText()).includes('Recommendation'));
  await page.locator('[data-report=previous]').click();assert((await page.locator('.report-card').innerText()).includes('Changed'));
  await page.locator('[data-report=accept]').click();await page.getByText('Result accepted.',{exact:true}).waitFor();assert.equal(await page.locator('[data-report=accept]').isEnabled(),false);
  await page.locator('[data-report=reopen]').click();await page.locator('[data-result-reason]').fill('Preserve the original receipt');await page.locator('[data-report=create]').click();
  await page.getByText('Read held draft #25',{exact:true}).waitFor();assert.equal(await page.evaluate(()=>sent.filter(m=>m.action==='reopen').at(-1).text),'Preserve the original receipt');
  await page.locator('[data-report=followup]').click();await page.locator('[data-result-reason]').fill('Document the receipt');await page.locator('[data-report=create]').click();await page.getByText('Read held draft #25',{exact:true}).waitFor();
  await page.evaluate(()=>fail=true);await page.locator('[data-report=reopen]').click();await page.locator('[data-result-reason]').fill('Keep the draft');await page.locator('[data-report=create]').click();await page.getByText('Board unavailable; acknowledgement unknown',{exact:true}).waitFor();assert.equal(await page.locator('.report-card').count(),1);
  assert(!await page.evaluate(()=>sent.some(m=>m.a==='reports'&&['list','open'].includes(m.action))),'list/open/move must make no board-read request');
  if(surface==='tray'){await page.locator('[data-report=back]').click();await page.locator('.head [data-a=home]').click();await page.locator('[data-a=view-list]').click();await page.locator('#taskq-popover [data-a=view-select][data-id=completed]').click();await page.locator('[data-completed]').waitFor();assert((await page.locator('[data-completed]').innerText()).includes(task.title));assert((await page.locator('[data-history-source]').innerText()).includes('Collection stale'),'unknown cached history retains its rows and reason');}
  if(surface==='tray'){await page.evaluate(()=>emptyHistory=true);await page.locator('[data-a=view-editor]').click();await page.locator('[data-a=view-period][data-value=all]').click();await page.locator('#taskq-popover').getByRole('button',{name:'Done',exact:true}).click();await page.waitForFunction(()=>sent.filter(m=>m.a==='completed').length>1);assert((await page.locator('[data-completed]').innerText()).includes(task.title),'empty unknown cache cannot erase previously displayed history');}
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Results wraps at '+surface+'/'+width+'/'+font);
  assert.deepEqual(errors,[]);cells++;await page.close();
 }
 console.log('PASS Results mixed navigation and actions: '+cells+' cells; Windows Chromium instrument, native qualification separate');
 // Closing the selected question must replace its row once without falling through to an owner-request renderer.
 const closed=await browser.newPage({viewport:{width:640,height:600}}),errors=[];closed.on('pageerror',e=>errors.push(e.message));await closed.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
 await closed.evaluate(s=>{window.ipc={postMessage(){}};s.projects[0].reports.reports=[];s.projects[0].tasks=[{n:13,title:'Recover interrupted requests once',state:'ask',revision:30}];taskq.receive(s);taskq.receive({open:{page:'conversations',root:'fixture',board:s.projects[0].issues,task:13}});},state('dark',16));
 await closed.evaluate(s=>taskq.receive(s),state('dark',16));await closed.getByText('Read result',{exact:true}).waitFor();assert.equal(await closed.locator('.conversation-row[data-n="13"]').count(),1);await closed.getByText('Read result',{exact:true}).click();await closed.locator('[data-report=reopen]').waitFor();assert.deepEqual(errors,[]);await closed.close();
 console.log('PASS selected question becomes one completed result without route failure');
 if(out&&baseline){fs.mkdirSync(out,{recursive:true});
  for(const surface of ['tray','workspace'])for(const theme of ['dark','light']){
   const page=await browser.newPage({viewport:{width:surface==='tray'?420:640,height:surface==='tray'?850:600}});
   await page.setContent(fs.readFileSync(path.join(baseline,surface==='tray'?'1.html':'0.html'),'utf8'));
   await page.evaluate(({s,t})=>{window.ipc={postMessage:text=>{const m=JSON.parse(text);if(m.a==='completed')queueMicrotask(()=>taskq.receive({completed:{...m,page:1,more:false,tasks:[{...t,state:'closed',outcome:'success',result:'Interrupted requests recover once.',merged:'merged #20; merge-result CI green'}]}}));}};delete s.projects[0].reports;taskq.receive(s);},{s:state(theme,16),t:task});
   if(surface==='tray'){await page.locator('[data-a=view-list]').click();await page.locator('#taskq-popover [data-a=view-select][data-id=completed]').click();await page.locator('[data-completed]').waitFor();}
   else if(await page.locator('[data-c=list]').isVisible())await page.locator('[data-c=list]').click();
   await page.screenshot({path:path.join(out,'before-windows-'+surface+'-'+theme+'.png')});await page.close();
  }
  console.log('PASS baseline screenshots: installed executable HTML, invented data, Windows Chromium; no board mutations');
 }
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
