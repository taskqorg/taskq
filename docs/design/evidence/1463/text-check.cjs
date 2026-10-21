// UI acceptance instrument: invented fixtures, no native/board effects or screenshots.
const fs=require('node:fs'),{chromium}=require('playwright');
const out=process.argv[2]||'.taskq/evidence/1463';
const project={root:'fixture-alpha',name:'Alpha',issues:'https://example.org/alpha/issues',alive:true,complete:true,age:1,focus:[],labels:[],cap_saved:{error:'Invented capacity unavailable'}};
const base={platform:'windows',font:16,theme:'dark',host:'fixture',version:'fixture',now:1000,ai:{draft:''},notify:{status:'unknown',host:{ask:'off',close:'off',failed:'off',gone:'off'},projects:{}},period:'1d',history:{start:0,end:1000,since:null,tz:'UTC',bins:Array.from({length:10},()=>[0,0,0,0])}};
async function textChecks(html){
 const browser=await chromium.launch(require('../../../../tests/browser.cjs')());
 try{
  const result=[];
  for(const theme of ['dark','light'])for(const font of [14,16,18,20])for(const width of [320,360]){
   const page=await browser.newPage({viewport:{width,height:600}});
   await page.setContent(html);
   for(const route of ['home','projects',project.root,'attention','settings','appearance','cadence','notifications','agents','services','about','stop','recovery','filtered','all-tasks','work-mode','workspace','workspace-sessions','workspace-agents']){
    if(route==='workspace')await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
    const p={...project,tasks:['doing','review','ask','failed','ready','waiting','later'].map((state,i)=>({n:i+1,state,revision:1,title:'Invented visible task '+state}))};
    if(route==='work-mode'){p.resources=JSON.parse(fs.readFileSync('tests/fixtures/resource-controls.json','utf8')).cases.find(c=>c.name.startsWith('host commands')).groups[0];p.cap_saved={};}
    await page.evaluate(s=>taskq.receive(s),{...base,...(!route.startsWith('workspace')?{page:['filtered','all-tasks','work-mode'].includes(route)?project.root:route}:{}),theme,font,projects:[p]});
    if(['filtered','all-tasks'].includes(route))await page.locator('.tqp-counts [data-state=doing]').first().click();
    if(route==='all-tasks')await page.locator('.filter-header [data-a=all-tasks]').click();
    if(route==='work-mode')await page.locator('[data-a=work]').click();
    if(route==='workspace-sessions')await page.locator('.content [data-route=sessions]').click();
    if(route==='workspace-agents'){await page.locator('[data-a=back]').click();await page.locator('.content [data-route=agents]').click();}
    result.push({theme,font,width,route,...await page.evaluate(()=>{
     const canvas=document.createElement('canvas');canvas.width=canvas.height=1;const c=canvas.getContext('2d');
     const rgb=color=>{c.clearRect(0,0,1,1);c.fillStyle=color;c.fillRect(0,0,1,1);return [...c.getImageData(0,0,1,1).data].slice(0,3);};
     const luminance=a=>a.map(n=>n/255).map(n=>n<=.04045?n/12.92:((n+.055)/1.055)**2.4).reduce((n,v,i)=>n+v*[.2126,.7152,.0722][i],0);
     const contrast=(a,b)=>{const x=luminance(a),y=luminance(b);return (Math.max(x,y)+.05)/(Math.min(x,y)+.05);};
     if(Math.abs(contrast([0,0,0],[255,255,255])-21)>.0001)throw Error('Contrast instrument failed its black/white self-check');
     const gaps=[],graphics=[],w=document.createTreeWalker(document.querySelector('.shell'),NodeFilter.SHOW_TEXT);
     for(let n;n=w.nextNode();){const e=n.parentElement;if(!n.textContent.trim()||!e.checkVisibility()||e.closest('button:disabled,svg'))continue;const ancestors=[];for(let p=e;p;p=p.parentElement)ancestors.unshift(p);c.clearRect(0,0,1,1);for(const p of ancestors){c.fillStyle=getComputedStyle(p).backgroundColor;c.fillRect(0,0,1,1);}const bg=[...c.getImageData(0,0,1,1).data].slice(0,3),fg=getComputedStyle(e).color,ratio=contrast(rgb(fg),bg);if(ratio<4.5)gaps.push({text:n.textContent.trim().slice(0,50),class:e.className,fg,bg,ratio});}
     const heading=document.querySelector('h1'),style=getComputedStyle(heading),content=document.querySelector('.content');
     for(const e of document.querySelectorAll('input:not([type=checkbox]),textarea,select')){
      if(e.disabled||!e.checkVisibility())continue;
      const style=getComputedStyle(e),ancestors=[];for(let p=e;p;p=p.parentElement)ancestors.unshift(p);c.clearRect(0,0,1,1);for(const p of ancestors){c.fillStyle=getComputedStyle(p).backgroundColor;c.fillRect(0,0,1,1);}const bg=[...c.getImageData(0,0,1,1).data].slice(0,3),ratio=contrast(rgb(style.borderTopColor),bg);if(parseFloat(style.borderTopWidth)<1||ratio<3)graphics.push({id:e.id,ratio});
     }
     return {heading:heading.textContent,title_px:style.fontSize,title_weight:style.fontWeight,overflow:content.scrollWidth>content.clientWidth,contrast_gaps:gaps,graphics_gaps:graphics};
    })});
   }await page.close();
  }return result;
 }finally{await browser.close();}
}
(async()=>{fs.mkdirSync(out,{recursive:true});const cells=await textChecks(fs.readFileSync('build/web/tray.html','utf8'));fs.writeFileSync(out+'/text-checks.json',JSON.stringify(cells,null,2)+'\n');console.log('UI text cells '+cells.length+'; measured gap cells '+cells.filter(c=>c.contrast_gaps.length||c.graphics_gaps.length||c.overflow).length);})().catch(e=>{console.error(e);process.exitCode=1;});
