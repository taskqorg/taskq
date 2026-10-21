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
