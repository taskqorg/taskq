// R6/M3: observed executions, not task states or capacity slots, determine running counts.
const assert=require('node:assert/strict');
// #1591: `alive`, `tasks_live` and row `live` come from the Rust ttl rule (tray.rs `liveness`); the page compares no age.
const execution=(id,host,runtime,role,state='running')=>({id,attempt:id,host,runtime,role,state,observed:990,live:true});
const project={root:'p',issues:'https://example.org/p',host:'mac',age:1,tasks_age_seconds:0,alive:true,tasks_live:true,complete:true,observed:1000,
 hosts:[{name:'mac',updated:990,live:true},{name:'win',updated:990,live:true},{name:'old',updated:690,live:false,age:310,age_text:'5m'},{name:'idle',updated:990,live:true}],
 tasks:[{n:1,state:'doing',executions:[execution(11,'mac','claude','worker')]},
 {n:2,state:'review',executions:[execution(12,'mac','codex','reviewer')]},
 {n:3,state:'doing',executions:[execution(13,'win','codex','worker')]},
 {n:4,state:'review',executions:[]},{n:5,state:'doing',executions:[execution(15,'old','claude','worker','unknown')]}],
 capacity:{host_runtime_used:{claude:2,codex:1},reservations:[{task:1,attempt:11,rt:'claude',state:'held'},{task:6,attempt:16,rt:'claude',state:'held'}]}};
