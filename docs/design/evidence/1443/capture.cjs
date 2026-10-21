// #1443 research: check every cell; emit text only. Review keeps eight selected gap images.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const out=process.argv[2]||__dirname, routes=['home','projects','project','filtered','all-tasks','attention','settings','workspace','appearance','cadence','notifications','agents','services','about','stop','recovery','work-mode','workspace-list','workspace-new'];
const states=['doing','review','ready','waiting','later','failed'];
const tasks=Array.from({length:18},(_,i)=>({n:100+i,state:states[i%states.length],title:('Preserve the complete review title, dependencies and owner decisions across every navigation and refresh. ').repeat(2).slice(0,180),revision:1,reason:'Invented failure; inspect the issue before retrying'}));
const card=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json','utf8')).cases.find(c=>c.name.startsWith('stale')).rows[0];
tasks.push(card);
const project={root:'/fixture/alpha',name:'Alpha',host:'fixture-host',issues:'https://example.org/alpha/issues',board_kind:'github',alive:true,complete:true,age:1,version:'fixture',focus:['all'],labels:[],tasks,closed:null,cap_saved:{error:'Fixture: capacity unavailable'}};
const base={platform:'windows',font:16,theme:'dark',host:'fixture-host',now:1000,version:'source audit',projects:[project,{...project,root:'/fixture/beta',name:'Beta',issues:'https://example.org/beta/issues',tasks:tasks.slice(0,3)}],ai:{draft:''},notify:{status:'unknown',host:{ask:'off',close:'off',failed:'off',gone:'off'},projects:{}},stop_modes:true,period:'1d',history:{start:0,end:1000,since:null,tz:'UTC',bins:Array.from({length:10},()=>[0,0,0,0])}};
const conversation={root:'/fixture/alpha',project:'Alpha',id:'1',first:'1',draft:'Preserve this unsent draft',chain:[{id:'1',text:'Review the selected changes and preserve active work.',answer:{state:'result',text:'Invented reply. No action was applied.\n'+'Review evidence before confirming. '.repeat(60)}}]};
const workspace={conversations:[conversation],selected:{root:conversation.root,id:'1'},projects:[{root:conversation.root,name:'Alpha'}],history:['History fills only'],draft:''};
async function load(page,route,font,theme){
 const isWorkspace=route.startsWith('workspace');
 await page.setContent(fs.readFileSync('build/web/'+(isWorkspace?'workspace':'tray')+'.html','utf8'));
 await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};});
 if(isWorkspace){
  await page.evaluate(s=>taskq.receive(s),{font,theme,select:true,data:workspace});
  if(route!=='workspace')await page.locator('header [data-a=list]').click();
  if(route==='workspace-new')await page.locator('[data-a=new]').click();
 }
 else {
  const snapshot={...base,font,theme,page:['project','filtered'].includes(route)?project.root:['all-tasks','work-mode'].includes(route)?'home':route};
  if(route==='work-mode'){
   const resources=JSON.parse(fs.readFileSync('tests/fixtures/resource-controls.json','utf8')).cases.find(c=>c.name.startsWith('host commands')).groups[0];
   snapshot.projects=base.projects.map(p=>({...p,resources}));
  }
  await page.evaluate(s=>taskq.receive(s),snapshot);
  if(route==='work-mode')await page.locator('[data-a=work]').first().click();
  if(['filtered','all-tasks'].includes(route))await page.locator('.tqp-counts [data-state=doing]').first().click();
  if(route==='all-tasks'){
   await page.locator('.filter-header [data-a=all-tasks]').click();
  }
  await page.locator('.content').evaluate(e=>e.scrollTop=0);
 }
 await page.evaluate(async()=>{await document.fonts.ready;await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);});
}
async function measure(page){return page.evaluate(()=>{
 const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};};
 const shown=e=>{if(!e.checkVisibility())return false;for(let p=e.parentElement;p;p=p.parentElement)if(p.tagName==='DETAILS'&&!p.open&&!p.querySelector('summary')?.contains(e))return false;return true;};
 const controls=[...document.querySelectorAll('button,a,summary,input,textarea,select,[role=button]')].filter(shown).map(e=>({label:e.getAttribute('aria-label')||e.getAttribute('title')||e.labels?.[0]?.innerText||e.innerText||e.id,tag:e.tagName,disabled:!!e.disabled,tooltip:e.title,rect:rect(e.matches('input[type=checkbox]')&&e.labels?.[0]?e.labels[0]:e)}));
 const style=s=>{const e=document.querySelector(s);if(!e)return null;const c=getComputedStyle(e);return {font:c.fontSize,weight:c.fontWeight,padding:c.padding,gap:c.gap,margin:c.margin,color:c.color,background:c.backgroundColor,rect:rect(e)};};
 return {coarse:matchMedia('(pointer:coarse)').matches,heading:document.querySelector('h1')?.textContent,headerTitle:document.querySelector('.head h1')?.textContent||null,
  controls,scrolls:[...document.querySelectorAll('.content,.transcript,.composer,aside')].filter(shown).map(e=>({name:e.className||e.tagName,rect:rect(e),scrollHeight:e.scrollHeight,clientHeight:e.clientHeight,scrollWidth:e.scrollWidth,clientWidth:e.clientWidth})),
  styles:Object.fromEntries(['.head','h1','.content','.group','.row','.meta','.tqp-counts button','.grip','header','.composer'].map(s=>[s,style(s)])),
  back:document.querySelector('[data-a=back]')?rect(document.querySelector('[data-a=back]')):null,bodyOverflow:document.documentElement.scrollWidth-innerWidth,
  statusRegions:document.querySelectorAll('[role=status],[aria-live]').length,location:location.href,historyLength:history.length};
 });}
