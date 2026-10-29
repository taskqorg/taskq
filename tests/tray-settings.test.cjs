// #1661 R18–R20, R28–R31: Settings content reads the AS5 sign-in view, effective access and one host service line.
// Defect guarded: a stale probe read as current sign-in, a method shown as the account, a remote host or a missing
// access file shown as known, a probe in progress shown as its prior result, Work mode pointing elsewhere instead of
// showing access, the host service repeated per project, and a widget route without its page.
const assert=require('node:assert/strict');
const fx=require('./fixtures/display-model.json');
const access=[{runtime:'claude',requested:'workspace-write',grant:'workspace-write',effective:'workspace-write',enforcement:'sandbox'},{runtime:'codex',requested:'full',grant:null,effective:null,refused:'host grant missing'}];
const project=(root,host,version,more={})=>({root,name:root,host,version,age:2,age_text:'<1m',alive:true,complete:true,tasks:[],focus:[],labels:[],delivery:{preset:'auto-merge'},...more});
const row=(runtime,state,more={})=>({runtime,state,version:'2.1',account:null,method:null,expires:null,refreshed:null,reason:null,next:null,...more});
const c={S:{now:1000,host:'mac',period:'1d',history:require('./fixtures/icon-history/day-light.json'),version:'4.1.0',font:16,display:fx.display,service:{state:'running',pid:42,fresh:true},notify:{status:'allowed',host:{ask:'off',close:'off',failed:'off',gone:'off'},projects:{}},
 projects:[project('alpha','mac','4.1.0',{access}),project('beta','mac','4.1.0'),project('gamma','win','4.0.9')],
 agents:{at:700,age:'5m',fresh:true,rows:[row('claude','signed-in',{account:'owner@example.org',method:'Claude subscription',expires:4102444800}),row('codex','check-failed',{method:'ChatGPT',reason:'probe output unreadable'})]}}};
