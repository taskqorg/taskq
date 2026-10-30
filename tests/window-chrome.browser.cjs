// #1660 A1–A5 (R07–R09, R21–R26): one stable header action row, Home/Back, one Conversations entry, version-only
// footer and the Settings surface; window drag, z-order, hide and raise are native and not measured here.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.env.CHROME_EVIDENCE;
const task=(n,state)=>({n,state,title:'Keep the task title readable '+n,revision:n+50,since:900,created:500,steps:[{state,start:900,end:null,complete:false}],controls:['stop','later']});
const long={root:'/fixture/long',name:'Infrastructure provisioning and observability pipeline',host:'fixture',issues:'https://example.org/org/long/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],tasks:[task(1,'ready'),task(2,'doing')],delivery:{preset:'auto-merge',revision:0}};
const state={platform:'windows',font:16,theme:'dark',host:'fixture',now:1000,version:'9.9.9',size_anchor:'auto',attached_edge:'top',top:false,pinned:false,service:{state:'running'},projects:[long],ai:{draft:''},notify:{projects:{}},stop_modes:false,display:require('./fixtures/display-model.json').display,messages:{complete:true,count:3}};
// icon::css, the palette the native menu injects
const palette={dark:'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--review:#BCA3EB',light:'--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--review:#8250DF'};
const html=fs.readFileSync('build/web/tray.html','utf8'),settingsHtml=html.replace('<head>','<head><script>window.taskqSurface=\'settings\';</script>');
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{for(const theme of ['dark','light']){
  const open=async(width,s={...state,theme},source=html)=>{const p=await browser.newPage({viewport:{width,height:640}});p.on('pageerror',e=>{throw e;});await p.setContent(source);await p.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});await p.evaluate(s=>taskq.receive(s),s);return p;};
  // right edge of each first-row control: an icon that moves between pages fails here
  const row=p=>p.evaluate(()=>[...document.querySelectorAll('.head .nav-home,.head .nav-actions>*,.head .window-controls>*')].map(e=>{const r=e.getBoundingClientRect();return [e.getAttribute('aria-label'),Math.round(r.top),Math.round(innerWidth-r.right)];}));
  // the Conversations count stays readable: an opaque fill with WCAG AA contrast to its digits, injected palette or not
  const badge=p=>p.locator('.head .badge').evaluate(e=>{const rgb=c=>{const x=document.createElement('canvas').getContext('2d');x.fillStyle=c;x.fillRect(0,0,1,1);return [...x.getImageData(0,0,1,1).data];};const cs=getComputedStyle(e),lum=c=>c.slice(0,3).map(v=>(v/=255)<=.03928?v/12.92:((v+.055)/1.055)**2.4).reduce((s,v,i)=>s+v*[.2126,.7152,.0722][i],0),[a,b]=[lum(rgb(cs.backgroundColor)),lum(rgb(cs.color))];return {alpha:rgb(cs.backgroundColor)[3],ratio:(Math.max(a,b)+.05)/(Math.min(a,b)+.05)};});
  const readable=async(p,why)=>{const b=await badge(p);assert.equal(b.alpha,255,'badge fill is opaque '+why);assert(b.ratio>=4.5,'badge contrast '+b.ratio.toFixed(2)+' '+why);};
  const words=p=>p.evaluate(()=>{const t=document.querySelector('.head h1'),cs=getComputedStyle(t);return {wrap:cs.overflowWrap,brk:cs.wordBreak,hyphens:cs.hyphens,cut:t.scrollWidth>t.clientWidth+1};});
  // #1681 R07: layout can commit after receive; native width changes without a new route must remeasure.
  const titlePage=await open(800,{...state,theme,font:18,stop_modes:true});
  await titlePage.evaluate(()=>document.querySelector('.menu').style.width='280px');
  await titlePage.waitForFunction(()=>document.querySelector('.head').hasAttribute('data-title-below'),{},{timeout:3000});
  assert.equal((await words(titlePage)).cut,false,'Tasks never reads Tas… at 18px');
  const controlsBefore=await row(titlePage);
  await titlePage.evaluate(()=>document.querySelector('.menu').style.width='800px');
  await titlePage.waitForFunction(()=>!document.querySelector('.head').hasAttribute('data-title-below'),{},{timeout:3000});
  assert.equal((await words(titlePage)).cut,false);
  await titlePage.evaluate(()=>document.querySelector('.menu').style.width='280px');
  await titlePage.waitForFunction(()=>document.querySelector('.head').hasAttribute('data-title-below'),{},{timeout:3000});
  assert.deepEqual(await row(titlePage),controlsBefore,'the title moves, the actions do not');
  await titlePage.close();
  for(const width of [320,360,800]){
   const p=await open(width);const home=await row(p);
   assert.deepEqual(home.map(r=>r[0]),['Home','Notifications · 0 loaded unread · history incomplete','Conversations · 3 new','Settings','Keep menu open','Always on top','Close menu'],'one Conversations entry, distinct window names, Close last');
   assert.equal(new Set(home.map(r=>r[1])).size,1,'frequent actions share the first row at '+width);
   assert.equal(await p.locator('.head [data-a=workspace]').count(),1);assert.equal(await p.locator('.head .badge').textContent(),'3');
   await readable(p,'without a palette at '+width+' '+theme);
   if(width===360){const q=await open(width,{...state,theme,palette:palette[theme]});await readable(q,'with the injected palette '+theme);await q.close();}
   assert.equal(await p.locator('.menu [aria-label*=Enlarge]').count(),0,'menu has no Enlarge');
   assert.deepEqual(await p.locator('.status-footer>*').evaluateAll(es=>es.map(e=>e.textContent||e.getAttribute('aria-label'))),['TaskQ v9.9.9','Resize window with arrow keys'],'footer is the version and the grip');
   // #1691 A2: the icon opened a separate Inbox page when events were new. #1699 A2 risk kept: one route at every count, the unread thread selected.
   await p.locator('.head [data-a=workspace]').click();assert.deepEqual(await p.evaluate(()=>(({page,thread})=>({page,thread}))(messages.filter(m=>m.a==='workspace').at(-1).open)),{page:'conversations',thread:'unread'},'new events open Conversations at the unread thread');
   assert.equal(await p.evaluate(()=>messages.filter(m=>m.a!=='workspace'&&/message|read|ack/.test(m.a)).length),0,'opening acknowledges nothing');
   // a project page with a long name: Back and the title take the row below, whole words only
   await p.evaluate(s=>taskq.receive({...s,page:'/fixture/long'}),{...state,theme});
   if(await p.locator('.head h1').textContent()===long.name){
    const project=await row(p),firstRow=project.filter(r=>r[0]!=='Open Infrastructure provisioning and observability pipeline Board');
    assert.deepEqual(firstRow.map(r=>[r[0],r[2]]),home.map(r=>[r[0],r[2]]),'icons keep their place across pages at '+width);
    assert(await p.locator('.head.title-below').count(),'Back moves the title to row 2');
    const w=await words(p);assert.deepEqual([w.wrap,w.brk,w.hyphens],['normal','normal','manual'],'no mid-word title break');
    assert((await p.locator('.head h1').boundingBox()).y>home[0][1],'title below the action row');
    if(out&&width!==800)await p.screenshot({path:out+'/menu-project-'+width+'-'+theme+'.png'});
   }else assert.fail('project page unreachable from the home dashboard');
   // Home: one activation back to the dashboard; Back then returns one page
   await p.locator('.head [data-a=home]').click();assert.equal(await p.locator('.head h1').textContent(),'Tasks');
   await p.locator('.head [data-a=back]').click();assert.equal(await p.locator('.head h1').textContent(),long.name,'Back steps one page');
   if(out&&width===320)await p.locator('.head [data-a=home]').click().then(()=>p.screenshot({path:out+'/menu-home-320-'+theme+'.png'}));
   await p.close();
  }
  // no events: the entry opens the workspace on its last thread
  let p=await open(360,{...state,theme,messages:{complete:true,count:0}});
  await p.locator('.head [data-a=workspace]').click();assert.deepEqual(await p.evaluate(()=>(({page,thread})=>({page,thread}))(messages.filter(m=>m.a==='workspace').at(-1).open)),{page:'conversations',thread:''},'#1691: no events still open Conversations');
  assert.equal(await p.locator('.head .badge').count(),0);
  // a native menu sends Settings to its own window and keeps the dashboard
  await p.evaluate(()=>taskq.receive({settingsWindow:true}));await p.locator('.head [data-route=settings]').click();
  assert.deepEqual(await p.evaluate(()=>messages.filter(m=>m.a==='settings-open').map(m=>m.page)),[null]);assert.equal(await p.locator('.head h1').textContent(),'Tasks');
  await p.close();
  // the Settings surface: its own Home, Back, Always on top, Enlarge and Close; no menu geometry or navigation writes
  p=await open(360,{...state,theme},settingsHtml);
  assert.equal(await p.locator('.head h1').textContent(),'Settings');
  await p.evaluate(()=>taskq.receive({settingsWindow:{top:true,maximized:false}}));
  assert.deepEqual((await row(p)).map(r=>r[0]),['Settings home','Always on top','Enlarge window','Close settings']);
  assert.equal(await p.locator('.head [data-a=settings-top]').getAttribute('aria-pressed'),'true');
  await p.evaluate(()=>taskq.receive({settingsOpen:{page:'appearance'}}));assert.equal(await p.locator('.head h1').textContent(),'Appearance');
  assert.equal(await p.locator('.content [aria-label="Keep menu open"]').count(),1);assert.equal(await p.locator('.content [aria-label="Menu always on top"]').count(),1);
  if(out)await p.screenshot({path:out+'/settings-window-360-'+theme+'.png'});
  await p.locator('.head [data-a=home]').click();assert.equal(await p.locator('.head h1').textContent(),'Settings','Settings home is the Settings index');
  await p.locator('.head [data-a=back]').click();assert.equal(await p.locator('.head h1').textContent(),'Appearance','Back steps one page');
  for(const a of ['settings-top','settings-enlarge','hide']){await p.locator('.head [data-a='+a+']').click();assert(await p.evaluate(a=>messages.some(m=>m.a===a),a));}
  const head=await p.locator('.head h1').boundingBox();await p.mouse.move(head.x+head.width+4,head.y+4);await p.mouse.down();await p.mouse.move(head.x+head.width+30,head.y+20);await p.mouse.up();
  assert(await p.evaluate(()=>messages.some(m=>m.a==='drag')),'header drag');
  assert.equal(await p.evaluate(()=>messages.filter(m=>m.a==='size'||m.a==='set'&&m.navigation).length),0,'no menu size or navigation writes');
  await p.close();
  // the Conversations window keeps Enlarge and Close (R26)
  p=await open(360,{...state,theme},fs.readFileSync('build/web/workspace.html','utf8'));
  await p.locator('.shell-overflow>summary').click();
  const overflow=await p.locator('.overflow-actions').boundingBox();assert(overflow.x>=0&&overflow.x+overflow.width<=360,'R43: the bundled overflow action keeps its full menu inside the native-width window');
  assert.equal(await p.locator('.shell-overflow>summary svg').count(),1,'bundled vector, never a Unicode glyph');
  await p.evaluate(()=>taskq.receive({open:{page:'conversations'}}));assert.equal(await p.locator('.head h1').textContent(),'Conversations');
  assert.equal(await p.locator('.head [aria-label="Enlarge window"]').count(),1,'Conversations keeps Enlarge');
  await p.close();
 }console.log('PASS #1660 stable header row at 320/360/800, whole-word titles, Home/Back, one Conversations entry, version-only footer, Settings surface, Conversations Enlarge, readable badge in both themes');}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
