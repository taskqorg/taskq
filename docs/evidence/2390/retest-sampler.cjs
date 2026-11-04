// A4 retest store sampler. Reads only: UI store, draft and reading records for one task, stored attachment bytes and the binary hash.
// Usage: node retest-sampler.cjs <log> <task number> <hours>. Writes a line when a value changes, plus a heartbeat every 5 minutes.
const fs=require('fs'),path=require('path'),crypto=require('crypto');
const I=path.join(process.env.LOCALAPPDATA,'taskq'),log=process.argv[2],n=process.argv[3],hours=Number(process.argv[4]||12);
const sha=b=>crypto.createHash('sha256').update(b).digest('hex'),read=f=>{try{return fs.readFileSync(f,'utf8')}catch{return null}};
let binary=null,binaryAt=0,last='',beat=0;
function snap(){
  const s={};
  try{s.dirs=fs.readdirSync(path.join(I,'ui')).sort().map(d=>d+'@'+fs.statSync(path.join(I,'ui',d)).mtime.toISOString());}catch(e){s.dirs='ERR '+e.code}
  try{s.state=JSON.parse(read(path.join(I,'ui','state.json')));s.state_mtime=fs.statSync(path.join(I,'ui','state.json')).mtime.toISOString()}catch{s.state=null}
  try{s.service=JSON.parse(read(path.join(I,'service'))).pid}catch{}
  s.tray=(read(path.join(I,'tray'))||'').trim();
  if(Date.now()-binaryAt>600e3){binaryAt=Date.now();try{const v=(read(path.join(I,'current'))||'').trim();binary={version:v,sha256:sha(fs.readFileSync(path.join(I,'releases',v,'taskq.exe')))}}catch(e){binary={error:e.code}}}
  s.binary=binary;
  try{
    const r=JSON.parse(read(path.join(I,'tray-requests.json'))),a=Object.entries(r.answers||{}).find(([k])=>k.endsWith('/taskq/issues#'+n))?.[1],t=r.conversation_navigation?.threads,id=Object.keys(t?.positions||{}).find(k=>k.endsWith('/taskq/issues/'+n)),p=id?t.positions[id]:null;
    s.selected_is_task=!!t?.selected?.endsWith('/taskq/issues/'+n);
    s.anchor=p?{at:p.at,i:p.i,y:p.y,attempt:p.attempt,source_sha:p.g?sha(p.g).slice(0,12):null}:null;
    s.draft=a?{text:a.draft,op:a.op,receipt:a.receipt,files:(a.files||[]).map(f=>{let stored=null;try{stored=sha(fs.readFileSync(path.join(I,'setup','reply-files',f.hash,f.name)))}catch(e){stored=e.code}return {name:f.name,bytes:f.bytes,hash:f.hash,stored_sha256:stored,pending:!!f.pending,error:f.error||null}})}:null;
  }catch(e){s.store_error=e.code||e.name}
  return s;
}
function tick(){const s=snap(),k=JSON.stringify(s),now=Date.now();if(k!==last||now-beat>300e3){const changed=k!==last;last=k;beat=now;fs.appendFileSync(log,JSON.stringify({t:new Date().toISOString(),changed,...s})+'\n')}}
tick();setInterval(tick,500);setTimeout(()=>process.exit(0),hours*3600e3);
