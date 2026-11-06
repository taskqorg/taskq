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
// #1740 A5: shared create() callers in workspace mode have no menu dashboard.
const workspace={operationController:true,S:{projects:[{root:'/a',name:'a',issues:'https://example.org/issues',board_kind:'github',tasks:[]}]}};
require('./tray-harness.cjs').install(workspace);
assert.match(workspace.taskRow({root:'/a',key:'https://example.org/issues/42',n:42,state:'ask',title:'Question'},true),/data-card="https:\/\/example.org\/issues\/42"/);
console.log('PASS TW2: exact task/revision launchers; shared workspace-mode task rows stay compatible');
