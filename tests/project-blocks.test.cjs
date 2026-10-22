// Chain: board snapshot -> shared summary/rows -> tray/browser navigation. Risk: duplicate counts, invented totals,
// drifted assets, unsafe links or counters triggering project navigation. No live board/native claim.
const fs=require('node:fs'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const q=require('../src/project-blocks.js'),fx=require('./fixtures/project-blocks.json');
const p=fx.project,row=fx.row;
let got=q.observation([{...p,tasks:[row]},{...p,id:'second-registration',tasks:[row]}]);
assert.equal(got.counts.doing,1);assert.equal(got.counts.review,0);assert.deepEqual(got.partial,[]);
// K-A1/K-A3/K-A6: All tasks counts open rows once, includes held, excludes closed rows; Held is its own counter and Closed is gone.
assert.equal(got.open,1);
got=q.observation([{...p,tasks:[row,{...row,n:2,state:'held'},{...row,n:3,state:'closed'}]}]);
assert.equal(got.open,2);assert.equal(got.counts.held,1);assert(!('closed' in got.counts),'no Closed counter (#1476)');
assert.equal(q.observation([{...p,complete:false,tasks:[row]}]).open,null);
assert.match(q.aggregate([{...p,complete:false,tasks:[row]}],[]),/All tasks \?<\/button>/);
assert(!q.summary(p,[],'"><script>').includes('<script>'),'All tasks only interpolates a valid count');
assert.match(q.aggregate([{...p,tasks:[row]}],[]),/data-a="all-tasks" aria-pressed="true"/,'All tasks is selected without a state filter');
assert.match(q.aggregate([{...p,tasks:[row]}],['doing']),/data-a="all-tasks" aria-pressed="false"/,'All tasks clears its selection under a state filter');
assert.match(q.aggregate([{...p,tasks:[row]},{...p,id:'duplicate',tasks:[row]}],[]),/All tasks 1<\/button>/);
assert.equal(q.observation([{...p,tasks:[row]},{...p,tasks:[{...row,state:'review'}]}]).counts.doing,null);
assert.equal(q.observation([{...p,complete:false}]).counts.ready,null);
assert.equal(q.observation([{...p,tasks:[row]},{...p,tasks:[{...row,title:'Changed title'}]}]).open,null,'conflicting titles make search scope unknown');
assert.equal(q.observation([{...p,complete:false}]).counts.held,null,'partial snapshots show Held as unknown');
assert.match(q.summary({...p,tasks:[{...row,state:'held'}]}),/data-state="held"[^>]*aria-label="[^"]* · Held: 1"/);
assert(!q.summary(p).includes('tqp-counts'),'known zero shows identity only');
assert(q.summary({...p,complete:false}).includes('Partial:'));
assert(!q.summary({...p,tasks:[row]}).includes('Closed'),'no Closed capsule');
assert(q.task(p,{...row,title:'<script>alert(1)</script>',url:'javascript:alert(1)'}).includes('&lt;script&gt;'));
assert(!q.task(p,{...row,url:'javascript:alert(1)'}).includes('href="javascript:'));
assert(!q.task(p,{...row,local_session:false}).includes('codex://'));
assert(q.task(p,{...row,local_session:false,session_reason:'Codex session opens only on WIN'}).includes('title="Codex session opens only on WIN">Session unavailable'));
// R6/R12: the native snapshot's unresolved execution must remain visible in every shared task block.
// § 4 Tray age: short units from the last logged state change; unknown names its reason; no `since`, no age.
assert.deepEqual([59,300,7200,259200,-5].map(q.age),['<1m','5m','2h','3d','<1m']);
assert(q.task({...p,now:1300},{...row,since:1000}).includes('data-since="1000" title="Since the last state change">5m<'));
assert(q.task(p,{...row,since:null}).includes('title="Age unknown: no logged state change">age ?'));
assert(!q.task(p,row).includes('tqp-age'));
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
assert.equal(Object.keys(manifest.icons).length,16);
for(const [file,hash] of Object.entries(manifest.files))assert.equal(crypto.createHash('sha256').update(fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n')).digest('hex'),hash,file);
for(const [name,hash] of Object.entries(manifest.icons))assert.equal(crypto.createHash('sha256').update(q.assets[name]).digest('hex'),hash,name);
for(const name of ['github','gitlab','claude','openai'])assert.equal(q.assets[name],fs.readFileSync('docs/design/assets/'+name+'.svg','utf8').replace(/\r\n/g,'\n'));
const tray=fs.readFileSync('build/web/tray.html','utf8');
const sandbox={setTimeout,clearTimeout,crypto:require('node:crypto').webcrypto};require('node:vm').createContext(sandbox);
require('node:vm').runInContext(fs.readFileSync('build/web/project-blocks.js','utf8'),sandbox);
assert.deepEqual(JSON.parse(JSON.stringify(sandbox.TaskqProjectBlocks.observation([p]))),JSON.parse(JSON.stringify(q.observation([p]))));
console.log('PASS shared counts, dedup/conflict/partial/Held, safe links, local/remote sessions, independent navigation, provider assets, provenance and native/release wiring');
