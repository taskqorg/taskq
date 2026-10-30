// §7/R12 native Reports capture. All report/board receipts are invented; no report decision reaches a board.
function taskqReportShot(surface,state){
 const row={n:1820,title:'Retry limits for interrupted requests',sha:'a'.repeat(40),url:'https://github.com/invented/research/issues/1820',closed:200,read:false,decision:'none',visible:true};
 const body='# Question\n\nHow should interrupted requests recover?\n\n# Findings\n\nThe [recorded protocol](https://example.com/invented-evidence) exposes a stable request identity.\n\n# Options\n\nReuse the identity: avoids duplicate work; needs an immutable request.\nStart again: simple; may duplicate work.\n\n# Recommendation\n\nReuse the request identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```';
 const s={...state,projects:[{...state.projects[0],root:'fixture',name:'Research',issues:'https://github.com/invented/research/issues',tasks:[],reports:{complete:true,observed:200,reports:[{...row,body,tasks:[{title:'Document request recovery',goal:'Make recovery steps visible.',acceptance:['Help names the original request identity.'],type:'docs'}]}]},alive:true,complete:true}],conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
 const sink=window.webkit?.messageHandlers?.ipc||window.chrome?.webview||window.ipc,post=sink&&sink.postMessage.bind(sink);
 const stub=t=>{const m=JSON.parse(t);if(m.a!=='reports')return post&&post(t);
  const value=m.action==='list'?{reports:[row],more:false}:{report:{...row,read:true,body,tasks:[{title:'Document request recovery',goal:'Make recovery steps visible.',acceptance:['Help names the original request identity.'],type:'docs'}]}};
  setTimeout(()=>{taskq.receive({reportReceipt:{id:m.id,root:m.root,...value}});if(m.action==='list')document.querySelector('[data-report=open]')?.click();if(m.action==='read'&&surface==='conversations')setTimeout(()=>{const reader=document.querySelector('.reports-section');reader.scrollTop=reader.scrollHeight;},0);},0);
 };
 if(sink)sink.postMessage=stub;else window.ipc={postMessage:stub};
 delete s.page;taskq.receive(s);const toggle=document.querySelector('[data-report=toggle]');if(toggle?.getAttribute('aria-expanded')!=='true')toggle?.click();document.querySelector('.reports-section [data-report=open]')?.click();
}
