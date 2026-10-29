// Chain: tray pending store -> `{sync}` push -> every window's snapshot overlay. Risk (#1819): a window waits for the
// 2 s tick, an unconfirmed state reads as final, a failed write loses its Retry, or stale data looks current.
const assert=require('node:assert/strict');
const view=require('../src/sync-view.js');
const S={now:1000,display:{states:[{id:'doing',name:'Doing'},{id:'ready',name:'Ready'}]},
  projects:[{root:'/p',alive:true,age:1,tasks_age_seconds:2,poll:5,counts:{ask:1},tasks:[{n:7,state:'ask',state_name:'Ask',revision:20},{n:8,state:'held',revision:3}]}]};
const op=(phase,extra={})=>({root:'/p',id:'tray.1',task:7,action:null,expected:'doing',phase,reason:'',...extra});

// Cross-window: the same overlay feeds the menu list and Conversations; counts and the snapshot stay the board's.
const shown=view.overlay(S,[op('sending')]);
assert.equal(shown.projects[0].tasks[0].state,'doing');
assert.equal(shown.projects[0].tasks[0].state_name,'Doing ⟳','unconfirmed until the board shows it');
assert.deepEqual(shown.projects[0].counts,{ask:1});
assert.equal(S.projects[0].tasks[0].state,'ask','the received snapshot is not changed');
assert.equal(shown.projects[0].tasks[1],S.projects[0].tasks[1]);
assert.equal(view.overlay(S,[op('done')]).projects[0].tasks[0].state_name,'Doing');
assert.equal(view.overlay(S,[]),S);

// Offline: a failed write keeps the board's state and names why.
const failed=view.overlay(S,[op('failed',{reason:'service unreachable'})]).projects[0].tasks[0];
assert.equal(failed.state,'ask');
assert.equal(view.line(failed.sync),'Failed: service unreachable');

// Conflict: the real state and a short reason, never a re-confirm.
const lost=view.overlay(S,[op('mismatch',{expected:'ready',reason:'Answered first by alex'})]).projects[0].tasks[0];
assert.equal(lost.state_name,'Ready');
assert.equal(view.line(lost.sync),'Not applied: Answered first by alex');

// Stale: older than two polls (at least 15 s while a window is open) or missing.
assert.equal(view.stale(S),null);
const at=(age)=>({...S,projects:[{...S.projects[0],tasks_age_seconds:age}]});
assert.equal(view.stale(at(20)),null,'one 15 s menu poll late is not stale');
const old=new Date((1000-1-40)*1000),hhmm=String(old.getHours()).padStart(2,'0')+':'+String(old.getMinutes()).padStart(2,'0');
assert.equal(view.stale(at(40)),'Data from '+hhmm);
assert.equal(view.stale({...S,projects:[{...S.projects[0],alive:false}]}),'Data from: unknown');

// The menu window: a `{sync}` push redraws at once over the last snapshot; a failed row offers one Retry.
const sent=[];let renders=0;
const context={operationController:true,S:null,pending:{},render:()=>renders++,send:m=>sent.push(m),setTimeout:()=>0,clearTimeout(){},crypto:require('node:crypto').webcrypto,window:{},settle(){},answerReadback(){}};
require('./tray-harness.cjs').install(context);
context.receive({sync:[op('sending')]});
assert.equal(renders,0,'no snapshot yet: nothing to draw over');
context.receive({...S,sync:[]});
const before=renders;
context.receive({sync:[op('waiting')]});
assert.equal(renders,before+1,'the push redraws without a tick');
assert.equal(context.S.projects[0].tasks[0].state_name,'Doing ⟳');
assert.match(context.syncLine(context.S.projects[0].tasks[0].sync),/Waiting GitHub/);
context.receive({sync:[op('failed',{reason:'service unreachable'})]});
const line=context.syncLine(context.S.projects[0].tasks[0].sync);
assert.match(line,/data-a="sync-retry" data-root="\/p" data-id="tray.1"/);
assert.equal((line.match(/<button/g)||[]).length,1);
assert.equal(context.syncLine(op('waiting')).includes('<button'),false,'only a failed operation retries');
context.receive({...S,tasks_age_seconds:0});
assert.equal(context.S.projects[0].tasks[0].sync.phase,'failed','a new snapshot keeps the store over it');
console.log('PASS pending overlay: same-frame expected state, unconfirmed mark, failed Retry, conflict reason, stale line');
