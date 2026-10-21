// Chain: WS11 menu -> settings/project/task routes -> Back. Risk: a refresh changes route, Back loses context,
// or moving maintenance hides existing actions. DOM/native rendering is qualified separately by snapshots.
const fs=require('node:fs'),assert=require('node:assert/strict');
const launches=[];
const context={send:m=>launches.push(m),page:'home',taskFilter:null,working:null,workCard:null,ask:null,esc:s=>String(s),render:()=>{},
  document:{scrollingElement:{scrollTop:120}},menu:{querySelector:()=>null,querySelectorAll:()=>[{dataset:{viewKey:'scope'},open:true},{dataset:{viewKey:'counts'},open:false}]} };
require('./tray-harness.cjs').install(context);
(context.navigate('settings'),context.navigate('appearance'));
assert.equal(context.page,'appearance');assert.equal(context.document.scrollingElement.scrollTop,0);
(context.back());assert.equal(context.page,'settings');
(context.back());assert.equal(context.page,'home');assert.equal(context.document.scrollingElement.scrollTop,120);
assert.equal((JSON.stringify(context.disclosures)),'{"scope":true,"counts":false}');
(context.navigate('project'),context.navigate('project','review'),context.navigate('project',null,{project:true}));
assert.equal(context.working,null,'Work mode no longer opens inside the menu');
assert.deepEqual(launches.at(-1),{a:'workspace',open:{page:'operations',operation:'resources',root:'project'}});
assert.equal(context.page,'project');assert.deepEqual(context.taskFilter,['review']);
(context.back());assert.equal(context.page,'project');assert.equal(context.taskFilter,null);
(context.back(),context.back());assert.equal(context.page,'home');
// Render all five categories from the same snapshot, including empty and unknown states.
context.S={platform:'windows',font:20,host:'mac',now:1000,version:'candidate',projects:[],agents:null};
context.page='settings';let h=(context.routeBody());
for(const route of ['appearance','cadence','notifications','agents','services','about'])assert(h.includes('data-route="'+route+'"'));
context.page='appearance';assert((context.routeBody()).includes('Text size · 20 px'));
context.page='cadence';context.S.period='1d';context.S.history={bins:[],tz:'UTC',start:1,end:2,cap:1,since:null};assert((context.routeBody()).includes('hist'));
context.page='agents';assert((context.routeBody()).includes('Unknown: no status.json'));
context.S.projects=[{root:'/p',name:'alpha',host:'mac',version:null,age:null,focus:[],tasks:[]}];
context.page='services';h=(context.routeBody());assert(h.includes('Host maintenance in workspace'));assert(!h.includes('data-a="restart"'));assert(h.includes('status unknown'));assert(h.includes('Service'));
context.page='about';h=(context.routeBody());assert(h.includes('version unknown'));assert(h.includes('status unknown'));
// M14: OS denial reads Blocked with the OS setting and no test post; a project shows its override and effective mode
const kinds={ask:'sound',close:'off',failed:'banner',gone:'banner'};
context.S.notify={status:'blocked',host:kinds,projects:{'/p':{ask:[null,'sound'],close:['banner','banner'],failed:[null,'banner'],gone:[null,'banner']}}};
context.page='notifications';h=(context.routeBody());
assert(h.includes('Blocked by the OS'));assert(h.includes('data-a="notify-os"'));assert.equal((h.match(/notify-test[^>]*disabled/g)||[]).length,4);
assert(h.includes('data-kind="ask" data-v="sound" aria-pressed="true"'));
context.S.notify.status='allowed';h=(context.routeBody());assert(!h.includes('notify-os'));assert(!/notify-test[^>]*disabled/.test(h));
h=(context.notifyGroup({root:'/p'}));
assert(h.includes('data-kind="close" data-root="/p" data-v="banner" aria-pressed="true"'));assert(h.includes('data-kind="ask" data-root="/p" data-v="inherit" aria-pressed="true"'));
// M14 click on a closed task: no open row, so the project page shows its card from the notification until navigation
Object.assign(context,{TaskqProjectBlocks:require('../src/project-blocks.js'),CSS:{escape:s=>s},blockProject:p=>({name:p.name,board:{kind:'github',url:p.issues}}),
  S:{projects:[{root:'/p',name:'p',issues:'https://x/issues',tasks:[]}]}});
