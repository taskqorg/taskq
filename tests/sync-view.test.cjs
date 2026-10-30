// Chain: tray pending store applied in Rust -> one snapshot -> every window shows it as received. Risk (#1918): a
// window overlays its own expected state, so its rows disagree with counts, icon and notifications or flip back after
// an answer; a failed write loses its Retry; stale data looks current.
const assert=require('node:assert/strict');
const view=require('../src/sync-view.js');
const op=(phase,extra={})=>({root:'/p',id:'tray.1',task:7,action:null,phase,reason:'',...extra});
const S={now:1000,display:{states:[{id:'doing',name:'Doing'},{id:'ask',name:'Ask'}]},
  projects:[{root:'/p',alive:true,age:1,tasks_age_seconds:2,poll:5,counts:{ask:1},tasks:[{n:7,state:'ask',state_name:'Ask',revision:20,sync:op('sending')},{n:8,state:'held',revision:3}]}]};

// One channel: the module has no overlay and no expected state; a sending row says Sending, never a guessed state.
assert.equal(view.overlay,undefined,'windows keep no optimistic overlay');
assert.deepEqual(Object.keys(view.phases),['sending','failed','mismatch']);
assert.equal(view.line(op('sending')),'Sending');

// Offline: a failed write keeps the board's state and names why.
assert.equal(view.line(op('failed',{reason:'service unreachable'})),'Failed: service unreachable');

// Conflict: the real state and a short reason, never a re-confirm.
assert.equal(view.line(op('mismatch',{reason:'Answered first by alex'})),'Not applied: Answered first by alex');

// Stale: older than two polls (at least 15 s while a window is open) or missing.
assert.equal(view.stale(S),null);
const at=(age)=>({...S,projects:[{...S.projects[0],tasks_age_seconds:age}]});
assert.equal(view.stale(at(20)),null,'one 15 s menu poll late is not stale');
const old=new Date((1000-1-40)*1000),hhmm=String(old.getHours()).padStart(2,'0')+':'+String(old.getMinutes()).padStart(2,'0');
assert.equal(view.stale(at(40)),'Data from '+hhmm);
assert.equal(view.stale({...S,projects:[{...S.projects[0],alive:false}]}),'Data from: unknown');

// The menu window adopts the snapshot as received; a failed row offers one Retry.
const sent=[];let renders=0;
const context={operationController:true,S:null,pending:{},render:()=>renders++,send:m=>sent.push(m),setTimeout:()=>0,clearTimeout(){},crypto:require('node:crypto').webcrypto,window:{},settle(){},answerReadback(){}};
require('./tray-harness.cjs').install(context);
context.receive(S);
assert.equal(context.S.projects[0].tasks[0].state,'ask','the row is the snapshot row');
assert.match(context.syncLine(context.S.projects[0].tasks[0].sync),/Sending/);
const failed={...S,projects:[{...S.projects[0],tasks:[{...S.projects[0].tasks[0],sync:op('failed',{reason:'service unreachable'})}]}]};
context.receive(failed);
const line=context.syncLine(context.S.projects[0].tasks[0].sync);
assert.match(line,/data-a="sync-retry" data-root="\/p" data-id="tray.1"/);
assert.equal((line.match(/<button/g)||[]).length,1);
assert.equal(context.syncLine(op('sending')).includes('<button'),false,'only a failed operation retries');
context.receive({...S,projects:[{...S.projects[0],tasks:[{n:7,state:'doing',revision:25}]}]});
assert.equal(context.S.projects[0].tasks[0].sync,undefined,'the next snapshot replaces the row whole');
// Conversations renders the same row and stale line as the menu (#1819 review): a failed answer there offers Retry,
// and its status footer names the data age.
const {html}=require('./tray-harness.cjs');
assert.equal(html(view.row(op('failed',{reason:'service unreachable'}))),line,'Conversations and the menu show one sync row');
assert.equal(view.row(undefined),null);
assert.equal(view.staleRow(S),null);
assert.match(html(view.staleRow(at(40))),new RegExp('role="status">Data from '+hhmm+'<'));
const fs=require('node:fs'),workspace=fs.readFileSync('src/workspace.js','utf8'),app=fs.readFileSync('src/tray-app.js','utf8');
assert.doesNotMatch(workspace+app,/overlay|view\.raw|view\.sync/,'no window keeps its own pending state');
assert.match(workspace,/syncView\.row\(t\?\.sync\)/,'the task thread shows its pending operation');
assert.match(workspace,/footer:\[api\.staleText\(\)/,'the Conversations footer shows Data from');
assert.match(workspace,/b\.dataset\.a==='sync-retry'\)ui\.send\(\{a:'sync-retry',root:b\.dataset\.root,id:b\.dataset\.id\}\)/,'Retry resends from Conversations');
console.log('PASS one channel: no overlay, Sending until readback, failed Retry, conflict reason, stale line, Conversations row');
