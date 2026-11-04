const fs=require('fs'),path=require('path'),https=require('https'),{execSync}=require('child_process');
const I=path.join(process.env.LOCALAPPDATA,'taskq'),log=process.argv[2];
let last='';
function snap(){let s={};try{s.dirs=fs.readdirSync(path.join(I,'ui')).sort().map(n=>{const st=fs.statSync(path.join(I,'ui',n));return n+'@'+st.mtime.toISOString()});}catch(e){s.dirs='ERR '+e.code}
try{s.state=fs.readFileSync(path.join(I,'ui','state.json'),'utf8')}catch(e){s.state=null}
try{s.service=JSON.parse(fs.readFileSync(path.join(I,'service'),'utf8')).pid}catch{}
try{s.tray=fs.readFileSync(path.join(I,'tray'),'utf8').trim()}catch{}
return s}
function tick(){const s=snap(),k=JSON.stringify(s);if(k!==last){last=k;fs.appendFileSync(log,JSON.stringify({t:new Date().toISOString(),...s})+'\n')}}
tick();const iv=setInterval(tick,500);setTimeout(()=>process.exit(0),3*3600e3);