(context.page='home',context.openCard({root:'/p',n:8,kind:'close',title:'Ship it'}));
assert.equal(context.page,'/p');h=(context.notedCard(context.S.projects[0]));
assert(h.includes('data-card="https://x/issues/8"'));assert(h.includes('Closed'));assert(h.includes('Ship it'));assert(h.includes('href="https://x/issues/8"'));
(context.navigate('home'));assert.equal((context.notedCard(context.S.projects[0])),'');
console.log('PASS M14 notification settings, OS block, project override and closed-task card');
console.log('PASS WS11 nested Back, filter/scroll/disclosure restoration, settings categories, unknown probes/versions and maintenance destinations');

// Chain: M13 header/fit -> native size request -> reopened persisted dimensions.
// Risk: Top is coupled to Pin or a refresh overwrites the user's chosen size.
const sizing={devicePixelRatio:1.4,gutter:()=>8,S:{font:20,pinned:false,top:true,size:null},sent:[],document:{body:{style:{}}},menu:{style:{},querySelector:()=>null,getBoundingClientRect:()=>({height:421.4})},send:m=>sizing.sent.push(m)};
require('./tray-harness.cjs').install(sizing);
(sizing.fit());assert.deepEqual(JSON.parse(JSON.stringify(sizing.sent.pop())),{a:'size',d:1.4,g:8,w:360,h:422});
assert((sizing.head()).includes('data-a="top" aria-pressed="true"'));
sizing.S.pinned=true;sizing.S.top=false;assert((sizing.head()).includes('data-a="top" aria-pressed="false"'));
sizing.S.size=[480,600];(sizing.fit());assert.equal(sizing.sent.at(-1).h,600);
(sizing.manualSize=[500,300],sizing.fit());assert.equal(sizing.sent.at(-1).w,500);assert.equal(sizing.sent.at(-1).h,300);
console.log('PASS M13 independent header toggles, measured initial height and preserved manual size');

// Chain: all actual resize listeners -> lost capture -> guard release.
const events={},messages=[],drag={devicePixelRatio:1.4,innerWidth:320,innerHeight:400,send:m=>messages.push(m),addEventListener(){},
 document:{fonts:{ready:Promise.resolve()},addEventListener(){}},
 menu:{style:{},addEventListener:(type,fn)=>(events[type]??=[]).push(fn)}};
require('./tray-harness.cjs').install(drag).mount();
const fire=(type,event={})=>{for(const fn of events[type])fn(event);};
fire('pointerdown',{button:0,screenX:10,screenY:10,target:{closest:()=>true,setPointerCapture(){}},preventDefault(){}});
fire('pointermove',{screenX:30,screenY:25});fire('lostpointercapture');fire('lostpointercapture');
assert.equal(drag.resizing,null);assert.equal(messages.filter(m=>m.a==='resize-end').length,1);
assert.deepEqual(drag.manualSize,[340,415]);
console.log('PASS actual resize listeners release once after capture loss');

// A2/A4: header destinations and content-fit modes preserve the user's width.
assert.match(sizing.head(),/data-route="home"/);assert.match(sizing.head(),/data-route="settings"/);
for(const mode of ['bottom','top']){sizing.S.size_anchor=mode;sizing.fit();assert.equal(sizing.sent.at(-1).h,422);assert.equal(sizing.sent.at(-1).w,500);}
sizing.S.size_anchor='fixed';sizing.fit();assert.equal(sizing.sent.at(-1).h,300);
console.log('PASS A2/A4 header navigation and fixed/content-fit modes');
