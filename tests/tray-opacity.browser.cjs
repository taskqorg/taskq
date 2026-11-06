// M1.1 / #1812 A2–A3: range input reaches native IPC; every menu route adopts readback and strong body tokens.
const assert=require('node:assert/strict'),fs=require('node:fs'),{chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{for(const theme of ['light','dark']){
  const page=await browser.newPage({viewport:{width:360,height:900}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
  await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
  const palette=theme==='light'?'--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--review:#8250DF':'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--review:#BCA3EB';
  const opacity_palette=theme==='light'?'--work:#04402E;--ask:#4D2900;--failed:#651321;--ready:#2A3139;--muted:#2A3139;--review:#412870':'--work:#EBFDF6;--ask:#FFF9ED;--failed:#FFEFF2;--ready:#F8F9FB;--muted:#F8F9FB;--review:#F8F6FD';
  const state={display:require('./fixtures/display-model.json').display,theme,palette,opacity_palette,opacity:100,font:16,page:'appearance',host:'fixture',version:'4.6.40',now:1000,period:'1d',history:require('./fixtures/icon-history/day-light.json'),notify:{status:'allowed',host:{ask:'off',close:'off',failed:'off',gone:'off'},projects:{}},projects:[],ai:{draft:''},view:'icon'};
  await page.evaluate(s=>taskq.receive(s),state);
  const slider=page.getByLabel('Menu opacity',{exact:false});
  assert.equal(await slider.getAttribute('type'),'range');assert.equal(await slider.getAttribute('min'),'60');assert.equal(await slider.getAttribute('max'),'100');
  await slider.focus();await page.keyboard.press('Home');
  assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='set'&&'opacity'in m).at(-1)),{a:'set',opacity:60});
  await page.evaluate(s=>taskq.receive(s),{...state,opacity:60});
  assert.equal(await slider.getAttribute('aria-valuetext'),'60% opaque');
  await page.keyboard.press('ArrowRight');assert.equal(await page.evaluate(()=>messages.filter(m=>'opacity'in m).at(-1).opacity),61);
  await page.getByRole('button',{name:'Off',exact:true}).click();assert.equal(await page.evaluate(()=>messages.filter(m=>'opacity'in m).at(-1).opacity),100);
  for(const route of ['home','appearance','cadence','settings','notifications','agents','services','about','projects','attention']){
   await page.evaluate(s=>taskq.receive(s),{...state,page:route,opacity:60});
   assert(await page.locator('.menu').evaluate(e=>e.classList.contains('translucent')),route+' keeps the strengthened palette');
  }
  await page.evaluate(s=>taskq.receive(s),{...state,opacity:60,opacity_error:'Native alpha refused'});
  assert.match(await page.locator('[role=alert]').innerText(),/Native alpha refused/);
  await page.evaluate(s=>taskq.receive(s),{...state,opacity:60});
  // Consumer-owned defaults must not shadow the menu's stronger text colors.
  const colors=await page.evaluate(()=>{
   const root=document.querySelector('.content-body');
   for(const [cls,label,secondary] of [['tqp','Project body text','tqp-meta'],['qac','Answer body text','qac-muted'],['rsc','Resource body text','rsc-muted']]){const e=document.createElement('div');e.className=cls;e.textContent=label+' · ';const sub=document.createElement('span');sub.className=secondary;sub.textContent='Secondary';e.append(sub);root.append(e);}
   return ['.menu','.tqp','.qac','.rsc'].map(sel=>getComputedStyle(document.querySelector(sel)).color);
  });
  assert.deepEqual(colors,Array(4).fill(theme==='light'?'rgb(18, 24, 28)':'rgb(255, 255, 255)'));
  assert.deepEqual(await page.locator('.tqp-meta,.qac-muted,.rsc-muted').evaluateAll(es=>es.map(e=>getComputedStyle(e).color)),Array(3).fill(theme==='light'?'rgb(42, 49, 57)':'rgb(248, 249, 251)'));
  if(process.env.TASKQ_OPACITY_SHOTS){
   fs.mkdirSync(process.env.TASKQ_OPACITY_SHOTS,{recursive:true});
   for(const opacity of [100,90,60]){
    await page.evaluate(s=>{taskq.receive(s);document.body.style.background='repeating-conic-gradient(#000 0% 25%,#fff 0% 50%) 0 0/24px 24px';const m=document.querySelector('.menu');m.style.opacity=s.opacity/100;m.style.margin='12px';m.style.width='calc(100% - 24px)';m.style.height='calc(100% - 24px)';}, {...state,opacity});
    await page.screenshot({path:process.env.TASKQ_OPACITY_SHOTS+`/model-${theme}-${opacity}.png`});
   }
  }
  await page.evaluate(s=>taskq.receive(s),state);
  assert.equal(await page.locator('.menu').evaluate(e=>e.classList.contains('translucent')),false,'Off restores normal body colors');
  await page.setViewportSize({width:320,height:900});
  await page.evaluate(s=>taskq.receive(s),{...state,opacity:60,font:20});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'large text stays within a narrow menu');
  assert.deepEqual(await page.locator('.opacity-setting > *').evaluateAll(es=>es.filter(e=>e.scrollWidth>e.clientWidth+1).map(e=>e.tagName)),[],'opacity controls do not clip at 320 px / 20 px text');
  assert.deepEqual(errors,[]);await page.close();
  const settings=await browser.newPage();await settings.setContent(fs.readFileSync('build/web/tray.html','utf8').replace('<head>',"<head><script>window.taskqSurface='settings';</script>"));
  await settings.evaluate(s=>taskq.receive(s),{...state,opacity:60});
  assert.equal(await settings.locator('.menu').evaluate(e=>e.classList.contains('translucent')),false,'the Settings window stays opaque');
  assert.equal(await settings.locator('.menu').evaluate(e=>getComputedStyle(e).getPropertyValue('--work').trim()),theme==='light'?'#087F5B':'#38E8A0');
  await settings.close();
 }console.log('PASS M1.1: keyboard slider/Off IPC, route readback, shared body colors, failure visibility and opaque Settings; native alpha is separate');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
