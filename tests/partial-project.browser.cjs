// #1840 A1/A2: one failed board must not hide loaded counts in the shared menu/dashboard.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const fx=require('./fixtures/display-model.json');
const good={...structuredClone(fx.projects[0]),name:'Loaded',root:'/fixture/loaded',alive:true,age:1};
const bad={...structuredClone(good),name:'Broken',root:'/fixture/broken',issues:'https://example.org/broken/issues',complete:false,partial:['Broken'],error:'no access to the repository',tasks:[]};
const state={platform:'windows',host:'fixture',font:16,theme:'dark',now:fx.now,projects:[good,bad],display:{...fx.display,all:{...fx.display.all,partial:['Broken']}},ai:{draft:''},notify:{projects:{}}};
(async()=>{const browser=await chromium.launch(require('./browser.cjs')());try{
 const page=await browser.newPage({viewport:{width:420,height:850}});page.on('pageerror',e=>{throw e;});
 await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));await page.evaluate(()=>{window.ipc={postMessage(){}};});await page.evaluate(s=>taskq.receive(s),state);
 const strip=page.locator('.filter-header');
 if(process.env.PROJECT_ERROR_EVIDENCE){fs.mkdirSync(process.env.PROJECT_ERROR_EVIDENCE,{recursive:true});await page.screenshot({path:process.env.PROJECT_ERROR_EVIDENCE+'/mixed-projects.png'});}
 assert.equal(await strip.locator('[data-a=all-tasks]').innerText(),'All tasks '+good.tasks.filter(t=>t.state!=='closed').length);
 assert.equal(await page.getByText('Broken: no access to the repository',{exact:true}).count(),1);
 assert(!((await page.locator('body').innerText()).includes('Partial: refresh the affected project or open its Board.')));
 assert.match(await strip.locator('[data-state=doing]').getAttribute('aria-label'),/Doing: 1/);
 // Project/search scope stays a presentation filter; missing-only data remains unknown rather than a healthy zero.
 await page.locator('[data-a=search-open]').click();await page.locator('[data-task-query]').fill('T8');
 assert.equal(await strip.locator('[data-a=all-tasks]').innerText(),'All tasks 1');
 await page.evaluate(s=>taskq.receive({...s,projects:[s.projects[1]]}),state);
 assert.equal(await strip.locator('[data-a=all-tasks]').innerText(),'All tasks ?');
 console.log('PASS #1840 loaded totals, named failure, scoped search and unknown-only failure');
 }finally{await browser.close();}})().catch(e=>{console.error(e);process.exitCode=1;});
