// Chain: Product, M2, M3, M1.2, R12 — UX30-01..05 alignment, retained Settings controls and one sticky counter scroll.
// Invented observations; intercepted IPC cannot qualify native rendering, board writes or OS folder opening.
'use strict';
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const {display}=require('./fixtures/display-model.json');
const before=process.env.POLISH_BEFORE,out=process.env.POLISH_EVIDENCE,baseline=process.env.POLISH_BASELINE==='1';
const routes=['settings','appearance','notifications','cadence','hosts','work','agents','projects','delivery','services','recovery','about'];
const tasks=display.states.flatMap(({id},i)=>Array.from({length:3},(_,j)=>({n:100+i*3+j,state:id,title:'Keep all controls and the complete task title '+(i*3+j),revision:900+i*3+j,age:'4m',since:900,controls:id==='held'?['start']:['stop','later']})));
const project={root:'/fixture/alpha',name:'Alpha',board_kind:'github',issues:'https://example.org/org/alpha/issues',host:'fixture',alive:true,complete:true,tasks_live:true,tasks_age_seconds:0,age:1,age_text:'1s',observed:1000,focus:['all'],labels:[],tasks,delivery:{preset:'auto-merge',revision:3},access:[{runtime:'codex',requested:'full',grant:null,effective:null,refused:'host grant missing'}]};
const state={platform:'macos',host:'fixture',now:1000,version:'fixture',font:16,theme:'dark',view:'icon',size_anchor:'auto',service:{state:'running',pid:42,fresh:true},projects:[project,{...project,name:'Beta',root:'/fixture/beta',board_kind:'gitlab',issues:'https://gitlab.example.org/org/beta/-/issues',tasks:tasks.slice(0,8)}],display,ai:{draft:''},notify:{status:'blocked',host:{ask:'on',close:'off',failed:'on',gone:'off'},projects:{}},stop_modes:true,messages:{complete:true,count:0},agents:{at:900,age:'1m',fresh:true,rows:[{runtime:'codex',state:'expired',version:'fixture',account:'invented@example.org',method:'ChatGPT',expires:950,next:'Ask the PM to sign in'}]},period:'1d',opacity:90,opacity_mode:'whole',background_opacity:37,history:{start:0,end:1000,tz:'UTC',peak:2,bins:[{start:0,end:1000,counts:[1,2,3,4],missing:[],ongoing:true}],svg:'<svg viewBox="0 0 22 22"></svg>'}};
state.palette=state.opacity_palette='--work:light-dark(#087F5B,#38E8A0);--ask:light-dark(#995200,#FFC247);--failed:light-dark(#C92641,#FF6378);--ready:light-dark(#536171,#B9C6D8);--review:light-dark(#8250DF,#BCA3EB)';
const html=fs.readFileSync(baseline?before+'/tray.html':'build/web/tray.html','utf8');
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const errors=[],failures=[],parity={},matrix=[];if(out)fs.mkdirSync(out,{recursive:true});
 const open=async(s=state,width=360,touch=false)=>{const p=await browser.newPage({viewport:{width,height:560},hasTouch:touch});p.on('pageerror',e=>errors.push(e.message));await p.setContent(html);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))}});await p.evaluate(s=>taskq.receive(s),s);return p;};
 const check=(ok,why)=>{if(!ok)failures.push(why);};
 const shot=async(p,name)=>{if(out)await p.screenshot({path:out+'/'+name+'.png'});};
 let p=await open();
 const c=p.locator('.content');
 for(const [name,fraction] of [['top',0],['middle',.5],['bottom',1]]){
  await c.evaluate((e,f)=>e.scrollTop=f*(e.scrollHeight-e.clientHeight),fraction);await p.evaluate(()=>new Promise(requestAnimationFrame));
  const r=await p.locator('.filter-header').evaluate(e=>{const s=e.closest('.content'),a=e.getBoundingClientRect(),b=s.getBoundingClientRect();return {top:a.top,bottom:a.bottom,start:b.top,end:b.bottom,position:getComputedStyle(e).position,fill:getComputedStyle(e).backgroundColor};});
  if(fraction)check(Math.abs(r.top-r.start)<=1&&r.bottom<=r.end,'sticky row fully visible at '+name);
  check(r.fill!=='rgba(0, 0, 0, 0)','opaque counter background at '+name);await shot(p,'menu-'+name);
 }
 await c.evaluate(e=>e.scrollTop=0);
 const edges=await p.locator('.task-stage').evaluateAll(es=>es.map(e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect(),g=e.querySelector('svg').getBoundingClientRect();return {w:r.width,h:r.height,center:Math.abs((r.left+r.right-g.left-g.right)/2),edge:e.nextElementSibling.getBoundingClientRect().left,target:parseFloat(getComputedStyle(e.closest('.menu')).getPropertyValue('--target'))};}));
 check(edges.every(e=>e.w===e.target&&e.h===e.target&&e.center<1)&&new Set(edges.map(e=>e.edge)).size===1,'eight states share centered target-sized slots and identity edge');
 const rhythm=await p.locator('.project-shortcuts').evaluateAll(es=>es.every(e=>{const s=getComputedStyle(e);return parseFloat(s.marginTop)>=Math.max(16,parseFloat(s.fontSize))&&parseFloat(s.marginBottom)===4;}));check(rhythm,'project heading rhythm');
 // Source-control parity includes hidden/disabled controls, exact destinations and values, not incidental heading text.
 const controls=async p=>p.locator('.content :is(button,a,input,select)').evaluateAll(es=>es.map(e=>({tag:e.tagName,data:{...e.dataset},label:e.getAttribute('aria-label')||e.textContent.trim(),title:e.title,href:e.getAttribute('href'),disabled:e.disabled||false,value:e.matches('input,select')?e.value:null})).sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b))));
 for(const route of routes){await p.evaluate(s=>taskq.receive(s),{...state,page:route});parity[route]=await controls(p);if(['appearance','agents'].includes(route))await shot(p,route);}
 if(before&&!baseline){const old=JSON.parse(fs.readFileSync(before+'/controls.json'));for(const route of routes)assert.deepEqual(parity[route],old[route],'retained controls on '+route);}
 if(baseline){fs.writeFileSync(before+'/controls.json',JSON.stringify(parity,null,2)+'\n');}
 await p.close();
 if(!baseline){
  // M3/M1.2: wheel scrolling over the actual controls keeps one content owner and an opaque, hittable strip.
  for(const opacity_mode of ['whole','background']){
   p=await open({...state,opacity_mode,background_opacity:0});
   const scroll=p.locator('.content'),strip=p.locator('.filter-header'),chips=await strip.locator('button').evaluateAll(es=>es.map(e=>[e.dataset.state,e.getAttribute('aria-label'),e.textContent]));
   await p.locator('.task-row').nth(2).hover();await p.mouse.wheel(0,650);
   await p.waitForFunction(()=>document.querySelector('.filter-header').classList.contains('pinned'));
   assert(await strip.evaluate(e=>{const r=e.getBoundingClientRect(),c=e.closest('.content').getBoundingClientRect(),s=getComputedStyle(e);return Math.abs(r.top-c.top)<1&&r.bottom<c.bottom&&s.backgroundColor!=='rgba(0, 0, 0, 0)'&&s.boxShadow!=='none';}));
   await strip.hover();const at=await scroll.evaluate(e=>e.scrollTop);await p.mouse.wheel(0,280);await p.waitForFunction(at=>document.querySelector('.content').scrollTop>at,at);
   assert.deepEqual(await strip.locator('button').evaluateAll(es=>es.map(e=>[e.dataset.state,e.getAttribute('aria-label'),e.textContent])),chips,'scroll cannot change counts, order or accessible names');
   await p.locator('.task-row').last().locator('.tqp-task-title a').focus();
   assert(await p.locator('.task-row').last().locator('.tqp-task-title a').evaluate(e=>{const a=e.getBoundingClientRect(),b=document.querySelector('.filter-header').getBoundingClientRect(),c=e.closest('.content').getBoundingClientRect();return a.top>=b.bottom&&a.bottom<=c.bottom;}),'keyboard destination is not masked by sticky counters');
   await scroll.evaluate(e=>e.scrollTop=0);await p.waitForFunction(()=>!document.querySelector('.filter-header').classList.contains('pinned'));await p.close();
  }

  p=await open();await p.locator('[data-route=settings]').click();assert.equal(await p.locator('.content').evaluate(e=>e.style.scrollPaddingTop),'','Home counter clearance cannot leak into Settings');await p.locator('[data-a=back]').click();assert(await p.locator('.content').evaluate(e=>parseFloat(e.style.scrollPaddingTop)>0),'Back restores counter clearance');await p.close();
  // Matrix checks descendants as well as the root: overflow clipping must not masquerade as fit.
  for(const platform of ['macos','windows'])for(const theme of ['dark','light'])for(const font of [14,16,18,20])for(const width of [320,360]){
   const s={...state,platform,theme,font,projects:[{...project,name:'Infrastructure_'+('long_identifier_'.repeat(4)),tasks:tasks.map(t=>({...t,title:'Unbroken_'+('long_identifier_'.repeat(6))}))}]};
   p=await open(s,width);
   for(const route of ['home',...routes]){
    await p.evaluate(s=>taskq.receive(s),{...s,page:route});
    const bad=await p.locator('.content-body,.project-group,.section-heading,.settings-disclosure,.settings-widget,.task-row').evaluateAll(es=>es.filter(e=>e.clientWidth&&e.scrollWidth>e.clientWidth+1).map(e=>e.className));assert.deepEqual(bad,[],[platform,theme,font,width,route].join('/'));
    assert(await p.locator('.section-heading').evaluateAll(es=>es.every(e=>{const s=getComputedStyle(e);return parseFloat(s.paddingTop)>=Math.max(16,parseFloat(s.fontSize))&&parseFloat(s.paddingBottom)===4;})),'Settings heading rhythm');
    matrix.push([platform,theme,font,width,route]);
   }await p.close();
  }
  p=await open({...state,font:20},320,true);assert(await p.locator('.task-stage').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().width>=44&&e.getBoundingClientRect().height>=44)));await p.close();
 }
 assert.deepEqual(errors,[]);if(out)fs.writeFileSync(out+'/checks.json',JSON.stringify({failures,matrix,errors},null,2)+'\n');assert.deepEqual(failures,[]);console.log('PASS UX30-01..05: sticky top/middle/bottom and wheel, status slots, headings, 12-route inventory'+(before?' and source-control parity':' (before artifact not supplied)')+'; '+matrix.length+' geometry cells');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
