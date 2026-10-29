// Chain: tray shared card -> IPC -> canonical dispatcher receipt. Risk: a lost response changes operation identity,
// a foreign receipt accepts an answer, or stale project data grants controls. Native interaction is a separate gate.
const fs=require('node:fs'),assert=require('node:assert/strict');
const card=require('../src/answer-card.js');
const sent=[],timers=new Map();let id=0;
const board='https://github.com/o/r/issues',project={root:'project',issues:board,alive:true,complete:true};
const context={S:{projects:[project],font:16},send:m=>sent.push({...m,local:undefined}),fit:()=>{},render:()=>{},menu:{scrollHeight:400},esc:String,
  setTimeout:(f,ms)=>{timers.set(++id,{f,ms});return id;},clearTimeout:i=>timers.delete(i)};
Object.assign(context,require('../src/workspace-answers.js').create(context,{menu:context.menu,document:context.document||{},send:m=>context.send(m),draw:()=>{},defer:(f,ms)=>context.setTimeout(f,ms),cancel:id=>context.clearTimeout(id),steer:require('../src/pm-composer.js').composer}));
const {html}=require('./tray-harness.cjs'),call=(name,...args)=>{const r=context[name](...args);return r===null||Array.isArray(r)||r?.props?html(r):r;};
const plain=x=>JSON.parse(JSON.stringify(x));
(async()=>{
  assert.equal(call('answerCapability',board).answer,true);
  for(const state of ['doing','review','ready','later','waiting','blocked']) assert.equal(call('answerSlot',{state,key:'task',card:'Old question',question:{parsed:true}}),'',state+' must not mount an answer card');
  assert(call('answerSlot',{state:'ask',key:'task'}).includes('data-answer'));
  project.complete=false;assert.equal(call('answerCapability',board).answer,false);
  project.complete=true;project.alive=false;assert.equal(call('answerCapability',board).answer,false);project.alive=true;
  assert.equal(call('answerCapability','other-board').answer,false);
  context.S.projects=[{...project,root:'same-board-other-root',alive:false},project];context.answer={board,root:project.root};assert.equal(call('answerCapability',board).answer,true,'same board does not substitute another project root');context.S.projects=[project];
  const op={id:'operation-1',task:841,revision:4,text:'literal 2'};
  const first=call('answerSend',board,op);
  assert.equal(call('answerSend',board,op),first,'duplicate bridge request shares its promise');
  assert.deepEqual(plain(sent),[{a:'answer',root:'project',board,op}]);
  call('answerReceipt',{root:'other',id:op.id,state:'accepted'});
  call('answerReceipt',{root:'project',id:'other',state:'accepted'});
  assert.equal(timers.size,1,'foreign receipt leaves request pending');
  [...timers.values()][0].f();
  assert.equal((await first).state,'unknown','timeout never accepts');
  const retry=call('answerSend',board,op);
  assert.deepEqual(plain(sent[1].op),op,'reconciliation keeps exact op ID, revision and literal text');
  call('answerReceipt',{root:'project',id:op.id,state:'accepted',reason:'read back',next:''});
  assert.deepEqual(plain(await retry),{state:'accepted',reason:'read back',next:''});
  // A1/A2 #1024: a refresh past the op's question reconciles once with the same op; only its receipt decides the state.
  for(const [move,state] of [[{revision:5},'accepted'],[{state:'ready'},'stale']]){
    const o={...op,id:'readback-'+state},before=sent.length,pending=call('answerSend',board,o);
    project.tasks=[{n:841,state:'ask',revision:4}];call('answerReadback');
    assert.equal(sent.length,before+1,'unchanged question: no reconcile');
    project.tasks=[{n:841,state:'ask',revision:4,...move}];call('answerReadback');call('answerReadback');
    assert.deepEqual(plain(sent.slice(before+1)),[{a:'answer',root:'project',board,op:o}],'one same-op reconcile');
    call('answerReceipt',{root:'project',id:o.id,state});
    assert.equal((await pending).state,state,'refresh alone never accepts');
  }
  const act={...op,id:'readback-later',action:'later'},before=sent.length;call('answerSend',board,act);
  project.tasks=[{n:841,state:'later',revision:5}];call('answerReadback');
  assert.equal(sent.length,before+1,'fallback action keeps Check delivery, no readback reconcile');
  call('answerReceipt',{root:'project',id:act.id,state:'accepted'});
  delete project.tasks;
  context.S.projects=[];
  assert.equal((await call('answerSend',board,op)).state,'rejected');
  assert.equal(sent.length,7,'unregistered board writes nothing');
  context.S.projects=[project];project.tasks=[{n:841,state:'ask',revision:4}];
  const late={...op,id:'late-native-receipt'},latePromise=call('answerSend',board,late),lateSent=sent.length;
  [...timers.values()].find(t=>t.ms===30000).f();assert.equal((await latePromise).state,'unknown');
  project.tasks=[];
  call('answerReceipt',{root:'other',id:late.id,state:'accepted',reason:'foreign'});
  call('answerReceipt',{root:'project',id:late.id,state:'accepted',reason:'late native receipt'});
  assert(call('answerNotices').includes('late native receipt'),'late receipt must replace timeout without a retry');
  assert(!call('answerNotices').includes('Check delivery'));
  assert.equal((await latePromise).state,'unknown','an already settled promise stays one-shot');
  assert.equal(sent.length,lateSent,'late receipt must not resend the answer');
  // #1148 M3: an accepted receipt toasts for 6 s and joins a bounded, dismissible Answered group in Attention.
  assert(call('answerToast').includes('#841 · answer accepted'));
  [...timers.values()].filter(t=>t.ms===6000).at(-1).f();assert.equal(call('answerToast'),'','the toast expires');
  for(let i=0;i<25;i++){const o={...op,id:'bound-'+i};call('answerSend',board,o);call('answerReceipt',{root:'project',id:o.id,state:'accepted'});}
  assert.equal(call('answerNotices').split('· accepted').length-1,20,'Answered keeps the newest 20');
  assert(call('answerNotices').includes('data-a="answer-dismiss"'));
  // #1785 A1/A2: losing the answer race showed a red stale-revision error with CLI advice; it now names who answered
  // and what, and once the refresh reads the worker running, offers the unsent answer as steer.
  project.tasks=[{n:900,state:'ask',revision:7}];
  const lost={id:'race-1',task:900,revision:7,text:'use y'};
  call('save',board,{n:900},{draft:'use y',op:lost});
  const lostPromise=call('answerSend',board,lost);
  call('answerReceipt',{root:'project',id:lost.id,state:'accepted',note:true,reason:'Saved as a task note; queued to steer the worker',next:'',answered:{by:'alex',text:'use x'}});
  assert.equal((await lostPromise).state,'accepted');
  assert(call('answerToast').includes('task note saved'),'a note is not shown as an accepted answer');
  const running={n:900,state:'doing',revision:8};project.tasks=[running];
  const thread=call('answerWon',board,running)+call('answerSteerComposer',board,running)+call('answerNotices',board,900);
  assert(thread.includes('Answered by alex: use x')&&thread.includes('Saved as a task note')&&!thread.includes('data-a="answer-steer"'),thread);
  assert(!/revision|taskq|alert/.test(thread),thread);
  assert(call('answerNotices').includes('#900 · Answered by alex: use x'),'Attention names the winner too');
  // #1861 A2/A4: the owner repeating the PM's answer got a duplicate note, a red error and the option twice.
  // The same answer is now a quiet no-op receipt that names the winner once; a stale control reads amber, not red.
  const same={id:'race-same',task:900,revision:7,text:'use x'};
  const samePromise=call('answerSend',board,same);
  call('answerReceipt',{root:'project',id:same.id,state:'accepted',reason:'Same answer; nothing saved',next:'',answered:{by:'alex',text:'use x'}});
  assert.equal((await samePromise).state,'accepted');
  assert(call('answerToast').includes('#900 · already answered'),call('answerToast'));
  const sameLine=call('answerNotices').split('<div').find(l=>l.includes('Same answer'))||'';
  assert(sameLine.includes('#900 · Answered by alex: use x · Same answer; nothing saved'),sameLine);
  assert(!/already answered|alert/.test(sameLine),'the winner is named once, never red: '+sameLine);
  const stale={id:'race-stale',task:900,revision:7,action:'start'};
  call('answerSend',board,stale);
  call('answerReceipt',{root:'project',id:stale.id,state:'stale',reason:'',next:''});
  const notice=id=>call('answerNotices').split('<div').find(l=>l.includes('project#'+id))||'';
  const staleLine=notice(stale.id);
  assert(staleLine.includes('task changed; action not sent')&&!/alert/.test(staleLine),staleLine);
  const refused={id:'race-closed',task:900,revision:7,action:'start'};
  call('answerSend',board,refused);
  call('answerReceipt',{root:'project',id:refused.id,state:'rejected',reason:'#900 is closed',next:''});
  assert(/alert/.test(notice(refused.id)),'a real refusal stays red');
  assert.equal(call('answerSteerComposer',board,{...running,state:'review'}),'','steer needs a running worker');
  assert.equal(call('answerWon',board,{...running,state:'ask'}),'','a new question replaces the answered line');
  call('save',board,{n:900},{draft:'one new instruction'});
  const steerAt=sent.length;call('answerSteer',board,running);
  // A steer now persists its immutable input before dispatch; still check the exact actual command, not the draft save.
  const steering=sent.slice(steerAt).find(m=>m.a==='answer').op;
  assert.deepEqual(plain(steering),{id:steering.id,task:900,revision:8,action:'steer',text:'one new instruction'});
  call('answerReceipt',{root:'project',id:steering.id,state:'accepted'});
  assert(call('answerToast').includes('#900 · steer sent'));
  assert(call('answerSteerComposer',board,running).includes('Steer sent to the agent.')&&!call('answerSteerComposer',board,running).includes('use y'),'a sent steer clears the composer');
  // #1785 A2: an answer from another client reached this tray only as a receipt, so the thread kept offering a closed question.
  // The refreshed status row's `answered` names it before anything is sent; steer stays for an unsent draft only.
  const elsewhere={n:901,state:'doing',revision:9,answered:{by:'sam',text:'ship it'}};project.tasks=[elsewhere];
  assert(call('answerWon',board,elsewhere).includes('Answered by sam: ship it'));
  assert.equal(call('answerSteerComposer',board,elsewhere),'','no draft, no steer offer');
  call('save',board,{n:901},{draft:'wait'});
  assert(call('answerSteerComposer',board,elsewhere).includes('wait'),'a kept draft becomes steer');
  call('save',board,{n:901},{draft:''});
  delete project.tasks;
  context.S.projects=[];
  // Run the consumer's capability across shared stale/lost-response fixtures; component owns draft and op state.
  const fx=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json','utf8'));
  for(const c of fx.cases.filter(c=>/stale|lost|duplicate|sending/.test(c.name))){
    context.S.projects=[{...project,issues:c.board}];
    const got=card.replay(c,(b,r)=>r.capability||c.capability&&{...c.capability,answer:c.capability.answer&&call('answerCapability',b).answer});
    c.events.filter(e=>e.type==='check').forEach((want,i)=>{
      if(want.status)assert.equal(got[i].status,want.status,c.name);
      if(want.draft!=null)assert.equal(got[i].draft,want.draft,c.name);
      if(want.same_op)assert.deepEqual(got[i].sent.at(-1),got[i].sent.at(-2),c.name);
    });
  }
  // A2: the tray alone grants fallback actions; every transition keeps revision and retry identity.
  const row={n:42,state:'ask',revision:12,card:'Reviewer needs help.',question:{parsed:false,text:'Reviewer needs help.',choices:[]}},cap={answer:true,manage:true};
  const fresh=()=>({draft:'',bound:12,op:null,receipt:null});
  let local=fresh(),seq=0;
  const step=ev=>{const r=card.step(row,local,cap,ev,()=> 'fallback-'+ ++seq);local=r.local;return r.send;};
  assert.equal(card.status(row,local,{answer:true}),'read-only','other consumers gain no fallback authority');
  assert(card.render(board,row,local,cap).includes('Requeue'));
  assert.equal(step({type:'submit'}),null,'requeue needs a reason');
  step({type:'input',text:'Please retry'});
  const rq=step({type:'submit'});assert.deepEqual(rq,{id:'fallback-1',task:42,revision:12,action:'requeue',text:'Please retry'});
  assert.equal(step({type:'later'}),null,'sending disables a second action');
  step({type:'receipt',opId:rq.id,state:'unknown'});assert.deepEqual(step({type:'retry'}),rq);
  local=fresh();const close=step({type:'close'});assert.equal(close.action,'close');assert.equal(close.confirmed,undefined,'one named action needs no confirmation flag');
  assert.equal(step({type:'close'}),null,'double click sends once');
  local=fresh();assert.equal(step({type:'later'}).action,'later');
  local={...fresh(),draft:'old draft'};row.revision=13;
  assert.equal(card.status(row,local,cap),'changed');
  assert.equal(step({type:'later'}).revision,13,'explicit action uses the displayed task revision; dispatcher fences races');
  for(const state of ['doing','later','review']) assert.equal(card.status({...row,state},fresh(),cap),'read-only');
  console.log('PASS tray capability, exact IPC payload, duplicate request, foreign receipt, timeout, same-ID retry, shared stale/lost-response fixtures');
})().catch(e=>{console.error(e);process.exit(1);});
