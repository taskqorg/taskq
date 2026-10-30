// #1957 baseline-only capture harness for be3c20f3: the same invented completed task #1821 as the #1956 fixture,
// delivered through the baseline Completed history read. Invented receipts only; no board is read or written.
function taskqCompletedBefore(surface,state){
 const task={n:1821,state:'closed',title:'Recover interrupted requests once',url:'https://github.com/invented/research/issues/1821',closed:200,outcome:'success',close:'delivered',merged:'merged #1822 as '+'a'.repeat(7),result:'Interrupted requests reuse their original identity.',pr:'https://github.com/invented/research/pull/1822'};
 const s={...state,projects:[{...state.projects[0],root:'fixture',name:'Research',issues:'https://github.com/invented/research/issues',tasks:[],alive:true,complete:true}],conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
 const sink=window.webkit?.messageHandlers?.ipc||window.chrome?.webview||window.ipc,post=sink&&sink.postMessage.bind(sink);
 const stub=t=>{const m=JSON.parse(t);if(m.a!=='completed')return post&&post(t);
  setTimeout(()=>taskq.receive({completed:{root:m.root,id:m.id,page:1,more:false,tasks:[task]}}),0);};
 if(sink)sink.postMessage=stub;else window.ipc={postMessage:stub};
 delete s.page;taskq.receive(s);
 if(surface==='menu')document.querySelector('[data-a=view-select][data-id=completed]')?.click();
}
