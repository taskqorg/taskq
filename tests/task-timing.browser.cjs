// #2366 numbers-only timing uses the released Home and Conversations rows; invented board data.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const now=1700000000,display=require('./fixtures/display-model.json').display;
 const task={n:7,revision:71,state:'doing',state_name:'Doing',rank:0,title:'A wrapped task title keeps its full width and one Conversations target',runtime:'codex',local_session:true,timing:{started:true,first:now-138*60,changes:7,current:now-7*60}};
 const project={root:'fixture',name:'Invented',host:'fixture',issues:'https://example.org/issues',board_kind:'github',alive:true,complete:true,age:1,focus:['all'],labels:[],cap_saved:{error:'Invented fixture'},tasks:[task]};
 const out=process.env.TASKQ_EVIDENCE_2366;if(out)fs.mkdirSync(out,{recursive:true});
 for(const surface of ['tray','workspace'])for(const font of [14,16,18,20])for(const width of surface==='tray'?[350,620]:[350,1100]){
  const page=await browser.newPage({viewport:{width,height:800}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fs.readFileSync('build/web/'+surface+'.html','utf8'));
  await page.evaluate(({now,display,project,font,surface})=>{Date.now=()=>now*1000;window.messages=[];window.ipc={postMessage:m=>messages.push(JSON.parse(m))};taskq.receive({page:surface==='tray'?'home':undefined,platform:'windows',theme:'dark',font,now,host:'fixture',display,projects:[project],ai:{draft:''},notify:{projects:{}},messages:{complete:true,entries:[]},conversations:{complete:true,items:[],navigation:{threads:{selected:'task:https://example.org/issues/7'}}}});},{now,display,project,font,surface});
  if(surface==='tray'){await page.locator('.tqp-counts [data-state=doing]').first().click();}
  else if(width<700)await page.getByRole('button',{name:'List',exact:true}).click();
  const row=page.locator('.task-target[data-task="7"]').first();await row.waitFor();assert(await row.isVisible());
  assert.deepEqual((await row.locator('.task-timing>span').allTextContents()).map(t=>t.trim()),['2h18m','7','7m']);
  assert.match(await row.getAttribute('aria-description'),/first 2023-11-14T19:55:20.000Z/);await row.focus();assert.equal(await row.evaluate(e=>e===document.activeElement),true);
  assert.equal(await row.locator('.task-timing [tabindex],.task-timing button,.task-timing a').count(),0);
  const geometry=await row.evaluate(e=>{const r=e.getBoundingClientRect(),title=e.querySelector('.tqp-task-title'),head=e.querySelector('.tqp-task-head');return {overflow:document.documentElement.scrollWidth>innerWidth,clipped:[...e.querySelectorAll('.task-timing>span')].some(n=>{const b=n.getBoundingClientRect();return b.left<r.left-1||b.right>r.right+1;}),title:title.textContent,full:title.getBoundingClientRect().width>=r.width-10,headHeight:head.getBoundingClientRect().height};});
  assert.equal(geometry.overflow,false);assert.equal(geometry.clipped,false);assert(geometry.full);assert.equal(geometry.title,task.title);
  assert(await row.locator('[data-icon="rotate-cw"]').count());assert(await row.locator('[data-icon="openai"]').count());assert(await row.locator('[data-icon="github"]').count());
  if(surface==='tray'&&width===620&&font===16){const height=await row.locator('.tqp-task-head').evaluate(e=>Math.max(...[...e.children].map(n=>n.getBoundingClientRect().height)));assert(geometry.headHeight<=height+1,'normal row metadata remains one line: '+JSON.stringify({geometry,height}));}
  if(out&&font===16){await page.evaluate(()=>document.activeElement.blur());await page.mouse.move(0,0);await page.waitForTimeout(300);await page.screenshot({path:out+'/'+surface+'-timing-'+width+'.png'});}
  if(surface==='workspace'&&width===1100){
   await page.getByRole('separator',{name:'Sidebar width'}).focus();await page.keyboard.press('Home');
   await page.evaluate(project=>{project.tasks[0].timing={};taskq.receive({projects:[project]});},structuredClone(project));
   await page.waitForFunction(()=>document.querySelector('.task-timing')?.textContent.includes('Unknown'));
   const clipped=await row.evaluate(e=>{const viewport=e.closest('.conversation-rows').getBoundingClientRect();return [...e.querySelectorAll('.task-timing>span')].some(n=>{const range=document.createRange();range.selectNodeContents(n);return [...range.getClientRects()].some(r=>r.left<viewport.left-1||r.right>viewport.right+1);});});
   assert.equal(clipped,false,'Unknown timing must wrap inside the minimum sidebar, not clip at its divider');
  }
  assert.deepEqual(errors,[]);await page.close();
 }
 console.log('PASS numbers-only timing in Home and Conversations, 14/16/18/20 px, normal/narrow widths, icons, focus and full-width title');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
