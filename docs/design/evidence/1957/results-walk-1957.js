// #1957 A3/A4 native walk for the #1956 candidate, run inside WKWebView by `tray --snapshot`.
// Mirrors tests/results.browser.cjs and tests/reports.browser.cjs; every receipt is invented and answered in the page,
// so no report decision or task creation reaches a board. The final screen lists each step as PASS or FAIL.
function taskqResultsWalk(surface,state){
 const sha='a'.repeat(40),out=[],t0=performance.now();
 const task={n:13,sha,kind:'task',title:'Recover interrupted requests once',url:'https://github.com/invented/research/issues/13',closed:210,read:false,visible:true,decision:'none',body:'Changed: Interrupted requests reuse their original identity.\nChecks: retry -- passed\nChecks: duplicate work -- prevented\nOpen: none.',verdict:'Checks: A1 ok; merge-result CI green',pr:'https://github.com/invented/research/pull/20'};
 const report={...task,n:12,kind:'report',title:'Request recovery research',closed:200,body:'# Question\n\nHow should interrupted requests recover?\n\n# Recommendation\n\nReuse the request identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```',tasks:[{title:'Document request recovery',goal:'Make recovery steps visible.',acceptance:['Help names the original request identity.'],type:'docs'}]};
 let rows=[task,report],fail=false;const sent=[];
 const project=rs=>({...state.projects[0],root:'fixture',name:'Research',issues:'https://github.com/invented/research/issues',tasks:[],reports:{complete:true,observed:state.now||0,reports:rs},alive:true,complete:true});
 const s={...state,projects:[project(rows)],conversations:{complete:true,items:[]},messages:{complete:true,count:0,entries:[]}};
 const sink=window.webkit?.messageHandlers?.ipc||window.chrome?.webview||window.ipc,post=sink&&sink.postMessage.bind(sink);
 const stub=text=>{const m=JSON.parse(text);if(m.a!=='reports'&&m.a!=='completed')return post&&post(text);sent.push(m);
  if(m.a==='completed'){setTimeout(()=>taskq.receive({completed:{...m,source:'service',tasks:[{...task,state:'closed',outcome:'success'}],reason:'Collection stale',page:1,more:false}}),0);return;}
  const prior=rows.find(r=>r.n===m.n);let value;
  if(m.action==='list')value={reports:rows.filter(r=>r.visible),more:false};
  else if(fail)value={reason:prior?.kind==='report'?'Board unavailable; decision unknown':'Board unavailable; acknowledgement unknown'};
  else{const row={...prior,read:true,decision:m.action==='accept'?'accepted':m.action==='make'?'made':m.action==='drop'?'drop':m.action==='later'?'later':prior?.decision,visible:!['make','drop'].includes(m.action),tasks_made:m.action==='make'?[21]:[]};
   rows=rows.map(r=>r.n===m.n?row:r);value={report:row,...(['reopen','followup'].includes(m.action)?{successor:25,successor_url:'https://github.com/invented/research/issues/25'}:{})};}
  setTimeout(()=>taskq.receive({reportReceipt:{id:m.id,root:m.root,...value}}),0);};
 if(sink)sink.postMessage=stub;else window.ipc={postMessage:stub};
 const $=q=>document.querySelector(q),text=q=>$(q)?.innerText||'';
 const until=(f,ms=400)=>new Promise((ok,no)=>{const end=performance.now()+ms;(function poll(){let v;try{v=f();}catch(e){}if(v)return ok(v);if(performance.now()>end)return no(Error('timeout'));setTimeout(poll,10);})();});
 const page=q=>until(()=>document.body.innerText.includes(q));
 const click=async q=>{(await until(()=>$(q))).click();};
 const fill=async v=>{const e=await until(()=>$('[data-result-reason]'));e.value=v;e.dispatchEvent(new Event('input',{bubbles:true}));};
 const step=async(name,f)=>{try{const r=await f();out.push((r===false?'FAIL ':'PASS ')+name);}catch(e){out.push('FAIL '+name+': '+e.message);}};
 const boardReads=()=>sent.filter(m=>m.a==='reports'&&['list','open'].includes(m.action)).length;
 (async()=>{
  delete s.page;taskq.receive(s);
  await step('unread badge/row',()=>surface==='menu'?until(()=>$('[data-a=workspace][data-page=conversations]')?.getAttribute('aria-label')?.includes('2 new')):($('[data-c=list]')?.offsetParent&&$('[data-c=list]').click(),until(()=>text('.conversation-rows').includes('New result'))));
  await step('cached list: 2 rows, Unread',async()=>{const t=$('[data-report=toggle]');if(t?.getAttribute('aria-expanded')!=='true')t?.click();await until(()=>document.querySelectorAll('.reports-section .report-row').length===2);return text('.reports-section').includes('Unread');});
  await step('open result #13',async()=>{await click('.reports-section [data-n="13"]');await until(()=>$('[data-report=accept]'));return text('.report-card').includes('duplicate work -- prevented');});
  await step('next/previous',async()=>{await click('[data-report=next]');await until(()=>text('.report-card').includes('Recommendation'));await click('[data-report=previous]');return until(()=>text('.report-card').includes('Changed'));});
  await step('Accept',async()=>{await click('[data-report=accept]');await page('Result accepted.');return $('[data-report=accept]').disabled;});
  await step('Reopen keeps reason',async()=>{await click('[data-report=reopen]');await fill('Preserve the original receipt');await click('[data-report=create]');await page('Read held draft #25');return sent.filter(m=>m.action==='reopen').at(-1)?.text==='Preserve the original receipt';});
  await step('Follow-up',async()=>{await click('[data-report=followup]');await fill('Document the receipt');await click('[data-report=create]');return page('Read held draft #25');});
  await step('Reopen unavailable stays unknown',async()=>{fail=true;await click('[data-report=reopen]');await fill('Keep the draft');await click('[data-report=create]');await page('Board unavailable; acknowledgement unknown');fail=false;return document.querySelectorAll('.report-card').length===1;});
  await step('seen state after back',async()=>{await click('[data-report=back]');const row=await until(()=>text('.reports-section [data-n="13"]'));return row.includes('Read')&&!row.includes('Unread');});
  await step('report Later',async()=>{await click('.reports-section [data-n="12"]');await until(()=>$('[data-report=later]'));await click('[data-report=later]');return page('Kept in Reports for later.');});
  await step('report Make unavailable keeps card',async()=>{fail=true;await click('[data-report=make]');await page('Board unavailable; decision unknown');fail=false;return document.querySelectorAll('.report-card').length===1;});
  await step('report Make creates held #21',async()=>{await click('[data-report=make]');return page('Created held drafts: #21');});
  await step('report Drop',async()=>{rows=[...rows,{...report,n:14,visible:true,decision:'none',read:false}];taskq.receive({...s,projects:[project(rows)]});await until(()=>$('.reports-section [data-n="14"]'));await click('.reports-section [data-n="14"]');await until(()=>$('[data-report=drop]'));await click('[data-report=drop]');return page('Report dropped.');});
  await step('zero board list/open requests',()=>boardReads()===0);
  await step('unavailable collection names reason',async()=>{await click('[data-report=back]').catch(()=>{});taskq.receive({...s,projects:[{...project([]),reports:{complete:false,reports:[],reason:'Report collection is stale; waiting for the service'}}]});return page('Research: Report collection is stale; waiting for the service');});
  if(surface==='menu')await step('stale Completed keeps rows and reason',async()=>{await click('[data-report=back]').catch(()=>{});await click('[data-a=view-list]');await click('#taskq-popover [data-a=view-select][data-id=completed]');await until(()=>$('[data-completed]'));return text('[data-completed]').includes(task.title)&&text('[data-history-source]').includes('Collection stale');});
 })().finally(()=>{const d=document.createElement('pre');d.id='walk-1957';
  d.style.cssText='position:fixed;inset:0;margin:0;padding:8px;z-index:2147483647;background:#fff;color:#000;font:11px/1.35 ui-monospace,monospace;white-space:pre-wrap;overflow:hidden';
  const pass=out.filter(l=>l.startsWith('PASS')).length;
  d.textContent='#1957 native '+surface+' walk · candidate #1956 · invented receipts\n'+out.join('\n')+'\n'+pass+'/'+out.length+' PASS · board list/open requests: '+boardReads()+' · sent: '+sent.map(m=>m.a+(m.action?':'+m.action:'')).join(' ')+' · '+Math.round(performance.now()-t0)+' ms';
  document.body.append(d);});
}
