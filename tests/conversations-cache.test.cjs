// #2110 CS5/C2/Q-A5: cached metadata cannot erase a confirmed draft or its immutable sent history.
const assert=require('node:assert/strict'),conversations=require('../src/conversations.js'),failures=[];
function instance(){return conversations.create({window:{crypto:require('node:crypto').webcrypto},document:{},menu:{querySelector(){return null;}},draw(){},navigate(){}});}
const owner={id:'owner',revision:2,title:'Retained',root:'fixture',text:'Independent unsent draft',caret:[2,4],files:[{id:'file',name:'draft.txt'}],sent:{root:'fixture',runtime:'claude',text:'Immutable submitted text',files:[{id:'sent',name:'sent.txt',hash:'a'.repeat(64)}]}};
const initial={projects:[],conversations:{items:[owner],complete:true,catalog_epoch:2,navigation:{selected:'owner',list:'recent'}}};
for(const [name,check]of [
 ['lightweight refresh retains submitted text/files',()=>{const api=instance();api.receive(structuredClone(initial));api.receive({projects:[],conversations:{complete:true,items:[{id:'owner',revision:2,lazy:true,sent:{root:'fixture',runtime:'claude'}}]}});assert.equal(api.current().sent.text,owner.sent.text);assert.deepEqual(api.current().sent.files,owner.sent.files);assert.equal(api.current().text,owner.text);}],
 ['stale catalog absence cannot delete a confirmed draft',()=>{const api=instance();api.receive(structuredClone(initial));api.receive({projects:[],conversations:{complete:true,absence_complete:true,catalog_epoch:1,items:[]}});assert(!api.current().deleted);assert.equal(api.current().text,owner.text);assert.deepEqual(api.current().files,owner.files);}]
])try{check();console.log('PASS '+name);}catch(e){failures.push(name+': '+e.message);}
assert.deepEqual(failures,[]);
