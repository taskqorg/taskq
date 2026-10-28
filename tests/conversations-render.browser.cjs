// F5: deterministic actual-bundle fixtures. Native/DPI/screen-reader qualification is separate.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),crypto=require('node:crypto'),{chromium}=require('playwright');
const fixture=JSON.parse(fs.readFileSync('tests/fixtures/conversations.json','utf8'));
const out=process.env.TASKQ_EVIDENCE_1580||'.taskq/conversations-render';
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')()),html=fs.readFileSync('build/web/workspace.html');fs.mkdirSync(out,{recursive:true});
 const cells=[];
 try{
  for(const sample of fixture.cases){
   const page=await browser.newPage({viewport:{width:sample.width,height:sample.height}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
   const items=structuredClone(fixture.items),c=items[0],s={font:sample.font,theme:sample.theme,host:'fixture-host',version:'fixture',projects:[structuredClone(fixture.project)],ai:{history:['Review the last result','Plan held tasks'],runtime:{id:'claude',name:'Claude Code',available:true}},messages:{complete:true,entries:[]},conversations:{complete:true,items,navigation:{list:'recent',search:'',selected:c.id,selection:{recent:c.id},listOnly:false}}};
   if(['launched','interrupted'].includes(sample.state)){
    c.sent={text:'Review these files. Ask before applying external effects.',root:c.root,runtime:'claude',files:structuredClone(c.files)};
    c.text=sample.state==='launched'?'A separate next draft remains editable.':'Draft preserved while the original delivery is unknown.';c.files=[];
    c.request={id:c.id,state:sample.state,kind:sample.state,next:sample.state==='launched'?'Open the session to read and answer.':'Check the original native session; do not resend.',...(sample.state==='launched'?{url:'https://claude.ai/code/session_fixture1580'}:{reason:'Native launch receipt interrupted. Applied effects and runtime completion are unknown.'})};
   }
   if(sample.state==='deleting'){c.archived=true;c.deleting=true;s.conversations.navigation={...s.conversations.navigation,list:'archive',selection:{archive:c.id}};}
   if(sample.state==='unavailable'){s.projects[0].alive=false;c.title='Long request with unavailable service';c.files[0].name='a-long-owner-selected-file-name-that-must-wrap-without-hiding-controls.md';}
   if(sample.state==='empty'){s.conversations.items=[];s.conversations.navigation.selected=null;}
   await page.setContent(html.toString());await page.evaluate(s=>{window.ipc={postMessage:()=>{}};taskq.receive(s);taskq.receive({open:{page:'conversations'}});},s);
   // #1741 Q-A2: Send tools stay outside the one reading viewport, without scrolling to reveal them.
   const geometry=await page.evaluate(()=>({client:innerWidth,scroll:document.documentElement.scrollWidth,heading:document.querySelector('h1').getBoundingClientRect().top,composer:document.querySelector('.conversation-composer')?.getBoundingClientRect().toJSON(),send:document.querySelector('[data-c=send],[data-c=from-draft]')?.getBoundingClientRect().toJSON()}));
   assert(geometry.scroll<=geometry.client,'F3 horizontal overflow: '+sample.name);assert(geometry.heading>=0,'fixed bar reachable: '+sample.name);
   if(geometry.composer)assert(geometry.composer.bottom<=sample.height&&geometry.composer.top>=0,'composer reachable through detail scroll: '+sample.name);
   if(geometry.send)assert(geometry.send.top>=0&&geometry.send.bottom<=sample.height,'Send fully visible without scrolling: '+sample.name);
   assert.deepEqual(errors,[]);
   const file=sample.name+'.png';await page.screenshot({path:path.join(out,file)});
   cells.push({...sample,file,sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(out,file))).digest('hex'),geometry,result:'Pass: browser geometry only'});
   await page.close();
  }
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({bundle_sha256:crypto.createHash('sha256').update(html).digest('hex'),fixture_sha256:crypto.createHash('sha256').update(fs.readFileSync('tests/fixtures/conversations.json')).digest('hex'),reference_sha256:'9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04',environment:{renderer:'Playwright Chromium headless',version:browser.version(),os:process.platform,arch:process.arch,scale:1,font:'system UI',native:'Not tested'},cells},null,2)+'\n');
  console.log('PASS F5 six deterministic browser fixtures: normal/files, native launch, interrupted, incomplete deletion, unavailable narrow and empty. Images require visual review.');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
