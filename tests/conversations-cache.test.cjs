// #2110 CS5/C2/Q-A5: cached metadata cannot erase a confirmed draft or its immutable sent history.
const assert=require('node:assert/strict'),conversations=require('../src/conversations.js'),failures=[];
const ui=require('../src/ui-runtime.js');ui.defer=()=>0;ui.cancel=()=>{};
function instance(){return conversations.create({window:{crypto:require('node:crypto').webcrypto},document:{},menu:{querySelector(){return null;}},draw(){},navigate(){}});}
const owner={id:'owner',revision:2,title:'Retained',root:'fixture',text:'Independent unsent draft',caret:[2,4],files:[{id:'file',name:'draft.txt'}],sent:{root:'fixture',runtime:'claude',text:'Immutable submitted text',files:[{id:'sent',name:'sent.txt',hash:'a'.repeat(64)}]}};
const initial={projects:[],conversations:{items:[owner],complete:true,catalog_epoch:2,navigation:{selected:'owner',list:'recent'}}};
for(const [name,check]of [
 ['#2159 empty linked history is truthful without losing older discovery',()=>{
  const api=instance(),sent=[];globalThis.ipc={postMessage:text=>sent.push(JSON.parse(text))};
  const c={id:'owner',sent:{}},p={tasks:[],root:'fixture'},nodes=v=>Array.isArray(v)?v.flatMap(nodes):v?.props?[v,...nodes(v.props.children)]:[];
  api.linked(c,p);const loading=JSON.stringify(api.linkedTools());
  const receipt=(value,state='accepted')=>api.receive({dialogueReceipt:{...sent.at(-1),state,value,reason:state==='unknown'?'Invented unavailable board':undefined}});
  receipt({tasks:[],more:true});api.linked(c,p);let rendered=api.linkedTools();
  assert.match(JSON.stringify(rendered),/No linked tasks loaded/);assert(!JSON.stringify(rendered).includes('1–0'),'empty association has no inverted range');
  assert.match(loading,/Reading linked task history/,'pending collection cannot claim known empty');
  const disclosure=nodes(rendered).find(v=>v.type==='details');assert(disclosure&&!disclosure.props.open,'unscoped board pages do not clutter the primary document');
  assert(nodes(disclosure).some(v=>v.props['data-c']==='linked-older'),'older associations remain discoverable');
  api.linkedRead(c.id,undefined,2);receipt({tasks:[],more:false});api.linked(c,p);
  assert(nodes(api.linkedTools()).some(v=>v.props['data-c']==='linked-newer'),'empty later page can return to newer history');
  api.linkedRead(c.id);receipt({tasks:[],more:false});api.linked(c,p);
  assert(!nodes(api.linkedTools()).some(v=>v.type==='button'),'exhausted empty history adds no irrelevant controls');
  api.linkedRead(c.id);receipt(null,'unknown');api.linked(c,p);
  assert.match(JSON.stringify(api.linkedTools()),/Task history unknown: Invented unavailable board/);
  assert(!JSON.stringify(api.linkedTools()).includes('No linked tasks loaded'),'failed collection cannot claim empty');
  assert(nodes(api.linkedTools()).some(v=>v.props['data-c']==='linked-refresh'),'failed collection retains retry');
  p.tasks=Array.from({length:65},(_,n)=>({n:n+1,conversation:{id:c.id}}));api.linked(c,p);rendered=api.linkedTools();
  assert.match(JSON.stringify(rendered),/1–64 \/ 65 loaded/);assert(nodes(rendered).some(v=>v.props['data-c']==='linked-page'),'real associated rows retain bounded paging');
 }],
 ['#2159 image bytes participate in native resident eviction',()=>{const api=instance(),item={text:'text',image:{src:'data:image/png;base64,'+'a'.repeat(4096)}};assert(api.bytes(item)>4096,'image data cannot bypass the 32 MiB native cache ceiling');}],
 ['#2159 New leaves Finished scope and retains its own runtime',()=>{const api=instance();api.receive({...structuredClone(initial),ai:{runtime:{id:'claude'},runtimes:[{id:'claude',available:true},{id:'codex',available:true}]}});api.filter('finished',{projects:['fixture']});api.fresh({root:'fixture',runtime:'codex'});assert.equal(api.filterName(),'all','New cannot strand its draft in Finished');assert.equal(api.context().completed,null);assert.equal(api.local(api.current()).runtime,'codex','the per-request choice must survive native saves/recovery');}],
 ['lightweight refresh retains submitted text/files',()=>{const api=instance();api.receive(structuredClone(initial));api.receive({projects:[],conversations:{complete:true,items:[{id:'owner',revision:2,lazy:true,sent:{root:'fixture',runtime:'claude'}}]}});assert.equal(api.current().sent.text,owner.sent.text);assert.deepEqual(api.current().sent.files,owner.sent.files);assert.equal(api.current().text,owner.text);}],
 ['stale catalog absence cannot delete a confirmed draft',()=>{const api=instance();api.receive(structuredClone(initial));api.receive({projects:[],conversations:{complete:true,absence_complete:true,catalog_epoch:1,items:[]}});assert(!api.current().deleted);assert.equal(api.current().text,owner.text);assert.deepEqual(api.current().files,owner.files);}]
])try{check();console.log('PASS '+name);}catch(e){failures.push(name+': '+e.message);}
assert.deepEqual(failures,[]);
