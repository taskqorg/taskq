// M8/UI02: stale/inapplicable view data never becomes task truth or an action.
const assert=require('node:assert/strict'),{normalize}=require('../src/tray-view-state.js');
const routes={home:{},projects:{},settings:{},appearance:{parent:'settings'},future:{parent:'settings'}};
const snapshot={projects:[{root:'/alpha'}]},base={schema:1,current:{page:'/alpha',project:'/alpha',taskFilter:['doing','unknown','doing','ready'],aiRoot:'/alpha',working:{'/alpha':true,'/removed':true},scroll:120,open:{scope:true,obsolete:'not boolean'},pending:{action:'close'}},trail:[]};
const saved=normalize(base,snapshot,routes);
assert.deepEqual(saved.current.taskFilter,['doing','ready']);assert.deepEqual(saved.current.working,{'/alpha':true});
assert.deepEqual(saved.current.open,{scope:true});assert.equal(saved.current.aiRoot,'/alpha');assert.equal(saved.current.pending,undefined);
for(const [current,page] of [[{page:'/removed',project:'/removed'},'projects'],[{page:'removed',parent:'settings'},'settings'],[{page:'unknown'},'home'],[{page:'toString'},'home'],[{page:'future'},'future']]){
 const result=normalize({...base,current:{...current,scroll:-10,aiRoot:'/removed',taskFilter:['obsolete']}},snapshot,routes).current;
 assert.equal(result.page,page);assert.equal(result.scroll,0);assert.equal(result.aiRoot,null);
}
assert.equal(normalize({...base,schema:2},snapshot,routes).current.page,'home');
assert.equal(normalize(base,{projects:[]},routes).current.working,null);
assert.deepEqual(normalize({...base,trail:[{page:'/removed',project:'/removed'},{page:'settings',scroll:20,workCard:{secret:'draft'}}]},snapshot,routes).trail.map(v=>[v.page,v.scroll,v.workCard]),[['settings',20,undefined]]);
assert.equal(normalize({...base,trail:Array(40).fill({page:'home'})},snapshot,routes).trail.length,32);
console.log('PASS M8/UI02 declared routes, OR filters, selected project, groups, invalid/schema fallback, bounded Back and no action/draft persistence');
// #2140 C2/C7: accumulated Back entries must fit native storage without losing current source/focus.
const workspaceRoutes={conversations:{},operations:{parent:'conversations'}};
const current={page:'conversations',answer:{root:'/alpha',board:'https://example.org/issues',task:112,revision:71},scroll:123,open:{reader:true},focus:{tag:'BUTTON',key:'{}',index:0}};
const large=normalize({schema:1,current,trail:Array.from({length:32},()=>({...current,open:Object.fromEntries(Array.from({length:80},(_,i)=>['x'.repeat(180)+i,true]))}))},snapshot,workspaceRoutes,'conversations');
assert(new TextEncoder().encode(JSON.stringify(large)).length<=65536,'workspace Back state exceeds native 64 KiB; navigation save refuses');
assert.deepEqual(large.current.answer,current.answer);assert.deepEqual(large.current.focus,current.focus);assert.equal(large.current.scroll,123);
assert(large.trail.length>0,'retain newest Back contexts within the bound');
// #2168 C2/C7: accumulated off-screen disclosures must not block saving current navigation.
const disclosures=Object.fromEntries(Array.from({length:1800},(_,i)=>['old-thread:'+String(i).padStart(4,'0')+':'+ '界'.repeat(50),i%2===0]));
const field={tagName:'TEXTAREA',dataset:{cField:'text'}},visible={dataset:{viewKey:'current-source'},open:true};
let mounted=[visible];
const menu={contains:()=>true,querySelector:()=>({scrollTop:123}),querySelectorAll:s=>s.startsWith('details')?mounted:s==='TEXTAREA'?[field]:[]};
const view={...current,S:snapshot,disclosures:{...disclosures,'current-source':false}},sent=[];
const controller=require('../src/tray-view-state.js').create(view,{menu,document:{activeElement:field},send:m=>sent.push(m),defer:()=>0,cancel(){},routes:workspaceRoutes,trail:[],root:'conversations'});
controller.hydrate({page:'conversations'});controller.flush();
assert.equal(sent.length,1,'old disclosure metadata cannot leave current navigation unsaved');
const navigation=sent[0].navigation;
assert(new TextEncoder().encode(JSON.stringify(navigation)).length<=65536);
assert.deepEqual(navigation.current.answer,current.answer);assert.equal(navigation.current.scroll,123);
assert.deepEqual(navigation.current.focus,{tag:'TEXTAREA',key:JSON.stringify(field.dataset),index:0});
assert.equal(navigation.current.open['current-source'],true,'visible disclosure wins and survives');
assert(Object.hasOwn(navigation.current.open,Object.keys(disclosures).at(-1)),'newest off-screen disclosure retained');
assert(!Object.hasOwn(navigation.current.open,Object.keys(disclosures)[0]),'oldest off-screen disclosure leaves saved metadata first');
assert.equal(Object.keys(view.disclosures).length,1801,'bounded persistence does not reset in-window context');
controller.flush();assert.equal(sent.length,1,'unchanged navigation is not written again');
mounted=Object.entries(disclosures).map(([key,open])=>({dataset:{viewKey:key},open}));
controller.flush();assert.equal(sent.length,1,'oversized mounted context cannot overwrite the prior durable navigation');
assert.match(view.navigationError,/exceeds 64 KiB/,'an irreducible current view still names its storage failure');
console.log('PASS bounded workspace persistence keeps current source, visible disclosures, scroll and focus');