// #1592: rows carry the display model `key` (display.rs `model`: board kind, board url, number) and the Tasks line reads
// the model's all-projects counts, so the page neither counts states nor dedupes boards itself.
const fx=require('./fixtures/display-model.json');
const counts=n=>({counts:fx.display.states.map(s=>({state:s.id,name:s.name,n:n===null?null:n[s.id]??0}))});
const keyed=ps=>ps.map(p=>Array.isArray(p?.tasks)?{...p,tasks:p.tasks.map(t=>({...t,key:'github:'+p.issues+'/'+t.n}))}:p);
const c={S:{now:1000,host:'mac',projects:[project],display:{...fx.display,all:counts({doing:3,review:2})}}};require('./tray-harness.cjs').install(c);
const running=c.runningAgents;c.runningAgents=(...a)=>{c.S.projects=keyed(c.S.projects);return running(...a);};
let html=c.runningAgents();
assert(html.includes('claude: 1 worker · 0 reviewers'));
assert(html.includes('codex: 0 workers · 1 reviewer'));
assert(html.includes('codex: 1 worker · 0 reviewers'));
assert(html.includes('old</strong> · runs unknown · 5m old'));
assert(html.includes('idle</strong> · Claude 0 · Codex 0'));
// #1661 R16/R17: one line per named host, Claude and Codex apart; this host's expansion holds its resources, a remote one names what is not collected
assert(html.includes('mac</strong> · Claude 1 · Codex 1'));assert(html.includes('win</strong> · Claude 0 · Codex 1'));
assert(html.includes('CPU and memory: not collected for remote hosts'));assert(html.includes('CPU and memory: no sample; sampled only while the load gate is on'));
assert.deepEqual(c.runningAgents(true),{label:'idle · Claude 0 · Codex 0; mac · Claude 1 · Codex 1; old · Claude ? · Codex ?; win · Claude 0 · Codex 1',meta:''},'R16: every host and both counters survive collapse, including unknown');
assert(html.includes('Reservations: claude 1'));
assert(html.includes('Occupied slots: claude 2 · codex 1'));
assert(html.includes('Tasks: Doing 3 · Review 2'));
c.S.display.all=counts({doing:7,review:0});assert(c.runningAgents().includes('Tasks: Doing 7 · Review 0'),'the line reads model counts, not rows');
c.S.display.all=counts(null);assert(c.runningAgents().includes('Tasks unknown'),'unknown model counts never read zero');
c.S.display.all=counts({doing:3,review:2});
const single=html;c.S.projects.push({...project,root:'duplicate'});assert.equal(c.runningAgents(),single,'duplicate board must not double count');
c.S.projects=[{...project,complete:false}];html=c.runningAgents();assert(!html.includes('idle</strong> · Claude 0'));assert(!html.includes('claude: 1 worker'));
c.S.projects=[{...project,age:301,alive:false,tasks_live:false}];html=c.runningAgents();assert(!html.includes('claude: 1 worker'));assert(html.includes('Capacity unknown'));
c.S.projects=[{...project,age:0,tasks_age_seconds:301,tasks_live:false}];assert(!c.runningAgents().includes('claude: 1 worker'),'capacity refresh cannot freshen old task observations');
c.S.projects=[{...project,tasks_age_seconds:null,tasks_live:false}];assert(!c.runningAgents().includes('idle</strong> · Claude 0'));
c.S.projects=[{...project,hosts:null}];assert(!c.runningAgents().includes('idle</strong> · Claude 0'));
c.S.projects=[{...project,hosts:[{name:'mac',updated:1100,live:false}]}];assert(!c.runningAgents().includes('claude: 1 worker'));
c.S.projects=[{...project,tasks:[{n:1,state:'doing',executions:[{...execution(11,'mac','claude','worker'),role:null}]}]}];assert(c.runningAgents().includes('mac</strong> · runs unknown'));
c.S.projects=[{...project,hosts:[{name:'<img>',updated:990,live:true}],tasks:[]}];assert(c.runningAgents().includes('&lt;img&gt;'));assert(!c.runningAgents().includes('<img>'));
for(const hosts of [{},[null],[{name:3}]]){c.S.projects=[{...project,hosts}];assert(!c.runningAgents().includes('idle</strong> · Claude 0'));}
c.S.projects=[{...project,tasks:[{n:1,executions:{}}]}];assert(!c.runningAgents().includes('idle</strong> · Claude 0'));
// A2: the same board/task/execution observed twice is one process; task/board identities remain distinct.
c.S.projects=[{...project,hosts:[{name:'mac',updated:990,live:true}],tasks:[{n:1,state:'doing',executions:[execution(11,'mac','codex','worker'),execution(11,'mac','codex','worker')]}]}];
assert(c.runningAgents().includes('codex: 1 worker · 0 reviewers'),'duplicate execution must not double count');
c.S.projects[0].tasks.push({n:2,state:'review',executions:[execution(11,'mac','codex','reviewer')]});
assert(c.runningAgents().includes('codex: 1 worker · 1 reviewer'),'task is part of execution identity');
c.S.projects.push({...c.S.projects[0],issues:'https://example.org/another'});
assert(c.runningAgents().includes('codex: 2 workers · 2 reviewers'),'board is part of execution identity');
c.S.projects=[{...project,age:301,alive:false,tasks_live:false}];assert(c.runningAgents().includes('stale task observation'),'A3: unknown names its reason');
// #1591 A3: a 61-299 s old observation the Rust rule calls live is live in the tray too.
c.S.projects=[{...project,age:250}];assert(c.runningAgents().includes('claude: 1 worker'),'no false unknown within ttl');
// #1661 R16-R19: a lone fresh host names itself with both counts; a load sample shows units and age; sign-in alerts reach the line
c.S.projects=[{...project,hosts:[{name:'mac',updated:990,live:true}],tasks:[{n:1,state:'doing',executions:[execution(11,'mac','claude','worker')]}],age_text:'<1m',sample_age_text:'2m',capacity:{...project.capacity,cpu_percent:41.6,memory_available_percent:62.2}}];
assert.deepEqual(c.runningAgents(true),{label:'mac · Claude 1 · Codex 0',meta:''});
assert(c.runningAgents().includes('CPU 42% busy · memory 62% available · sampled 2m ago'));
c.S.projects[0].tasks.push({n:9,key:'github:p/9',executions:[execution(19,'mac','codex-app','worker')]});assert.equal(c.runningAgents(true).label,'mac · Claude 1 · Codex 1','Codex app stays in the Codex family counter');
c.S.agents={at:900,fresh:true,rows:[{runtime:'codex',state:'expired'},{runtime:'claude',state:'check-failed'}]};
assert(c.runningAgents().includes('<span class="alert"> · Codex expired</span>'),'an expired sign-in shows on the host line');
c.S.projects=[];assert.deepEqual(c.runningAgents(true),{label:'mac · Claude ? · Codex ?',meta:''},'this host without an observation is unknown, never zero');
module.exports={project};
console.log('PASS running agents: roles, runtimes, hosts, stale/partial, reservations, deduplication and escaping');
