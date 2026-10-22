// #1615 A1/A2/A4: shared bytes, exact correlation, safe handoff, and no input in capture.
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),crypto=require('node:crypto');
const q=require('../src/pm-composer.js'),f=require('./fixtures/pm-requests.json'),r=f.request;
assert.deepEqual(q.request(r,r.text,r.op_id,r.issued_at),r);
for(const text of ['','   ','é'.repeat(32769)])assert.throws(()=>q.request(r,text,r.op_id,r.issued_at));
const native={state:'launched',host:'invented-host',runtime:'claude',session:'invented-session',url:'https://claude.ai/code/session_invented'};
const receipt={schema:f.schema,actor_id:r.actor_id,op_id:r.op_id,revision:r.revision,outcome:'accepted',request_id:'a'.repeat(64),native};
assert.deepEqual(q.receipt(receipt,r),receipt);
for(const key of ['actor_id','op_id','revision'])assert.equal(q.receipt({...receipt,[key]:'other'},r).outcome,'unknown');
assert.equal(q.receipt({...receipt,native:null},r).outcome,'unknown');
for(const url of ['javascript:alert(1)','https://example.org/session','codex://threads/invented'])assert(!JSON.stringify(q.handoff({...native,url})).includes('href'));
assert(JSON.stringify(q.handoff(native)).includes('Open session'));
const manifest=require('../docs/design/pm-composer.sources.json');
for(const [path,hash] of Object.entries(manifest.files))assert.equal(crypto.createHash('sha256').update(fs.readFileSync(path,'utf8').replace(/\r\n/g,'\n')).digest('hex'),hash,path);
const c={TextEncoder,crypto:crypto.webcrypto,setTimeout,clearTimeout};vm.createContext(c);vm.runInContext(fs.readFileSync('build/web/pm-composer.js','utf8'),c);
assert.deepEqual(JSON.parse(JSON.stringify(c.TaskqPmComposer.request(r,r.text,r.op_id,r.issued_at))),r);
console.log('PASS PM fixture input, immutable identity, receipt correlation, safe native handoff and released bundle provenance');
