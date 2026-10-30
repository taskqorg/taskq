// §7/R12 native Reports capture. All report/board receipts are invented; no report decision reaches a board.
function taskqReportShot(surface,state,ordinary=false){
 const row={n:1820,title:'Retry limits for interrupted requests',sha:'a'.repeat(40),url:'https://github.com/invented/research/issues/1820',closed:200,read:false,decision:'none',visible:true};
 const body='# Question\n\nHow should interrupted requests recover?\n\n# Findings\n\nThe [recorded protocol](https://example.com/invented-evidence) exposes a stable request identity.\n\n# Options\n\nReuse the identity: avoids duplicate work; needs an immutable request.\nStart again: simple; may duplicate work.\n\n# Recommendation\n\nReuse the request identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```';
 const report={...row,body,tasks:[{title:'Document request recovery',goal:'Make recovery steps visible.',acceptance:['Help names the original request identity.'],type:'docs'}]};
 const task={...row,n:1821,title:'Recover interrupted requests once',kind:'task',body:'Changed: Interrupted requests reuse their original identity.\nChecks: replay -- passed\nChecks: duplicate work -- prevented\nOpen: macOS native observation not tested.',verdict:'Changed: Delivered recovery.\nChecks: A1 ok; merge-result CI green',pr:'https://github.com/invented/research/pull/1822'};
 const content=ordinary?task:report;
 const s={...state,projects:[{...state.projects[0],root:'fixture',name:'Research',issues:'https://github.com/invented/research/issues',tasks:[],reports:{complete:true,observed:200,reports:ordinary?[task,report]:[report]},alive:true,complete:true}],conversations:{complete:true,items:[]},messages:{complete:true,count:0,entries:[]}};
 const sink=window.webkit?.messageHandlers?.ipc||window.chrome?.webview||window.ipc,post=sink&&sink.postMessage.bind(sink);
 const stub=t=>{const m=JSON.parse(t);if(m.a!=='reports')return post&&post(t);
  const value=m.action==='list'?{reports:[content],more:false}:{report:{...content,read:true}};
  setTimeout(()=>{taskq.receive({reportReceipt:{id:m.id,root:m.root,...value}});if(m.action==='list')document.querySelector('[data-report=open]')?.click();if(m.action==='read'&&surface==='conversations')setTimeout(()=>{const reader=document.querySelector('.reports-section');reader.scrollTop=reader.scrollHeight;},0);},0);
 };
 if(sink)sink.postMessage=stub;else window.ipc={postMessage:stub};
 delete s.page;taskq.receive(s);const toggle=document.querySelector('[data-report=toggle]');if(toggle?.getAttribute('aria-expanded')!=='true')toggle?.click();document.querySelector('.reports-section [data-report=open]')?.click();
}
function taskqResultShot(surface,state){taskqReportShot(surface,state,true);}