require('./tray-harness.cjs').install(c);
c.page='agents';let html=c.routeBody();
assert(html.includes('Claude 2.1 · <span class="">Signed in</span><span class="meta"> · owner@example.org</span>'),'account, not method, beside the state');
assert(html.includes('Method: Claude subscription'),'the method stays in the details');
assert(html.includes('Codex 2.1 · <span class="alert">Check failed</span><span class="meta"> · account unknown</span>'),'ChatGPT is a method, never the account');
assert(html.includes('Expiry unknown'),'no expiry is invented');assert(html.includes('Expires '),'a supplied expiry is shown');
assert(html.includes('Runtime client 2.1 · checked 5m ago'));assert(html.includes('Tray 4.1.0 · service 4.1.0 (2)'),'worker service versions apart from the runtime client');
assert(/win · heartbeat [^<]* · clients unknown/.test(html),'a remote host shows heartbeat freshness only');
assert(html.includes('Codex app · unknown: win heartbeat publishes no client version, sign-in, account or access'),'#1702 A3: missing source named');
assert(html.includes('Claude: requested workspace-write · saved grant workspace-write · applied workspace-write · sandbox'));
assert(html.includes('Codex: refused: host grant missing'));assert(html.includes('Access unknown for beta: no access in status.json'),'a missing access file names one reason');
assert(!html.includes('data-route="agents"'),'R29: no circular Agents placeholder');
c.S.agents={...c.S.agents,fresh:false};html=c.routeBody();
assert(!html.includes('>Signed in<'),'a stale probe is never current sign-in');assert(html.includes('Signed in at '));
for(const [state,label] of [['expired','Expired'],['rejected','Rejected'],['sign-in-required','Sign in required'],['unknown','Unknown']]){
 c.S.agents={...c.S.agents,rows:[row('claude',state,{next:state==='unknown'?null:'run `claude auth login`'})]};html=c.routeBody();
 assert(html.includes('>'+label+'<'),label+' stays distinct');assert.equal(html.includes('A check never signs in.'),state!=='unknown');
}
// AS5 Checking: a probe in progress reads Checking apart from every result, the prior result kept as a detail
c.S.agents={at:700,age:'5m',fresh:true,checking:true,rows:[row('claude','expired')]};html=c.routeBody();
assert(html.includes('Claude 2.1 · <span class="">Checking</span>'),'a probe in progress is not its prior result');
assert(html.includes('Last result: Expired'));assert(html.includes('sign-in checking now · checked 5m ago'));
c.page='settings';assert(c.routeBody().includes('checking sign-in'));c.page='agents';
c.S.agents={at:null,checking:true,rows:[]};assert(c.routeBody().includes('Checking: no earlier result'));
c.S.agents=null;assert(c.routeBody().includes('Unknown: no status.json has agents'));
// R30: one host service line on Services; project rows name their status age only
c.page='services';html=c.routeBody();assert.equal(html.split('Host service: running · PID 42').length-1,1,'the host service appears once per host');
// R31: equal service versions form one group
c.page='about';html=c.routeBody();assert(html.includes('Service 4.1.0<span class="meta"> · 2 projects'));assert(html.includes('Service 4.0.9<span class="meta"> · 1 project'));
// R32: admission comes first; projects without a reported mode never read as none
assert.match(c.admissionText(),/^Unknown: all 3 projects.*Intake: 3 unknown/);c.S.projects[0].mode='paused';assert.match(c.admissionText(),/Intake: 1 paused · 2 unknown/);delete c.S.projects[0].mode;
const original=c.S.projects;c.S.projects=[{...original[0],mode:'run',capacity:{gate:'open',reasons:[{code:'access',text:'claude: host grant missing'}]}},{...original[1],mode:'run',capacity:{gate:'open',reasons:[]}}];
assert.match(c.admissionText(),/^Mixed: alpha \(access\) · beta \(no reported blockers\).*Intake: 2 configured/,'a load-ready gate cannot hide access-blocked work, or block every host runtime');
c.S.projects[1].alive=false;assert.match(c.admissionText(),/^Unknown:/,'stale never ready');
c.S.projects=original;
c.S.agents={at:900,fresh:true,rows:[row('codex','signed-in'),row('codex-app','signed-in',{version:'3.2'})]};
c.S.projects[0].access.push({runtime:'codex-app',effective:'workspace'});c.page='agents';html=c.routeBody();assert(html.includes('Codex app 3.2 · <span'));assert(html.includes('Codex app 3.2 workspace'));assert(html.includes('Codex 2.1 refused'));
// R32: Work mode shows the access facts themselves after admission, not a pointer to Agents
const wm={operationController:true,S:c.S};require('./tray-harness.cjs').install(wm);html=wm.workPage();
assert(html.indexOf('Admission · ')<html.indexOf('Codex: refused: host grant missing'),'admission first, then access');
assert(html.includes('Claude: requested workspace-write · saved grant workspace-write · applied workspace-write · sandbox'));
assert(html.includes('Access unknown for beta: no access in status.json'));assert(!html.includes('Access is separate'));
// R28: every Settings widget opens a route the page renders
c.page='settings';html=c.routeBody();
for(const route of html.match(/data-route="[^"]+"/g).map(x=>x.slice(12,-1))){c.page=route;assert.notEqual(c.routeBody(),'','route '+route+' renders');}
// R35: Recovery names each target's read state and next action; an unread target is never shown as read
const w={operationController:true,S:{now:1000,host:'mac',projects:[{root:'a',name:'A'},{root:'b',name:'B'},{root:'c',name:'C'}]}};require('./tray-harness.cjs').install(w);
w.recovery={b:{pending:true},c:{notice:'Recovery read failed'}};
assert.deepEqual(w.recoveryPanel().match(/role="status">[^<]*/g).slice(0,3),['role="status">Not read · Next: read current recovery','role="status">Reading… · Next: wait for the receipt','role="status">Outcome unknown · Next: read current recovery']);
// #1812 A2 / M1.1: Appearance must expose a bounded, named opacity control and an explicit Off value.
c.page='appearance';c.S.opacity=90;html=c.routeBody();
assert(html.includes('id="menu-opacity"'),'Appearance must expose menu opacity');
assert.match(html,/type="range"[^>]*min="90"[^>]*max="100"[^>]*step="1"/);
assert(html.includes('for="menu-opacity"'));assert(html.includes('90%'));
c.S.opacity=100;html=c.routeBody();assert.match(html,/data-set="opacity"[^>]*data-v="100"[^>]*aria-pressed="true"[^>]*>Off</);
console.log('PASS Settings: sign-in, access, versions, routes, recovery and bounded opacity with Off');
