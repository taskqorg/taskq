// Invented #1964 owner scenario. Native snapshots and Chromium use the same data and intercept every write.
function taskqNotificationsFixture(state){
 const board='https://github.com/invented/cadence/issues',root='fixture',now=state.now||1000;
 const results=Array.from({length:10},(_,i)=>({n:100+i,sha:(state.notificationFixtureSeed||'a').repeat(39)+i.toString(16),kind:'task',title:'Cadence v4 '+(i+1)+'/10',url:board+'/'+(100+i),closed:now-100+i,read:false,visible:true,body:'Changed: Delivered series item '+(i+1)+'.\nChecks: fixture passed\nOpen: none'}));
 const tasks=[{n:20,title:'Choose the rollout window',state:'ask',revision:30,card:'Problem: Choose a rollout window.',question:{parsed:true,problem:'Choose a rollout window.',options:[{n:1,text:'After review'},{n:2,text:'Tomorrow'}]},controls:[]},{n:21,title:'Keep current work visible',state:'doing',revision:31,runtime:'codex',host:'fixture',controls:[]}];
 const counts=state.display.states.map(({id,name})=>({state:id,name,n:tasks.filter(t=>t.state===id).length}));
 return {...state,host:'fixture',display:{...state.display,all:{counts,partial:[],open:2}},page:'home',size:[320,750],size_anchor:'fixed',projects:[{...state.projects[0],root,name:'Cadence',issues:board,board_kind:'github',host:'fixture',mode:'run',focus:['all'],partial:[],counts,open:2,observed:now,tasks_age_seconds:0,poll:60,age_text:'0 s',hosts:[{name:'fixture',updated:now,live:true}],tasks,events:[[now-50,'ask','30',20],[now-120,'gone','31',21],...results.map(r=>[r.closed,'close',r.sha,r.n])],events_error:null,reports:{complete:true,reports:results},error:null,alive:true,complete:true,tasks_live:true,age:0}],messages:{complete:true,count:1,entries:[]},notify:{...state.notify,inbox:{}},stop_modes:false,conversations:{complete:true,items:[]}};
}
function taskqNotificationsInstall(state){
 const s=taskqNotificationsFixture(state),sink=window.webkit?.messageHandlers?.ipc||window.chrome?.webview||window.ipc,post=sink&&sink.postMessage.bind(sink);
 window.notificationFixture=s;window.notificationSent=[];window.notificationFail=false;window.notificationDelayed=false;window.notificationReplies=[];
 const stub=text=>{const m=JSON.parse(text);notificationSent.push(m);
  if(m.a==='notification-state'){
   if(!notificationFail)for(const id of m.ids)if(s.notify.inbox[id]!=='cleared')s.notify.inbox[id]=m.action==='clear'?'cleared':'read';
   queueMicrotask(()=>{taskq.receive({...s,page:undefined});taskq.receive({notificationReceipt:{reason:notificationFail?'Notification state was not saved; previous state retained':null}});});return;
  }
  if(m.a==='reports'){
   const row=s.projects[0].reports.reports.find(r=>r.n===m.n);
   const reply=()=>{if(!notificationFail)row.read=true;taskq.receive({reportReceipt:{id:m.id,root:m.root,...(notificationFail?{reason:'Board unavailable; acknowledgement unknown'}:{report:{...row}})}});};
   if(notificationDelayed)notificationReplies.push(reply);else queueMicrotask(reply);return;
  }
  // Snapshot navigation/settings/telemetry stay native; no fixture command reaches a board or real task.
  if(!['answer','workspace','completed'].includes(m.a))post?.(text);
 };
 if(sink)sink.postMessage=stub;else window.ipc={postMessage:stub};taskq.receive(s);return s;
}
function taskqNotificationsShot(scene,state){
 const seed=(state.theme==='light'?'5678':'1234')[['home','panel','expanded','result'].indexOf(scene)];
 taskqNotificationsInstall({...state,notificationFixtureSeed:seed});
 if(scene!=='home')document.querySelector('[data-route=notifications]')?.click();
 if(scene==='expanded')document.querySelector('[data-notification-group]>summary')?.click();
 if(scene==='result'){document.querySelector('[data-notification-group]>summary')?.click();document.querySelector('[data-notification-group] [data-notification=open]')?.click();}
}