function assess(cell){
 const gaps=[],minimum=cell.pointer==='coarse'?44:32;
 if(!cell.route.startsWith('workspace')&&!cell.headerTitle)gaps.push('V03: no route title in fixed header');
 const small=cell.controls.filter(c=>!c.disabled&&(c.rect.width<minimum-.01||c.rect.height<minimum-.01));
 if(small.length)gaps.push('V09: '+small.length+' targets below '+minimum+'x'+minimum+' CSS px; '+small[0].label+' '+small[0].rect.width.toFixed(2)+'x'+small[0].rect.height.toFixed(2));
 const title=cell.styles.h1;
 if(title&&(Math.abs(parseFloat(title.font)-1.1*cell.font)>.1||title.weight!=='600'))gaps.push('V22: title '+title.font+'/'+title.weight+'; target '+(1.1*cell.font).toFixed(1)+'px/600');
 if(cell.bodyOverflow>1||cell.scrolls.some(s=>s.scrollWidth>s.clientWidth+1))gaps.push('V23: horizontal overflow');
 if(cell.back&&cell.backAfterScroll&&Math.abs(cell.back.y-cell.backAfterScroll.y)>1)gaps.push('V02: Back moves '+Math.abs(cell.back.y-cell.backAfterScroll.y).toFixed(2)+' CSS px when scrolled');
 return {name:cell.name,route:cell.route,theme:cell.theme,font:cell.font,width:cell.width,pointer:cell.pointer,verdict:gaps.length?'gap':'pass',gaps};
}
function table(cells){
 const text=s=>String(s).replaceAll('|','/').replaceAll('\n',' ');
 return '# Script pass/gap table\n\nOnly measured header, target, type, overflow and scroll checks; not full v2 or native acceptance. No screenshots per cell.\n\n| Route | Theme | Text px | Width px | Pointer | Verdict | Measured gaps |\n|---|---|---|---|---|---|---|\n'+cells.map(c=>'| '+[c.route,c.theme,c.font,c.width,c.pointer,c.verdict,c.gaps.join('; ')||'none in measured checks'].map(text).join(' | ')+' |').join('\n')+'\n';
}
async function main(){
 const good={name:'selfcheck',route:'workspace',theme:'dark',font:16,width:320,pointer:'fine',controls:[],styles:{h1:{font:'17.6px',weight:'600'}},scrolls:[],bodyOverflow:0};
 assert.equal(assess(good).verdict,'pass');
 assert(assess({...good,controls:[{label:'small',rect:{width:31,height:32}}]}).gaps.some(g=>g.startsWith('V09:')));
 assert(assess({...good,route:'project',back:{y:45},backAfterScroll:{y:-10}}).gaps.some(g=>g.startsWith('V02:')));
 fs.mkdirSync(out,{recursive:true});
 const browser=await chromium.launch(require('../../../../tests/browser.cjs')());
 const result={source:require('node:child_process').execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),renderer:await browser.version(),fixture:'invented, simulated Windows branches; no bridge; 600 CSS px viewport is not native content-fit evidence',cells:[],probes:{}};
 try{
  for(const pointer of ['fine','coarse'])for(const theme of ['dark','light'])for(const font of [16,20])for(const width of [320,360]){
   const context=await browser.newContext({viewport:{width,height:600},colorScheme:theme,hasTouch:pointer==='coarse',deviceScaleFactor:1});
   for(const route of routes){
    const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
    await load(page,route,font,theme);
    const name=`${route}-${theme}-${font}-${width}-${pointer}`;
    const metrics=await measure(page);assert.equal(metrics.coarse,pointer==='coarse');assert.deepEqual(errors,[]);
    const cell={name,route,theme,font,width,pointer,...metrics};
    if(metrics.back){await page.locator('.content').evaluate(e=>e.scrollTop=e.scrollHeight);cell.backAfterScroll=(await measure(page)).back;await page.locator('.content').evaluate(e=>e.scrollTop=0);}
    result.cells.push(assess(cell));
    if(route==='project'&&pointer==='fine'&&theme==='dark'&&font===20&&width===320){
     const before=metrics.back;await page.locator('.content').evaluate(e=>e.scrollTop=e.scrollHeight);
     result.probes.backScroll={before,after:(await measure(page)).back};
    }
    await page.close();
   }await context.close();
  }
  const page=await browser.newPage({viewport:{width:320,height:600}});
  await load(page,'home',16,'dark');
  const invoker=page.locator('[data-route=projects]').first();await invoker.focus();await invoker.click();await page.locator('[data-a=back]').click();
  result.probes.returnFocus=await page.evaluate(()=>({tag:document.activeElement.tagName,text:document.activeElement.textContent,route:document.activeElement.dataset.route||null}));
  await page.locator('[data-route=settings]').first().click();result.probes.navigationURL=page.url();
  await load(page,'home',16,'dark');result.probes.recreatedViewHeading=await page.locator('h1').innerText();
  await page.evaluate(s=>taskq.receive({...s,page:'home',projects:s.projects.map(p=>({...p,tasks:[]}))}),base);
  result.probes.emptyAttention={visible:await page.locator('.content').innerText()};
  await load(page,'workspace',16,'dark');await page.locator('[data-a=refresh]').focus();await page.evaluate(()=>taskq.receive({data:{...{conversations:[],projects:[],history:[]},error:'Invented service unavailable'}}));
  result.probes.workspaceRefresh={active:await page.evaluate(()=>document.activeElement.tagName),liveRegions:await page.locator('[role=status],[aria-live]').count()};

  await load(page,'projects',20,'dark');
  result.probes.counterGrowth=[];
  for(const n of [0,1,12,100,1000]){
   await page.evaluate(n=>{document.querySelector('.content-body').innerHTML='<div class="tqp">'+TaskqProjectBlocks.counters(Object.fromEntries(TaskqProjectBlocks.states.map(([s])=>[s,n])))+'</div>';},n);
   result.probes.counterGrowth.push({n,...await page.evaluate(()=>{const strip=document.querySelector('.tqp-counts');return {buttons:document.querySelectorAll('.tqp-counts button').length,width:strip?.clientWidth||0,scrollWidth:strip?.scrollWidth||0,states:[...document.querySelectorAll('.tqp-counts button')].map(e=>e.dataset.state)};})});

  }
  await load(page,'filtered',20,'dark');await page.evaluate(()=>document.querySelector('.menu').style.fontSize='40px');
  result.probes.resize200=await measure(page);
  await page.close();assert.equal(result.cells.length,routes.length*16);
  fs.writeFileSync(out+'/observations.json',JSON.stringify(result,null,2)+'\n');
  fs.writeFileSync(out+'/matrix.md',table(result.cells));
  console.log('Checked '+result.cells.length+' cells; '+result.cells.filter(c=>c.verdict==='pass').length+' pass, '+result.cells.filter(c=>c.verdict==='gap').length+' gap; zero page errors; no PNGs written.');
 }finally{await browser.close();}
}
module.exports={assess,table};
if(require.main===module)main().catch(e=>{console.error(e);process.exitCode=1;});
