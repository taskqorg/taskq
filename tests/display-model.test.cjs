// Chain: #1592 A4 one display model. Risk: the tray page shows other counts, order, state names or age text than the
// CLI prints for the same board. The fixture is the Rust model and CLI text (snapshot.rs cli_and_page_model_agree_on_one_board).
const assert=require('node:assert/strict');
const fx=require('./fixtures/display-model.json');
const c={S:{now:fx.now,host:'mac',projects:fx.projects,display:fx.display,period:'1d',history:{bins:[],tz:'UTC',start:1,end:2,since:null},notify:{projects:{}}},page:'project'};
require('./tray-harness.cjs').install(c);
const page=c.project(fx.projects[0]).replace(/<svg[\s\S]*?<\/svg>/g,'');
// counts: the CLI counts line and the page counters read one tally
const n=s=>fx.display.all.counts.find(x=>x.state===s).n;
assert.equal(fx.cli.split('\n')[0],'In work '+(n('doing')+n('review'))+' · Waiting for answer '+n('ask')+' · Ready '+n('ready'));
for(const {state,name,n:v} of fx.projects[0].counts)if(v)assert(page.includes('aria-label="a · '+name+': '+v+'"'),state);
// order and labels: page groups follow the strip order and the model names; rows inside a group follow `rank`
const groups=[...page.matchAll(/<div class="row meta"><span>([^<]+)<\/span>/g)].map(m=>m[1]);
assert.deepEqual(groups,fx.display.states.filter(s=>fx.projects[0].tasks.some(t=>t.state===s.id)).map(s=>s.name));
const shown=[...page.matchAll(/data-card="[^"]*\/(\d+)"/g)].map(m=>Number(m[1]));
assert.deepEqual(shown,[...fx.projects[0].tasks].sort((a,b)=>a.rank-b.rank).map(t=>t.n));
const cliReady=[...fx.cli.matchAll(/^\| \[#(\d+) [^\n]*\| ready \|/gm)].map(m=>Number(m[1]));
assert.deepEqual(cliReady,shown.filter(k=>fx.projects[0].tasks.find(t=>t.n===k).state==='ready'));
for(const t of fx.projects[0].tasks.filter(t=>['held','later'].includes(t.state)))assert(fx.cli.includes(t.state_name+': [#'+t.n+' '),t.state);
// age: the CLI stuck age and the page row age are one text
const ask=fx.projects[0].tasks.find(t=>t.state==='ask');
assert(fx.cli.includes('(ask, '+ask.age+')'));
assert(page.includes('<span class="tqp-age">'+ask.age+'</span>'));
// mode: the paused project reads paused on the page as the CLI Paused line does
assert.equal(fx.projects[0].mode,'paused');assert(page.includes('Paused: running tasks finish'));
