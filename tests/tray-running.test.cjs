// R6/M3.24, #1927 A1–A5: fenced executions, not task states/slots; truthful host/family minima and retained resources.
const assert=require('node:assert/strict'),{install,html:markup}=require('./tray-harness.cjs');
const execution=(id,host,runtime,role='worker',state='running')=>({id,attempt:id,host,runtime,role,state,observed:990,live:true});
const project={root:'p',issues:'https://example.org/p',host:'mac',age:1,age_text:'1s',tasks_age_seconds:0,alive:true,tasks_live:true,complete:true,observed:1000,
 hosts:['mac','win','idle'].map(name=>({name,live:true,age:5,age_text:'5s'})),
 tasks:[{n:1,state:'doing',executions:[execution(11,'mac','claude')]},{n:2,state:'review',executions:[execution(12,'mac','codex','reviewer')]},{n:3,state:'doing',executions:[execution(13,'win','codex')]},{n:4,state:'review',executions:[]}],
 capacity:{host_runtime_used:{claude:2,codex:1},reservations:[{task:1,attempt:11,rt:'claude',state:'held'},{task:6,attempt:16,rt:'claude',state:'held'}]}};
const fx=require('./fixtures/display-model.json'),c={S:{host:'mac',display:structuredClone(fx.display)}};install(c);
function render(ps=[project]){c.S.projects=ps.map(p=>({...p,tasks:Array.isArray(p.tasks)?p.tasks.map(t=>({...t,key:'github:'+p.issues+'/'+t.n})):p.tasks}));return c.runningAgents();}
const strip=()=>markup(c.runningAgents(true).strip);
const host=n=>strip().match(new RegExp('data-host="'+n+'"[^]*?<span class="host-number">([^<]*)'))?.[1];
const totals=()=>Object.fromEntries([...strip().split('class="host-runtime-totals"')[1].matchAll(/data-runtime="([^"]*)"[^]*?<span class="host-number">([^<]*)/g)].map(m=>m.slice(1)));
let page=render();
assert.equal(host('mac'),'2');assert.equal(host('idle'),'0');assert.deepEqual(totals(),{Codex:'2',Claude:'1'});
for(const text of ['claude: 1 worker · 0 reviewers','codex: 0 workers · 1 reviewer','codex: 1 worker · 0 reviewers','Reservations: claude 1','Occupied slots: claude 2 · codex 1','CPU and memory: not collected for remote hosts','CPU and memory: no sample; sampled only while the load gate is on','Heartbeat · 5s old','Count definitions','Running processes, occupied slots, reservations and board tasks are counted apart.'])assert(page.includes(text),text);
assert(page.includes('host-grid-head')&&page.includes('aria-label="Host, Codex, Claude, Total running agents"'));
const single=page;assert.equal(render([project,{...project,root:'duplicate'}]),single,'duplicate board snapshots count once');
// Board task counts still read the shared display model, never the execution list.
c.S.display.all.counts=fx.display.states.map(s=>({state:s.id,name:s.name,n:s.id==='doing'?7:0}));assert(render().includes('Tasks: Doing 7 · Review 0'));
c.S.display.all.counts.forEach(x=>x.n=null);assert(render().includes('Tasks unknown'));
c.S.display=structuredClone(fx.display);
for(const patch of [{complete:false},{age:301,alive:false,tasks_live:false},{age:0,tasks_age_seconds:301,tasks_live:false},{tasks_age_seconds:null,tasks_live:false},{tasks:[{n:1,executions:{}}]}]){
 page=render([{...project,...patch}]);assert.equal(host('mac'),'?');assert.deepEqual(totals(),{Codex:'?',Claude:'?'});assert(!page.includes('claude: 1 worker'));
}
for(const hosts of [null,{},[null],[{name:3}],[{name:'mac',live:false}]]){render([{...project,hosts}]);assert.equal(host('mac'),hosts?.[0]?.name==='mac'?'?':undefined,'a missing heartbeat has no chip; an open stale heartbeat cannot establish idle or running');assert.equal(totals().Codex,'?');}
render([{...project,age:250}]);assert.equal(host('mac'),'2','Rust ttl flag remains authoritative within ttl');
page=render([{...project,tasks:[...project.tasks,{n:8,executions:[{...execution(18,'mac','claude'),role:null}]}]}]);assert.equal(host('mac'),'2+?');assert.deepEqual(totals(),{Codex:'2+?',Claude:'1+?'});assert(page.includes('Partial: run state unknown'));
page=render([project,{...project,root:'q',issues:'https://example.org/q',tasks_live:false,hosts:[{name:'win',live:true,age:100,age_text:'2m'}],tasks:[]}]);assert.equal(host('mac'),'2');assert.equal(host('win'),'1+?');assert(page.includes('stale task observation (q)'));assert(page.includes('Heartbeat · 5s old'),'freshest heartbeat wins');
render([{...project,tasks:[...project.tasks,{n:8,executions:[execution(18,'mac','claude','worker','waiting'),execution(19,'mac','codex','reviewer','ended')]}]}]);assert.equal(host('mac'),'2');assert.deepEqual(totals(),{Codex:'2',Claude:'1'});
// Duplicate execution IDs dedupe within a task; distinct board/task identities remain separate.
const one={...project,tasks:[{n:1,executions:[execution(11,'mac','codex'),execution(11,'mac','codex')]}]};
render([one]);assert.equal(host('mac'),'1');
const two={...one,tasks:[...one.tasks,{n:2,executions:[execution(11,'mac','codex','reviewer')]}]};render([two,{...two,issues:'https://example.org/other'}]);assert.equal(host('mac'),'4');
// Conflicts never leave the first observed host/family as a false lower bound, regardless of order.
for(const field of ['host','runtime','role'])for(const reverse of [false,true]){
 const xs=[execution(11,'mac','codex'),{...execution(11,'mac','codex'),[field]:{host:'win',runtime:'other-runtime',role:'reviewer'}[field]}];if(reverse)xs.reverse();
 page=render([{...project,tasks:[{n:1,executions:xs}]}]);assert.equal(host('mac'),'?');if(field==='host')assert.equal(host('win'),'?');assert.equal(totals().Codex,'0+?');assert(page.includes('conflicting execution observations'));assert(!page.includes('codex: 1 worker'));
}
render([{...project,tasks:[...project.tasks,{n:9,executions:[execution(19,'mac','codex-app')]},{n:10,executions:[execution(20,'mac','other-runtime')]},{n:11,executions:[execution(21,'unlisted','other-runtime')]}]}]);assert.equal(host('mac'),'4');assert.equal(host('unlisted'),undefined);assert.deepEqual(totals(),{Codex:'3+?',Claude:'1+?','other-runtime':'1+?'});assert(c.runningAgents().includes('unlisted</strong>'));
page=render([{...project,hosts:[{name:'<img>',live:true}],tasks:[]}]);assert(page.includes('&lt;img&gt;')&&!page.includes('<img>'));
render([{...project,hosts:['mac','macbook','hermes-fra1','hermes-win','win'].map(name=>({name,live:true}))}]);assert.deepEqual([...strip().matchAll(/<strong>([^<]*)<\/strong>/g)].map(m=>m[1]),['hermes-f','hermes-w','mac','macb','win']);assert(strip().includes('title="hermes-fra1 · idle · Total running agents: 0"'));
c.S.agents={fresh:true,rows:[{runtime:'codex',state:'expired'},{runtime:'claude',state:'check-failed'}]};page=render([{...project,sample_age_text:'2m',capacity:{...project.capacity,cpu_percent:41.6,memory_available_percent:62.2}}]);assert.equal(host('mac'),'2');assert.deepEqual(totals(),{Codex:'2',Claude:'1'});assert(page.includes('Sign-in: Codex expired'));assert(page.includes('CPU 42% busy · memory 62% available · sampled 2m ago'));assert(strip().includes('class="host-warning"'));
page=render([]);assert.equal(host('mac'),undefined,'#1967 A4: no named strip chip without an open heartbeat');assert(strip().includes('Hosts ?'));assert.deepEqual(totals(),{Codex:'?',Claude:'?'});assert(page.includes('Capacity unknown'));
module.exports={project};
console.log('PASS UX30-06 / A1–A5: host/family totals, idle/partial/stale/conflict/duplicate/ended/runtime cases; roles, resources, source and sign-in preserved');
