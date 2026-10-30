// Chain: §7 Reports -> Menu/Conversations buttons -> board receipts -> visible report. Invented IPC, real bundles.
// Risk: one surface drifts, unavailable writes hide a report, or narrow/large-text cards lose actions.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/task-workspace.json'),out=process.env.TASKQ_REPORT_EVIDENCE;
const root='fixture',sha='a'.repeat(40),row={n:12,sha,title:'Retry limits for interrupted requests',url:'https://github.com/invented/research/issues/12',closed:200,read:false,decision:'none',visible:true};
const report={...row,read:false,body:'# Question\n\nHow should interrupted requests recover?\n\n# Findings\n\nThe [recorded protocol](https://example.com/invented-evidence) exposes a stable request identity.\n\n# Options\n\n1. Reuse the identity. Pro: no duplicate work. Con: requires an immutable request.\n2. Start again. Pro: simple. Con: may duplicate work.\n\n# Recommendation\n\nReuse the request identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```',tasks:[{title:'Document request recovery',goal:'Make recovery steps available to operators.',acceptance:['Help names the original request identity.'],type:'docs'}]};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());let cells=0;try{
 for(const surface of ['workspace','tray'])for(const theme of ['dark','light'])for(const width of [350,940])for(const font of [16,20]){
  const page=await browser.newPage({viewport:{width,height:surface==='workspace'?480:850}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));
  const state={...fx.state,font,theme,projects:[{...fx.state.projects[0],root,name:'Research',issues:'https://github.com/invented/research/issues',tasks:[],reports:{complete:true,observed:200,reports:[report]}}],messages:{complete:true,count:0,entries:[]},conversations:{complete:true,items:[]},ai:{history:[],runtime:{id:'invented',name:'Fixture',available:false}}};
  await page.evaluate(({state,row,report})=>{window.sent=[];window.reportRow=report;window.reportData=report;window.failReportAction=false;window.ipc={postMessage:text=>{const m=JSON.parse(text);sent.push(m);if(m.a!=='reports')return;let value;
   if(m.action==='list')value={reports:reportRow.visible?[reportRow]:[],more:false};
   else if(failReportAction)value={reason:'Board unavailable; decision unknown'};
   else {reportRow={...reportRow,read:true,decision:m.action==='make'?'made':m.action==='drop'?'drop':m.action==='later'?'later':reportRow.decision,visible:!['make','drop'].includes(m.action),tasks_made:m.action==='make'?[21]:[]};value={report:m.action==='open'?{...reportData,...reportRow}:reportRow};}
   queueMicrotask(()=>taskq.receive({reportReceipt:{id:m.id,root:m.root,...value}}));}};taskq.receive(state);},{state,row,report});
  assert.equal(await page.evaluate(()=>sent.filter(m=>m.a==='reports').length),0,'pushed reports need no list request');
  if(surface==='workspace'){if(await page.locator('[data-c=list]').isVisible())await page.locator('[data-c=list]').click();assert((await page.locator('.conversation-rows').innerText()).includes('Needs decision'));}else {assert((await page.locator('[data-a=workspace][data-page=conversations]').getAttribute('aria-label')).includes('0 new'));await page.locator('[data-route=notifications]').click();}
  if(surface==='workspace')await page.locator('[data-report=toggle]').click();await page.locator(surface==='tray'?'.notification-row':'.reports-section .report-row').waitFor({state:'attached',timeout:5000}).catch(async e=>{console.error({surface,theme,width,font,errors,text:await page.locator('.reports-section').innerText(),sent:await page.evaluate(()=>sent.filter(m=>m.a==='reports'))});throw e;});
  assert((await page.locator(surface==='tray'?'.notification-row':'.reports-section .report-row').innerText()).includes('Unread'));
  if(out&&surface==='tray'&&theme==='dark'&&width===350&&font===16){fs.mkdirSync(out,{recursive:true});await page.screenshot({path:path.join(out,'menu-unread.png')});}
  await page.locator(surface==='tray'?'[data-notification=open]':'.reports-section [data-report=open]').click();await page.locator('.report-card').waitFor();
  assert.equal(await page.locator('[data-report=make]').isEnabled(),true);await page.waitForFunction(()=>document.querySelector('.report-card').innerText.includes('Read'));assert(!await page.evaluate(()=>sent.some(m=>m.a==='reports'&&['list','open'].includes(m.action))),'navigation makes no board read');
  assert.equal(await page.locator('.report-body').count(),0,'findings stay behind Full report');
  assert((await page.locator('.report-card').innerText()).indexOf('Recommendation')<(await page.locator('.report-card').innerText()).indexOf('Proposed tasks'));
  await page.locator('[data-report=full]').click();assert.equal(await page.locator('.report-body a').getAttribute('href'),'https://example.com/invented-evidence');
  if(surface==='workspace'){
   assert(await page.evaluate(()=>document.querySelector('.reports-section').getBoundingClientRect().bottom<=document.querySelector('.status-footer').getBoundingClientRect().top+1),'the report reader must fit above the fixed footer');
   await page.locator('[data-report=drop]').scrollIntoViewIfNeeded();
   await page.waitForFunction(()=>{const b=document.querySelector('[data-report=drop]').getBoundingClientRect(),footer=document.querySelector('.status-footer').getBoundingClientRect();return b.top>=0&&b.bottom<=footer.top;},null,{timeout:3000});
  }
  assert.equal(await page.getByRole('heading',{name:'Proposed tasks',exact:true}).count(),1,'structured proposals have one heading');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth&&[...document.querySelectorAll('.reports-section button')].every(e=>e.scrollWidth<=e.clientWidth+1)),'Reports must wrap at '+surface+'/'+width+'/'+font);
  if(out&&surface==='workspace'&&theme==='light'&&width===940&&font===16)await page.screenshot({path:path.join(out,'conversations-report.png')});
  await page.locator('[data-report=later]').click();await page.getByText('Kept in Reports for later.',{exact:true}).waitFor();
  await page.locator('[data-report=back]').click();assert((await page.locator(surface==='tray'?'.notification-row':'.reports-section .report-row').innerText()).includes('Read'));
  await page.locator(surface==='tray'?'[data-notification=open]':'.reports-section [data-report=open]').click();await page.locator('.report-card').waitFor();
  await page.evaluate(()=>failReportAction=true);await page.locator('[data-report=make]').click();await page.getByText('Board unavailable; decision unknown',{exact:true}).waitFor();
  assert.equal(await page.locator('.report-card').count(),1,'unknown creation retains the report');
  if(out&&surface==='tray'&&theme==='light'&&width===350&&font===20)await page.screenshot({path:path.join(out,'make-unknown.png')});
  await page.evaluate(()=>failReportAction=false);await page.locator('[data-report=make]').click();await page.getByText('Created held drafts: #21',{exact:true}).waitFor();
  assert.equal(await page.locator('.reports-section .report-row').count(),0);
  await page.evaluate(({state,report})=>{reportRow={...report,n:13};taskq.receive({...state,projects:[{...state.projects[0],reports:{complete:true,observed:201,reports:[reportRow]}}]});},{state,report});await page.locator(surface==='tray'?'.notification-row':'.reports-section .report-row').waitFor();
  await page.locator(surface==='tray'?'[data-notification=open]':'.reports-section [data-report=open]').click();await page.locator('.report-card').waitFor();await page.locator('[data-report=drop]').click();await page.getByText('Report dropped.',{exact:true}).waitFor();assert.equal(await page.locator('.reports-section .report-row').count(),0);
  if(surface==='workspace'){await page.locator('[data-report=toggle]').click();assert(await page.locator('.conversations').isVisible(),'closing Reports restores Conversations');}
  assert.deepEqual(errors,[]);cells++;await page.close();
 }
 console.log('PASS Reports buttons and theme/width/text matrix: '+cells+' cells; Chromium instrument, native platforms separate');
 // Reuse the native fixture with a WebView2-shaped sink. Risk: a capture silently leaves Reports collapsed.
 for(const surface of ['tray','workspace']){
  const page=await browser.newPage({viewport:{width:640,height:480}});await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));
  await page.evaluate(state=>{window.chrome={webview:{postMessage(t){if(JSON.parse(t).a==='reports')throw Error('fixture reached native IPC');}}};window.ipc=Object.freeze({postMessage:t=>window.chrome.webview.postMessage(t)});taskq.receive(state);},fx.state);
  await page.evaluate(fs.readFileSync('tests/fixtures/reports-shots.js','utf8')+'taskqReportShot('+JSON.stringify(surface)+','+JSON.stringify(fx.state)+')');
  await page.locator('.report-card').waitFor({timeout:5000});assert((await page.locator('.report-card').innerText()).includes('Retry limits'));
  assert.equal(await page.locator('[data-report=make]').isEnabled(),true);await page.close();
 }
 console.log('PASS native Reports fixture invocation and WebView2-shaped IPC on both surfaces');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
