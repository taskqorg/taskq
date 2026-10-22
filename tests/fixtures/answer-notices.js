// Snapshot-only (#1148, menu.md M3): synthetic tasks on Attention; four Later answers get one unknown and three accepted
// receipts. No board operation: snapshot mode does not dispatch answers. A failed check rejects, which faults the
// snapshot (exit 1). `phase`: toast, expanded, dismissed or expired.
window.taskqAnswerNotices=async phase=>{
  const q=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)],check=(ok,what)=>{if(!ok)throw new Error(what);};
  const tick=()=>new Promise(r=>setTimeout(r,0));
  // leftovers of the previous step; each click redraws the menu
  for(let b;(b=q('[data-a=answer-dismiss],[data-a=toast-dismiss]'));)b.click();
  const board='https://github.com/o/r/issues',root='project',now=Math.floor(Date.now()/1000);
  // each step answers fresh questions: an answered card stays disabled for its task
  const k=(window.taskqAnswerStep=(window.taskqAnswerStep||0)+1)*100;
  const t=(n,state,title,since,extra={})=>({n:n+k,state,title,revision:1,url:board+'/'+n,since,...extra});
  const ask=(n,title)=>t(n,'ask',title,now-600,{card:title,question:{parsed:false,text:title,choices:[]}});
  const state={page:'attention',now,theme:document.documentElement.style.colorScheme,font:parseInt(q('[style*="--size"]')?.style.getPropertyValue('--size'))||16,host:'mac',version:'dev',platform:'macos',icons:{},
    projects:[{root,name:'demo',issues:board,alive:true,complete:true,age:5,focus:['all'],labels:[],closed:3,
      tasks:[ask(1101,'Old question A'),ask(1102,'Old question B'),ask(1103,'Old question C'),ask(1104,'Old question D'),t(1110,'ask','Pick the release window',now-7200),t(1111,'doing','Fix tray resize',now-300),t(1112,'ready','Write release notes',null),t(1113,'failed','Flaky check',now-259200)],cap_saved:{error:'not configured'}}],
    ai:{draft:''},notify:{projects:{}}};
  taskq.receive(state);
  // the answer card names each operation with crypto.randomUUID; the native bridge object is read-only
  const sent=[];
  crypto.randomUUID=()=>{const id='fixture-'+k+'-'+sent.length;sent.push({id});return id;};
  for(const b of all('[data-qac=later]').slice(0,4)){b.click();await tick();}
  await tick();
  delete crypto.randomUUID;
  check(sent.length===4,'four Later answers sent, got '+sent.length);
  const top=()=>q('.tqp-age')?.getBoundingClientRect().top;
  const before=top();
  state.projects[0].tasks=state.projects[0].tasks.map(x=>x.state==='ask'&&x.n<1110+k?{...x,state:'later',since:now-5}:x);
  taskq.receive(state);
  const timers=[],real=window.setTimeout;
  if(phase==='expired')window.setTimeout=(f,ms,...a)=>{if(ms===6000){timers.push(f);return 0;}return real(f,ms,...a);};
  for(const [i,op] of sent.entries())taskq.receive({answerReceipt:{root,id:op.id,state:i===0?'unknown':'accepted',reason:i===0?'No bridge receipt received':'',next:i===0?'Check delivery with the same operation':''}});
  window.setTimeout=real;
  const answered=q('details.answered');
  check(answered&&!answered.open,'Answered starts collapsed');
  check(answered.querySelectorAll('[data-a=answer-dismiss]').length===3,'Answered holds three accepted receipts');
  check(all('[data-a=answer-retry]').length===1,'unknown receipt keeps Check delivery');
  check(q('.toast'),'accepted answer shows a toast');
  check(top()===before,'rows keep their place: '+before+' then '+top());
  if(phase==='expanded'){answered.open=true;}
  if(phase==='dismissed'){
    q('[data-a=toast-dismiss]').click();
    q('details.answered [data-a=answer-dismiss]').click();
    check(!q('.toast'),'toast dismissed');
    check(q('details.answered').querySelectorAll('[data-a=answer-dismiss]').length===2,'one Answered receipt dismissed');
    q('details.answered').open=true;
  }
  if(phase==='expired'){
    check(timers.length===3,'one 6 s timer per accepted receipt, got '+timers.length);
    timers.forEach(f=>f());
    check(!q('.toast'),'toast expires after 6 s');
    check(q('details.answered').querySelectorAll('[data-a=answer-dismiss]').length===3,'expiry keeps Answered');
  }
  check(top()===before,'rows keep their place after '+phase);
};
