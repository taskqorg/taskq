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
const c={S:{now:1000,host:'mac',projects:[project]}};require('./tray-harness.cjs').install(c);
let html=c.runningAgents();
assert(html.includes('claude: 1 worker · 0 reviewers'));
assert(html.includes('codex: 0 workers · 1 reviewer'));
assert(html.includes('codex: 1 worker · 0 reviewers'));
assert(html.includes('old</strong> · unknown · 5m old'));
assert(html.includes('idle</strong> · idle'));
assert(html.includes('Reservations: claude 1'));
assert(html.includes('Occupied slots: claude 2 · codex 1'));
assert(html.includes('Tasks: Doing 3 · Review 2'));
const single=html;c.S.projects.push({...project,root:'duplicate'});assert.equal(c.runningAgents(),single,'duplicate board must not double count');
c.S.projects=[{...project,complete:false}];html=c.runningAgents();assert(!html.includes(' · idle'));assert(!html.includes('claude: 1 worker'));
c.S.projects=[{...project,age:301,alive:false,tasks_live:false}];html=c.runningAgents();assert(!html.includes('claude: 1 worker'));assert(html.includes('Capacity unknown'));
c.S.projects=[{...project,age:0,tasks_age_seconds:301,tasks_live:false}];assert(!c.runningAgents().includes('claude: 1 worker'),'capacity refresh cannot freshen old task observations');
c.S.projects=[{...project,tasks_age_seconds:null,tasks_live:false}];assert(!c.runningAgents().includes(' · idle'));
c.S.projects=[{...project,hosts:null}];assert(!c.runningAgents().includes(' · idle'));
c.S.projects=[{...project,hosts:[{name:'mac',updated:1100,live:false}]}];assert(!c.runningAgents().includes('claude: 1 worker'));
c.S.projects=[{...project,tasks:[{n:1,state:'doing',executions:[{...execution(11,'mac','claude','worker'),role:null}]}]}];assert(c.runningAgents().includes('mac</strong> · unknown'));
c.S.projects=[{...project,hosts:[{name:'<img>',updated:990,live:true}],tasks:[]}];assert(c.runningAgents().includes('&lt;img&gt;'));assert(!c.runningAgents().includes('<img>'));
for(const hosts of [{},[null],[{name:3}]]){c.S.projects=[{...project,hosts}];assert(!c.runningAgents().includes(' · idle'));}
c.S.projects=[{...project,tasks:[{n:1,executions:{}}]}];assert(!c.runningAgents().includes(' · idle'));
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
module.exports={project};
console.log('PASS running agents: roles, runtimes, hosts, stale/partial, reservations, deduplication and escaping');
