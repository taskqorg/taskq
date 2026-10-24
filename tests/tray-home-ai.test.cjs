// TW2: menu opens the workspace without composing, sending or answering.
const assert=require('node:assert/strict');
const context={S:{projects:[{root:'/a',name:'a'}]},aiRoot:'/a'};
require('./tray-harness.cjs').install(context);
assert.match(context.ai(),/data-a="workspace" data-page="conversations" data-fresh="true" data-root="\/a"/);
assert(!/textarea|ai-send|<form/.test(context.ai()));
assert.match(context.answerSlot({state:'ask',key:'https://example.org/issues/42',n:42,revision:9}),/data-board="https:\/\/example.org\/issues" data-task="42" data-revision="9"/);
for(const state of ['doing','review','ready','later'])assert.equal(context.answerSlot({state}),'');
console.log('PASS TW2: request and exact task/revision launchers contain no composer or answer card');
