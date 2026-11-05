// #2366 approved numbers-only suffix: unknown/future boundaries never become issue age or zero.
const assert=require('node:assert/strict'),blocks=require('../src/project-blocks.js');
const now=1700000000,value={started:true,first:now-138*60,changes:7,current:now-7*60,closed:null};
const group=blocks.timing(value,now);assert.match(group.html,/>2h18m</);assert.match(group.html,/7<\/span>/);assert.match(group.html,/>7m</);assert.match(group.html,/rotate-cw/);assert.match(group.detail,/first worker ownership/);
assert.match(blocks.timing({started:false,current:now-60},now).html,/Not started/);assert.match(blocks.timing({started:false},now).html,/N\/A/);
for(const first of [null,NaN,-1,now+1])assert.match(blocks.timing({...value,first},now).html,/Unknown/);
assert.match(blocks.timing(null,now).html,/Unknown/);
assert.match(blocks.timing({...value,closed:now-60},now).html,/>2h17m</);
assert.match(blocks.timing({...value,completed:true,closed:null},now).html,/>Unknown</,'closed history without its closure boundary cannot keep running');
const row=blocks.task({name:'Invented',root:'fixture',board:{kind:'github',url:'https://example.org/issues'}},{n:7,state:'doing',revision:71,title:'Full wrapped title',runtime:'codex',timing:value},true,true);
assert.match(row,/aria-description="Total elapsed/);assert.match(row,/data-task="7"/);assert.equal((row.match(/task-timing/g)||[]).length,1);assert(!/timing[^>]*tabindex/.test(row));
console.log('PASS numbers-only task timing and accessible whole-row description');
