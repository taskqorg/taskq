// M1.2 / #1876 A2–A3: all fills fade, foreground stays opaque, modes and windows stay independent.
const assert=require('node:assert/strict'),fs=require('node:fs'),{chromium}=require('playwright');
const out=process.env.TASKQ_BACKGROUND_SHOTS;
const html=fs.readFileSync('build/web/tray.html','utf8');
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture',issues:'https://example.org/org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],tasks:['doing','review','ask','failed','ready','held'].map((state,i)=>({n:i+1,state,title:'Preserve readable task content over a busy desktop',revision:20+i,since:900,controls:[]}))};
const state={display:require('./fixtures/display-model.json').display,platform:'macos',font:16,host:'fixture',version:'fixture',now:1000,period:'1d',history:require('./fixtures/icon-history/day-light.json'),notify:{status:"allowed",host:{ask:"off",close:"off",failed:"off",gone:"off"},projects:{}},projects:[project],ai:{draft:''},view:'icon',opacity:60,opacity_mode:'background',background_opacity:0,backing_strength:98};

// Resolve actual computed CSS colors and every ancestor background against both extreme backdrop pixels.
// The two extremes also bound every pixel in the invented busy wallpaper used by the screenshots.
function inspect(){
 const root=document.querySelector('.menu'),canvas=document.createElement('canvas');canvas.width=canvas.height=1;
 const ctx=canvas.getContext('2d',{willReadFrequently:true});
 const rgba=color=>{ctx.clearRect(0,0,1,1);ctx.fillStyle=color;ctx.fillRect(0,0,1,1);return [...ctx.getImageData(0,0,1,1).data].map((v,i)=>i===3?v/255:v);};
 const lum=rgb=>rgb.reduce((s,c,i)=>s+[.2126,.7152,.0722][i]*(c/255<=.04045?c/255/12.92:((c/255+.055)/1.055)**2.4),0);
 const contrast=(a,b)=>{a=lum(a);b=lum(b);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);};
 const blend=(fg,bg)=>fg.slice(0,3).map((v,i)=>fg[3]*v+(1-fg[3])*bg[i]);
 const failures=[],opacity=[],samples=[];
 const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width&&r.height&&s.visibility==='visible'&&s.display!=='none';};
 const check=(e,label,min)=>{
  if(!visible(e))return;
  const chain=[];for(let a=e;a&&a!==document.body;a=a.parentElement){chain.unshift(a);if(Number(getComputedStyle(a).opacity)!==1)opacity.push(label+' ancestor '+a.className);}
  const fg=rgba(getComputedStyle(e).color);if(fg[3]!==1)opacity.push(label+' color alpha');
  if(e.closest(':disabled,[aria-disabled=true]'))return;
  for(const backdrop of [0,255]){let bg=[backdrop,backdrop,backdrop];for(const a of chain)bg=blend(rgba(getComputedStyle(a).backgroundColor),bg);
   const ratio=contrast(fg.slice(0,3),bg);samples.push(ratio);if(ratio<min)failures.push({label,ratio,color:getComputedStyle(e).color,bg});}
 };
 const walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT);let n;
 while((n=walker.nextNode()))if(n.textContent.trim()&&!n.parentElement.closest('script,style,svg'))check(n.parentElement,n.textContent.trim().slice(0,50),4.5);
 for(const e of root.querySelectorAll('button,input,select,textarea,[role=button],.ui-icon,.ui-icon *, .tqp svg, .tqp svg *, img')){
  if(e.matches('input[type=file]')&&!visible(e))continue;
  check(e,e.getAttribute('aria-label')||e.className,3);
 }
 const fills=[];
 const alpha=parseFloat(root.style.getPropertyValue('--background-alpha'))/100;
 for(const e of [root,...root.querySelectorAll('*')]){
  if(!visible(e)||e.closest('button,.counter,.badge,svg,img'))continue;
  const c=getComputedStyle(e),a=rgba(c.backgroundColor)[3];
  if(a>alpha+1/255)fills.push({element:e.outerHTML.slice(0,240),alpha:a});
 }
 return {failures,opacity,fills,minimum:Math.min(...samples),samples:samples.length};
}

