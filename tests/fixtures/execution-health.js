// A2/A3/A6: invented data in the actual native renderer; no board/session operation.
async function taskqExecutionHealthShot(phase,state){
 const q=s=>document.querySelector(s),check=(v,text)=>{if(!v)throw new Error(text);};
 const now=Math.floor(Date.now()/1000),board='https://example.org/invented/health/issues',root='execution-health-fixture';
 const beat=(status,reason)=>({host:'fixture-mac',attempt:55,source:'board host heartbeat',observed:now-5,age_s:5,ttl_s:300,freshness:'fresh',reason:'Matching ownership lease; progress is separate',warning:status==='overdue'?reason:null,
  progress:{source:'native execution events',clock:'append mtime lower bound',observed:now-3,last_event:now-1100,tool_since:now-1100,pending_tools:status==='quiet'?0:1,status,reason}});
 const overdue=beat('overdue','Tool progress overdue; preserve execution and inspect owning session');
 const task={n:55,state:'doing',title:'Inspect the unbounded diagnostic tool',attempt:55,role:'worker',runtime:'antigravity',host:'fixture-mac',session:'antigravity:invented fixture-mac',revision:55,since:now-1200,age:'20m',url:board+'/55',heartbeat:overdue,controls:[]};
 const healthy={...task,n:56,title:'Quiet work with a fresh observation',heartbeat:beat('quiet','No pending tool; quiet work is not failure')};
 const s={...state,host:'fixture-mac',projects:[{root,name:'Health fixture',host:'fixture-mac',issues:board,board_kind:'github',alive:true,complete:true,tasks_live:true,age:0,focus:[],labels:[],tasks:[task,healthy]}],messages:{complete:true,count:0,entries:[]},conversations:{complete:true,items:[]},workspace_local:{}};
 if(phase==='conversation'){
  delete s.page;taskq.receive(s);taskq.receive({open:{page:'conversations',root,board,task:55}});
  await new Promise(r=>setTimeout(r,100));
  const info=q('.conversation-info');check(info,'native task info exists');info.open=true;
  check(q('.task-health'),'Conversations carries execution health');return;
 }
 s.page='home';taskq.receive(s);await new Promise(r=>setTimeout(r,100));
 q('#taskq-question-details [data-a=popup-close]')?.click();
 const trigger=q('[data-a=task-detail][data-key="'+board+'/55"]');check(trigger,'native doing row exists');
 const row=trigger.closest('[data-card]'),height=row.getBoundingClientRect().height;
 const warning=row.querySelector('.task-health-warning');check(warning?.getAttribute('aria-label'),'native local warning has accessible reason');
 if(phase==='recovered'){
  task.heartbeat=beat('quiet','No pending tool; quiet work is not failure');taskq.receive(s);await new Promise(r=>setTimeout(r,100));
  check(row.getBoundingClientRect().height===height,'native recovery preserves row geometry');check(row.querySelector('.task-health-warning').getAttribute('aria-hidden')==='true','native recovery clears local warning');
 }else if(phase==='detail'){
  trigger.click();await new Promise(r=>setTimeout(r,100));
  const detail=q('#taskq-question-details');check(detail?.querySelector('.task-health'),'native inline details carry execution health');detail.scrollIntoView({block:'start'});
 }else {warning.focus();check(getComputedStyle(warning.querySelector('.task-health-tip')).display!=='none','native focus shows textual diagnostic');}
}
