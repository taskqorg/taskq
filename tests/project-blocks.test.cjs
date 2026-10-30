// Chain: board snapshot -> shared summary/rows -> tray/browser navigation. Risk: duplicate counts, invented totals,
// drifted assets, unsafe links or counters triggering project navigation. No live board/native claim.
const fs=require('node:fs'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const q=require('../src/project-blocks.js'),fx=require('./fixtures/project-blocks.json');
const p=fx.project,row=fx.row;
// #1592: counts, dedup, conflict and partial come from the Rust display model (display.rs tests); blocks only render it.
const {display}=require('./fixtures/display-model.json'),names=Object.fromEntries(display.states.map(s=>[s.id,s.name]));
const counts=(o={},unknown=false)=>Object.keys(names).map(state=>({state,name:names[state],n:unknown?null:o[state]||0}));
const all=(o,open,partial=[])=>({counts:counts(o,!!partial.length),partial,open:partial.length?null:open});
// K-A1/K-A6: Held is its own counter and Closed is gone; unknown counts and All tasks read `?`, never zero.
assert.deepEqual(Object.keys(q.marks),Object.keys(names),'one mark per model state');
// The released fixture is display model output: its project carries model counts/partial and its row the model name/age.
assert.deepEqual([p.counts,p.partial,p.open,row.state_name],[counts(),[],0,names[row.state]],'fixture follows the display model');
assert.match(q.aggregate(all({},null,['project']),[]),/All tasks \?<\/button>/);
assert(!q.summary({...p,counts:counts()},[],'"><script>').includes('<script>'),'All tasks only interpolates a valid count');
assert.match(q.aggregate(all({doing:1},1),[]),/data-a="all-tasks" aria-pressed="true"/,'All tasks is selected without a state filter');
assert.match(q.aggregate(all({doing:1},1),['doing']),/data-a="all-tasks" aria-pressed="false"/,'All tasks clears its selection under a state filter');
assert.match(q.aggregate(all({doing:1},1),[]),/All tasks 1<\/button>/);
assert.match(q.aggregate(all({},null,['project']),[]),/aria-label="All projects · Held: unknown"/,'partial snapshots show Held as unknown');
assert.match(q.summary({...p,counts:counts({held:1})}),/data-state="held"[^>]*aria-label="[^"]* · Held: 1"/);
assert(!q.summary({...p,counts:counts()}).includes('tqp-counts'),'known zero shows identity only');
assert(q.summary({...p,counts:counts({},true),partial:[p.name]}).includes('Partial:'));
assert(!q.summary({...p,counts:counts({doing:1})}).includes('Closed'),'no Closed capsule');
assert(q.task(p,{...row,title:'<script>alert(1)</script>',url:'javascript:alert(1)'}).includes('&lt;script&gt;'));
assert(!q.task(p,{...row,url:'javascript:alert(1)'}).includes('href="javascript:'));
assert(!q.task(p,{...row,local_session:false}).includes('codex://'));
assert(q.task(p,{...row,local_session:false,session_reason:'Codex session opens only on WIN'}).includes('title="Codex session opens only on WIN">Session unavailable'));
// R6 #1823: a timed park shows its board time on this computer's clock and the reason in both row forms; untimed adds nothing.
const until='2026-11-05T09:00:00Z',parked='Parked until '+new Date(until).toLocaleString()+': 168 qualified hours<';
for(const compact of [false,true])assert(q.task(p,{...row,state:'later',until,later:'Reason: 168 qualified hours'},false,compact).includes(parked),'timed park, compact '+compact);
assert(!q.task(p,{...row,state:'later'}).includes('Parked until'),'an untimed park names no time');
// Chain: R6 / taskq.md § 2 Stages tray row (#1880). Risk: a multi-stage task hides its first open stage, or a row without stages shows one.
for(const compact of [false,true])assert(q.task(p,{...row,state:'later',stage:{k:3,n:5,done:['A1','A2']}},false,compact).includes('>Stage 3/5<'),'stage k/n, compact '+compact);
assert(!q.task(p,row).includes('Stage '),'no recorded stage, no stage text');
assert(!q.task(p,{...row,until,later:'<script>'}).includes('<script>'),'the reason is escaped');
assert.equal(q.parked({until,later:'Reason: 168 qualified hours'}),'Parked until '+new Date(until).toLocaleString()+': 168 qualified hours','Conversations reads the same text');
assert.equal(q.parked({later:'Reason: x'}),'','an untimed park has no text');
// R6/R12: the native snapshot's unresolved execution must remain visible in every shared task block.
// § 4 Tray age: the model's age text; unknown names its reason; no `since`, no age.
assert(q.task(p,{...row,since:1000,age:'5m'}).includes('title="Since the last state change">5m<'));
assert(q.task(p,{...row,since:null,age:null}).includes('title="Age unknown: no logged state change">age ?'));
assert(q.task(p,row).includes('>'+row.age+'<'),'fixture row shows its model age');
const {since,...bare}=row;assert(!q.task(p,bare).includes('tqp-age'));
const waiting=require('./fixtures/status-snapshot.json').tasks.find(t=>t.n===2),waitRow=q.task(p,waiting);
// Chain: #1524 menu.md M3 Task row text. Risk: the raw execution diagnostic reaches the row instead of the owner text.
assert(waitRow.includes(waiting.wait)&&!waitRow.includes(waiting.execution_wait),'owner wait text is visible, the raw execution reason is not');
assert(waitRow.indexOf(waiting.wait)<waitRow.indexOf('tqp-task-title'),'execution status stays with metadata');
assert(!q.task(p,{...row,wait:'<script>secret</script>'}).includes('<script>'),'wait text is escaped');
// Chain: #1204 remote Claude row. Risk: the account-scoped link is dropped, or the remote mark follows the runtime mark or adds text.
const remoteClaude=q.task(p,{...row,runtime:'claude',local_session:false,session_url:'https://claude.ai/code/session_014abc'});
assert(remoteClaude.includes('href="https://claude.ai/code/session_014abc"'));
assert(remoteClaude.indexOf('data-icon="network"')<remoteClaude.indexOf('data-icon="claude"')&&!remoteClaude.includes('Remote session'));
assert(!q.task(p,row).includes('data-icon="network"'),'local rows carry no remote mark');
// Chain: #1928 A3/A4 / M3 / UX30-09..10: runtime mark opens the exact TaskQ thread;
// native/progress links keep adapter identity, protocol and remote restrictions separately.
const compact=q.task(p,row,false,true);
assert.match(compact,/class="tqp-mark" data-a="workspace" data-page="answer"/);
assert(compact.includes('data-task="'+row.n+'"')&&compact.includes('data-revision="'+row.revision+'"'));
for(const protocol of ['http:','https:']){
 const relay=protocol+'//relay.example/open.html#codex://threads/invented';
 assert.equal(q.destinations({local_session:true,native_session_url:relay})[0].url,relay,'native relay remains exact');
 assert.equal(q.destinations({progress_url:relay})[1].url,relay,'progress remains a separate exact web target');
}
assert.equal(q.destinations({local_session:true,native_session_url:'codex://threads/invented'})[0].url,'codex://threads/invented');
assert.equal(q.destinations({local_session:false,native_session_url:'codex://threads/invented'})[0].url,null,'remote native Codex cannot open locally');
for(const href of [null,'javascript:alert(1)'])assert.equal(q.destinations({local_session:true,native_session_url:href})[0].url,null);
for(const runtime of ['claude','codex','codex-app']){
 const session_url=runtime==='claude'?'https://claude.ai/code/session_invented':'http://relay.example/open.html#codex://threads/invented';
 assert.equal(q.destinations({runtime,local_session:true,session_url})[0].url,session_url,'older local adapter preserves its exact target');
 const d=q.destinations({runtime,local_session:false,session_url});
 assert.equal(d[runtime==='claude'?0:1].url,session_url,'older remote adapter preserves native/progress distinction');
}
assert.equal(q.destinations({native_session_reason:'Owning host unavailable'})[0].reason,'Owning host unavailable');
assert.equal(q.destinations({progress_reason:'Connection unavailable'})[1].reason,'Connection unavailable');
// Chain: M3 shared task row -> metadata/independent links -> full title block in every consumer.
// Risk: a long title is truncated or pushes the session control into the title line.
const splitRow=q.task(p,{...row,title:'Complete title '.repeat(30)},true);
assert(splitRow.includes('<div class="tqp-task-title">'+'Complete title '.repeat(30)+'</div>'));
assert(splitRow.indexOf('aria-label="Open codex session"')<splitRow.indexOf('tqp-task-title'));
assert(splitRow.indexOf('data-icon="openai"')<splitRow.indexOf('tqp-task-title'));
for(const provider of ['github','gitlab'])assert(q.summary({...p,board:{...p.board,kind:provider}}).includes('data-icon="'+provider+'"'));
const actions=[],adapter={project:id=>actions.push(['project',id]),tasks:(id,st)=>actions.push(['tasks',id,st]),open:u=>actions.push(['open',u])};
function click(dataset){let prevented=false;assert(q.handle({target:{closest:()=>({dataset,getAttribute:()=>dataset.url})},preventDefault(){prevented=true;}},adapter));assert(prevented);}
click({tqp:'tasks',project:p.id,state:'review'});click({tqp:'project',project:p.id});click({url:row.url});
assert.deepEqual(actions,[['tasks',p.id,'review'],['project',p.id],['open',row.url]]);
const manifest=require('../docs/design/project-blocks.sources.json');
// #1841: the build derives the released hashes from the committed file and icon lists.
const built=JSON.parse(fs.readFileSync('build/web/project-blocks.sources.json','utf8'));
assert.equal(manifest.icons.length,16);
assert.deepEqual(Object.keys(built.files),manifest.files);assert.deepEqual(Object.keys(built.icons),manifest.icons);
for(const [file,hash] of Object.entries(built.files))assert.equal(crypto.createHash('sha256').update(fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n')).digest('hex'),hash,file);
for(const [name,hash] of Object.entries(built.icons))assert.equal(crypto.createHash('sha256').update(q.assets[name]).digest('hex'),hash,name);
for(const name of ['github','gitlab','claude','openai'])assert.equal(q.assets[name],fs.readFileSync('docs/design/assets/'+name+'.svg','utf8').replace(/\r\n/g,'\n'));
const tray=fs.readFileSync('build/web/tray.html','utf8');
const sandbox={setTimeout,clearTimeout,crypto:require('node:crypto').webcrypto};require('node:vm').createContext(sandbox);
require('node:vm').runInContext(fs.readFileSync('build/web/project-blocks.js','utf8'),sandbox);
assert.equal(sandbox.TaskqProjectBlocks.summary({...p,counts:counts({doing:1})}),q.summary({...p,counts:counts({doing:1})}));
console.log('PASS model counters, unknown/partial/Held, safe links, local/remote sessions, independent navigation, provider assets, provenance and native/release wiring');
