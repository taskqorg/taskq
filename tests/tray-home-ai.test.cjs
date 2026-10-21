// TW2: menu opens the workspace without composing, sending or answering.
const assert=require('node:assert/strict');
const context={S:{projects:[{root:'/a',name:'a'}]},aiRoot:'/a'};
require('./tray-harness.cjs').install(context);
assert.match(context.ai(),/data-a="workspace" data-page="request" data-root="\/a"/);
assert(!/textarea|ai-send|<form/.test(context.ai()));
assert.match(context.answerSlot({state:'ask',key:'https://example.org/issues/42',n:42,revision:9}),/data-board="https:\/\/example.org\/issues" data-task="42" data-revision="9"/);
for(const state of ['doing','review','ready','later'])assert.equal(context.answerSlot({state}),'');
console.log('PASS TW2: request and exact task/revision launchers contain no composer or answer card');
// TW5: the entry counts messages, never tasks or executions; incomplete collection must say unknown.
context.S.messages={count:2,complete:true};
assert.match(context.messagesBadge(),/Messages · 2/);
assert.match(context.messagesBadge(),/data-page="messages"/);
context.S.messages={count:null,complete:false,reason:'Source stale'};
assert.match(context.messagesBadge(),/Messages · unknown/);
assert.match(context.messagesBadge(),/Source stale/);
console.log('PASS TW5: message badge/count launcher and explicit unknown reason');
