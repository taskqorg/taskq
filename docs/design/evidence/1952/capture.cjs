// #1952 A5: actual bundled header output; invented board input, no board or native effects.
// node docs/design/evidence/1952/capture.cjs <baseline tray.html> <baseline SHA>
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),{execFileSync}=require('node:child_process'),{chromium}=require('playwright');
const root=path.resolve(__dirname,'../../../..'),before=fs.readFileSync(process.argv[2],'utf8'),after=fs.readFileSync(path.join(root,'build/web/tray.html'),'utf8');
const views=require(path.join(root,'src/task-views.js')),display=require(path.join(root,'tests/fixtures/display-model.json')).display;
const task=(n,state)=>({n,state,title:'Illustrative task '+n,revision:n+50,since:900,controls:[]});
const project={root:'/fixture/taskq',name:'taskq',issues:'https://example.org/org/taskq/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],tasks:['ready','ready','doing','review','waiting','ask','ask','failed','later','held'].map((s,i)=>task(i+1,s))};
const palette='--work:light-dark(#087F5B,#38E8A0);--ask:light-dark(#995200,#FFC247);--failed:light-dark(#C92641,#FF6378);--ready:light-dark(#536171,#B9C6D8);--muted:light-dark(#536171,#B9C6D8);--review:light-dark(#8250DF,#BCA3EB)';
(async()=>{const browser=await chromium.launch(require(path.join(root,'tests/browser.cjs'))());try{
 const cells=[];
 for(const theme of ['light','dark'])for(const questions of [true,false])for(const collapsed of [false,true]){
  const state={platform:'windows',font:16,theme,palette,host:'fixture',now:1000,version:'fixture',service:{state:'running'},display,projects:[{...project,tasks:project.tasks.filter(t=>questions||t.state!=='ask')}],navigation:{schema:1,current:{page:'home'},dashboard:{collapsed:collapsed?[views.identity(project)]:[],views:[{id:'all',name:'All tasks',working:{states:['ready']}}]}}};
  const shots=[];
  for(const html of [before,after]){const p=await browser.newPage({viewport:{width:320,height:760},deviceScaleFactor:1});await p.setContent(html);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))}});await p.evaluate(s=>taskq.receive(s),state);await p.evaluate(()=>document.fonts.ready);shots.push((await p.locator('.project-group').screenshot()).toString('base64'));await p.close();}
  cells.push({label:theme+' · '+(questions?'2 questions':'no questions')+' · '+(collapsed?'collapsed':'expanded'),shots});
 }
 const p=await browser.newPage({viewport:{width:680,height:1200},deviceScaleFactor:1});
 await p.setContent('<style>body{margin:16px;font:14px Segoe UI,sans-serif;background:#e4e7eb;color:#243039}h1{font-size:18px}section{margin:12px 0}h2{font-size:13px;margin:4px 0}.pair{display:grid;grid-template-columns:1fr 1fr;gap:8px}img{display:block;width:100%}small{display:block;margin-bottom:4px}</style><h1>Tasks header · before / after</h1><p>320px menu · 16px text · browser output · invented data</p>'+cells.map(c=>'<section><h2>'+c.label+'</h2><div class="pair">'+c.shots.map((s,i)=>'<div><small>'+(i?'After':'Before')+'</small><img src="data:image/png;base64,'+s+'"></div>').join('')+'</div></section>').join(''));
 await p.screenshot({path:path.join(__dirname,'comparison.png'),fullPage:true});await p.close();
 fs.writeFileSync(path.join(__dirname,'capture.json'),JSON.stringify({before:process.argv[3],after:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),browser:browser.version(),node:process.version,platform:process.platform,viewport:{width:320,height:760},font:'Segoe UI 16px',scale:1,bundle_sha256:{before:crypto.createHash('sha256').update(before).digest('hex'),after:crypto.createHash('sha256').update(after).digest('hex')},cases:cells.map(c=>c.label)},null,2)+'\n');
 console.log('Captured eight before/after header cases in comparison.png');
 }finally{await browser.close()}})().catch(e=>{console.error(e);process.exitCode=1});
