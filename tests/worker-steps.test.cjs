// Chain: live worker transcript items -> Conversations worker panel. Risk (#1918 A6): every tool call is its own line, so
// a worker's run buries its messages; a fold hides which files, commands and tests ran, or the current step.
const assert=require('node:assert/strict');
const {html}=require('./tray-harness.cjs');
const c=require('../src/conversations.js').create({window:{},document:{},menu:{},draw(){},navigate(){},onSelect(){}});
let i=0;
const item=(role,text,tool_name)=>({at:1,i:i++,role,text,...(tool_name?{tool_name}:{})});
const items=[item('assistant','Looking at the parser'),
  item('tool','Read /repo/src/parse.rs','Read'),item('tool','Edit /repo/src/parse.rs','Edit'),item('tool','Grep fold','Grep'),
  item('tool','Bash cargo test --locked parse','Bash'),item('assistant','Fixed; one more look'),
  item('tool','Read /repo/src/lex.rs','Read'),item('assistant','Done'),
  item('tool','Bash git status','Bash'),item('tool','Bash python3 scripts/check.py','Bash')];
const render=x=>({type:'article',props:{class:'dialogue-item','data-key':x.at+':'+x.i,children:x.text}});

const out=c.steps(items,true,render),shown=html(out);
// a message ends a group: four tool calls, then one alone, then two at the end
assert.equal(out.length,6);
const groups=[...shown.matchAll(/<summary class="dialogue-tool"><span class="tool-label">([^<]*)<\/span>/g)].map(m=>m[1]);
assert.deepEqual(groups,['4 steps · files parse.rs · 1 command · tests cargo test --locked parse',
  '2 steps · 1 command · tests python3 scripts/check.py · now: Bash python3 scripts/check.py']);
// a single call stays one plain line; expanding a group shows each step in order
assert.match(shown,/<p data-key="1:6" class="dialogue-tool"[^>]*><span class="tool-label">Tool · Read …\/lex\.rs<\/span>.*Time unknown<\/span><\/p>/);
assert.match(shown,/<details data-key="1:1" class="dialogue-steps">.*Tool · Read …\/parse\.rs.*Tool · Edit …\/parse\.rs.*Tool · Grep fold.*Tool · Bash cargo test --locked parse.*<\/details>/);
// an ended worker has no current step
assert.doesNotMatch(html(c.steps(items,false,render)),/now:/);
console.log('PASS worker steps: consecutive tool calls fold with count, files, commands, tests and the live step');
