// Render build/web/tray.html with synthetic tasks: row ages, accepted toast, Answered group, unknown receipt.
const fs=require('fs'),{chromium}=require('playwright');
const [html,out]=process.argv.slice(2);
(async()=>{const b=await chromium.launch({channel:'chrome'});
for(const theme of ['light','dark']){
const p=await b.newPage({viewport:{width:340,height:700},deviceScaleFactor:2,colorScheme:theme});
await p.setContent(fs.readFileSync(html,'utf8'));
await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>window.messages.push(JSON.parse(s))};});
const board='https://github.com/o/r/issues',root='project',now=1760000000;
const t=(n,state,title,since,extra={})=>({n,state,title,revision:1,url:board+'/'+n,since,...extra});
const ask=(n,title)=>t(n,'ask',title,now-600,{card:title,question:{parsed:false,text:title,choices:[]}});
const state={page:'attention',now,font:16,theme,host:'mac',version:'dev',platform:'macos',icons:{},projects:[{root,name:'demo',issues:board,alive:true,complete:true,age:5,focus:['all'],labels:[],closed:3,
 tasks:[ask(1101,'Old question A'),ask(1102,'Old question B'),ask(1103,'Old question C'),ask(1104,'Old question D'),t(1110,'ask','Pick the release window',now-7200),t(1111,'doing','Fix tray resize',now-300),t(1112,'ready','Write release notes',null),t(1113,'failed','Flaky check',now-259200)],cap_saved:{error:'not configured'}}],ai:{draft:''},notify:{projects:{}}};
await p.evaluate(s=>taskq.receive(s),state);
await p.evaluate(()=>document.querySelectorAll('details').forEach(d=>d.open=true));
for(let i=0;i<4;i++){await p.getByRole('button',{name:'Later',exact:true}).nth(i).click();}
const ops=await p.evaluate(()=>messages.filter(m=>m.a==='answer').map(m=>m.op));
state.projects[0].tasks=state.projects[0].tasks.map(x=>x.state==='ask'&&x.n<1110?{...x,state:'later',since:now-5}:x);
await p.evaluate(s=>taskq.receive(s),state);
for(const [i,op] of ops.entries())await p.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:op.id,state:i===0?'unknown':'accepted',reason:i===0?'No bridge receipt received':'',next:i===0?'Check delivery with the same operation':''});
await p.evaluate(()=>document.querySelectorAll('details').forEach(d=>d.open=true));
await p.screenshot({path:out.replace('.png','-'+theme+'.png')});
}
await b.close();})();
