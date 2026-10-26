// TW2: menu opens the workspace without composing, sending or answering.
const assert=require('node:assert/strict');
const context={S:{projects:[{root:'/a',name:'a'}]},aiRoot:'/a'};
require('./tray-harness.cjs').install(context);
assert.match(context.ai(),/data-a="workspace" data-page="conversations" data-fresh="true" data-root="\/a"/);
assert(!/textarea|ai-send|<form/.test(context.ai()));
assert.match(context.answerSlot({state:'ask',key:'https://example.org/issues/42',n:42,revision:9}),/data-board="https:\/\/example.org\/issues" data-task="42" data-revision="9"/);
// #1723 A2 replaces the old exclusion of non-Ask task entries; keep exact context and navigation-only controls.
for(const state of ['doing','review','ready','later','waiting','held','failed']){
 const entry=context.answerSlot({state,key:'https://example.org/issues/42',root:'/a',n:42,revision:9});
 assert.match(entry,/data-board="https:\/\/example.org\/issues" data-task="42" data-revision="9"/);
 assert.equal((entry.match(/data-a="workspace"/g)||[]).length,1);assert(!/textarea|ai-send|<form|data-a="control"/.test(entry));
}
console.log('PASS TW2: request and exact task/revision launchers contain no composer or answer card');
