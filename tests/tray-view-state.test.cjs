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