(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{for(const theme of ['light','dark']){
  const page=await browser.newPage({viewport:{width:360,height:960}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(html);await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  const palette=theme==='light'?'--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--review:#8250DF':'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--review:#BCA3EB';
  const s={...state,theme,palette,page:'appearance'};await page.evaluate(s=>taskq.receive(s),s);
  assert(await page.locator('.menu').evaluate(e=>e.classList.contains('background-only')));
  assert.equal(await page.locator('.menu').evaluate(e=>e.classList.contains('translucent')),false,'saved whole-window value does not strengthen/fade background-mode content');
  const background=page.getByLabel('Background opacity');
  assert.equal(await page.getByLabel('Readability backing').count(),0);
  await background.focus();await page.keyboard.press('End');assert.deepEqual(await page.evaluate(()=>{const {context,...message}=messages.filter(m=>m.a==='set').at(-1);return message;}),{a:'set',background_opacity:100});
  await page.getByRole('button',{name:'Whole window',exact:true}).click();assert.deepEqual(await page.evaluate(()=>{const {context,...message}=messages.filter(m=>m.a==='set').at(-1);return message;}),{a:'set',opacity_mode:'whole'});
  assert.equal(await background.isDisabled(),true);assert.equal(await page.locator('#menu-opacity').isDisabled(),false);
  await page.getByRole('button',{name:'Background only',exact:true}).click();assert.deepEqual(await page.evaluate(()=>{const {context,...message}=messages.filter(m=>m.a==='set').at(-1);return message;}),{a:'set',opacity_mode:'background'});
  for(const background_opacity of [0,50])for(const route of ['home','appearance','settings','agents','notifications','services','about','projects','attention']){
   await page.evaluate(s=>taskq.receive(s),{...s,page:route,background_opacity});
   if(route==='home')await page.locator('.task-row .tqp-task-title>a').first().hover();
   const result=await page.evaluate(inspect);assert.deepEqual(result.opacity,[],route+' full foreground alpha');assert.deepEqual(result.fills,[],route+' all fills follow background opacity');
  }
  await page.evaluate(s=>taskq.receive(s),{...s,page:'home'});await page.locator('[data-a=view-editor]').click();
  assert.deepEqual((await page.evaluate(inspect)).fills,[],'floating fills clear at zero');
  assert.deepEqual((await page.evaluate(inspect)).opacity,[],'floating controls retain full alpha');
  await page.locator('#taskq-popover [data-a=popup-close]').first().click();
  await page.evaluate(s=>taskq.receive(s),{...s,opacity_error:'Native alpha refused'});assert.match(await page.locator('[role=alert]').innerText(),/Native alpha refused/);
  await page.evaluate(s=>taskq.receive(s),{...s,page:'appearance'});
  // Shared consumer foreground stays opaque when body fills clear.
  await page.evaluate(()=>{const body=document.querySelector('.content-body');const sample=document.createElement('div');sample.id='contrast-sample';sample.innerHTML='<div class="body"><span class="work">Work</span> <span class="review">Review</span> <span class="failed">Failed</span></div><div class="tqp">Project <small>Secondary</small></div><div class="qac">Answer <span class="qac-muted">Secondary</span></div><div class="rsc">Resource <span class="rsc-muted">Secondary</span><button disabled>Unavailable</button></div>';body.prepend(sample);});
  const shared=await page.evaluate(inspect);assert.deepEqual(shared.opacity,[]);assert.deepEqual(shared.fills,[],'shared body fills clear at zero');
  // Mutation proves the guard rejects a faded control and a faded ancestor.
  for(const selector of ['.content-body','#background-opacity','.ui-icon svg']){
   await page.locator(selector).first().evaluate(e=>e.style.opacity='.5');assert((await page.evaluate(inspect)).opacity.length>0,'opacity mutation must fail');await page.locator(selector).first().evaluate(e=>e.style.removeProperty('opacity'));
  }
  await page.evaluate(s=>taskq.receive(s),{...s,background_opacity:100});assert.deepEqual((await page.evaluate(inspect)).failures,[],'opaque mode retains contrast protection');
  await page.evaluate(()=>document.getElementById('contrast-sample')?.remove());
  await page.evaluate(s=>taskq.receive(s),s);
  await page.locator('.content-body').evaluate(e=>e.style.background='rgba(250,251,252,.98)');
  assert((await page.evaluate(inspect)).fills.length>0,'restoring the old backing must fail');
  await page.locator('.content-body').evaluate(e=>e.style.removeProperty('background'));
  assert.deepEqual((await page.evaluate(inspect)).fills,[]);
  if(out){fs.mkdirSync(out,{recursive:true});for(const wallpaper of ['light','dark']){
   await page.evaluate(({s,wallpaper})=>{taskq.receive(s);document.body.style.background=wallpaper==='light'?'repeating-conic-gradient(#fff 0% 25%,#999 0% 50%) 0 0/20px 20px':'repeating-conic-gradient(#000 0% 25%,#555 0% 50%) 0 0/20px 20px';const m=document.querySelector('.menu');m.style.margin='8px';m.style.width='calc(100% - 16px)';m.style.height='calc(100% - 16px)';},{s:{...s,page:'home'},wallpaper});
   const result=await page.evaluate(inspect);assert.deepEqual(result.fills,[]);assert.deepEqual(result.opacity,[]);
   await page.screenshot({path:out+`/${theme}-${wallpaper}-wallpaper.png`});console.log(theme,wallpaper,JSON.stringify(result));
  }await page.evaluate(s=>taskq.receive(s),s);await page.screenshot({path:out+`/${theme}-appearance.png`});}
  await page.setViewportSize({width:320,height:960});await page.evaluate(s=>taskq.receive(s),{...s,font:20});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'320 px / 20 px reflows');
  assert.deepEqual(await page.locator('.opacity-setting > *').evaluateAll(es=>es.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.tagName)),[],'new controls do not clip at 320 px / 20 px');
  assert.deepEqual(await page.locator('.opacity-setting input').evaluateAll(es=>es.filter(e=>e.getBoundingClientRect().right>e.parentElement.getBoundingClientRect().right+.5).map(e=>e.id)),[],'range endpoints stay inside their row');
  assert.deepEqual((await page.evaluate(inspect)).opacity,[]);assert.deepEqual(errors,[]);await page.close();
  for(const surface of ['settings','views','workspace']){
   const p=await browser.newPage();await p.setContent(surface==='workspace'?fs.readFileSync('build/web/workspace.html','utf8'):html.replace('<head>',`<head><script>window.taskqSurface='${surface}';</script>`));await p.evaluate(s=>taskq.receive(s),s);
   assert.equal(await p.locator('.menu').evaluate(e=>e.classList.contains('background-only')),false,surface+' stays opaque');await p.close();
  }
 }console.log('PASS M1.2: background IPC, mode isolation, cleared fills and opaque-mode AA, full foreground alpha and mutation sensitivity');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
