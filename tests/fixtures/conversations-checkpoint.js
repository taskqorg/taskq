// #2366 A2/A8: real native webview and bundle, invented IPC; no board/session/provider effects.
function taskqConversationsCheckpoint(state){
 const board='https://example.org/invented/issues',id='task:'+board+'/7',count=80;
 const sink=window.webkit?.messageHandlers?.ipc||window.chrome?.webview||window.ipc,post=sink.postMessage.bind(sink);
 const fail=reason=>post(JSON.stringify({a:'overflow',shot:'2366 '+reason,x:1}));
 sink.postMessage=text=>{
  const m=JSON.parse(text);if(m.a!=='conversation-dialogue')return post(text);
  let value;
  if(m.action==='history'){
   const start=Math.max(0,(m.before??10)-64),end=Math.min(10,start+64);
   value={revision:71,start,next:end,total:10,complete:true,rows:Array.from({length:end-start},(_,i)=>({question:null,answered:{text:'Owner note '+(start+i)+': retain source identity and reading context.',at:1700000000+(start+i)*2,source:{kind:'steer',role:'owner',id:start+i+1}}}))};
  }else if(m.action==='transcript'){
   const end=m.before??count,start=m.after??Math.max(0,end-64),stop=Math.min(count,m.after===undefined?end:start+64);
   value={generation:'invented-v1',start,end:stop,size:count,following:true,activity:'working',items:Array.from({length:stop-start},(_,i)=>({at:start+i,i:0,role:(start+i)%3===0?'user':'assistant',text:'Passage '+(start+i)+'. Keep the source paragraph visible while new material arrives.',updated:new Date((1700000001+(start+i)*2)*1000).toISOString(),model:'invented-model',version:'invented-cli'}))};
  }else if(m.action==='tasks')value={tasks:[],more:false};
  else return fail('unexpected action '+m.action);
  setTimeout(()=>taskq.receive({dialogueReceipt:{...m,state:'accepted',value}}),0);
 };
 const s={...state,host:'invented',projects:[{root:'fixture',name:'Invented checkpoint',issues:board,alive:true,complete:true,tasks:[{n:7,title:'Preserve a reading passage across late layout',state:'doing',role:'worker',attempt:71,revision:71,runtime:'codex',session:'codex:invented invented',local_session:true,host:'invented',executions:[{attempt:71,state:'running'}],history_count:10,history_lazy:true}]}],messages:{complete:true,entries:[]},conversations:{complete:true,items:[],navigation:{threads:{selected:id}}}};
 delete s.page;taskq.receive(s);taskq.receive({open:{page:'conversations',root:'fixture',board,task:7}});
 setTimeout(()=>{
  try{
   const reader=document.querySelector('.conversation-reader'),record=reader.querySelector('[data-key="attempt:71:40:0"]'),paragraph=record.querySelector('[data-native-part]');
   reader.dispatchEvent(new WheelEvent('wheel',{deltaY:-1,bubbles:true}));
   reader.scrollTop+=paragraph.getBoundingClientRect().top-reader.getBoundingClientRect().top-20;reader.dispatchEvent(new Event('scroll'));
   const range=document.createRange();range.selectNodeContents(paragraph);getSelection().removeAllRanges();getSelection().addRange(range);record.querySelector('.copy-block').focus({preventScroll:true});
   requestAnimationFrame(()=>{
    const y=paragraph.getBoundingClientRect().top-reader.getBoundingClientRect().top,text=getSelection().toString(),focus=document.activeElement;
    const spacer=document.createElement('div');spacer.style.height='120px';record.previousElementSibling.prepend(spacer);record.querySelector('.block-details').open=true;
    requestAnimationFrame(()=>requestAnimationFrame(()=>requestAnimationFrame(()=>{
     const drift=paragraph.getBoundingClientRect().top-reader.getBoundingClientRect().top-y;
     const value={drift,selection:getSelection().toString()===text,focus:document.activeElement===focus};
     if(Math.abs(drift)>2||!value.selection||!value.focus)fail('anchor/focus/selection drift '+drift);
     else post(JSON.stringify({a:'snapshot-checkpoint',state:'passed',value}));
    })));
   });
  }catch(error){fail(String(error));}
 },250);
}
