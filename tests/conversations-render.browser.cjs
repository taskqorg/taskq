// F5: deterministic actual-bundle fixtures. Native/DPI/screen-reader qualification is separate.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),crypto=require('node:crypto'),{chromium}=require('playwright');
const fixture=JSON.parse(fs.readFileSync('tests/fixtures/conversations.json','utf8'));
const out=process.env.TASKQ_EVIDENCE_1580||'.taskq/conversations-render';
// #1742 Q-A3..A5/A9: long history in the actual bundle. An in-page model of the D4 reader (transcript.rs: 256 KiB pages,
// at most 200 items, whole records, ignored records skipped) answers reads out of order with seeded delays.
// Defect class: a jump after prepend, append, font or width change; a duplicate or omitted record at a page seam;
// unbounded mounted nodes; a stale reply applied after Jump to latest; a page of ignored records that stops the reader.
async function historyCase(browser,html,runtime){
 const page=await browser.newPage({viewport:{width:900,height:720}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 const launched=(id,rt)=>({id,revision:1,title:rt+' history',text:'',root:'fixture-alpha',caret:[0,0],scroll:0,files:[],archived:false,
  sent:{text:'Start',root:'fixture-alpha',runtime:rt,files:[]},request:{id,state:'launched',kind:'launched',url:'https://example.org/'+rt+'/'+id,next:'Reply here'},dialogue:{state:'idle',watch:'no watch',turn:null}});
 const id=runtime+'-history',other=runtime+'-other';
 await page.setContent(html);
 await page.evaluate(({runtime,id,other})=>{
  // seeded so a failure replays; record heights vary from one line to a long wrapped message
  let seed=1742;const rnd=()=>(seed=(seed*1103515245+12345)%2147483648)/2147483648;
  const records=[];let at=0;
  const push=(len,items)=>{records.push({at,len,items});at+=len;};
  const words='alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'.split(' ');
  const say=n=>Array.from({length:1+Math.floor(rnd()*(n%97===50?400:30))},()=>words[Math.floor(rnd()*words.length)]).join(' ');
  for(let n=0;n<10000;n++){
   // ignored-only runs longer than one 256 KiB page: thinking, progress or reasoning records with no view item
   // one run longer than the fill budget, so reading on needs Load earlier
   if(n%2500===1250)for(let k=0;k<(n===6250?12:3);k++)push(200*1024,[]);
   const text=(n%2?'Answer ':'Question ')+n+' '+say(n);
   if(runtime==='claude'&&n%2)push(text.length+400,[{role:'assistant',text},{role:'tool',text:'Bash: taskq status --line '+n}]);
   else push(text.length+(runtime==='codex'?260:180),[{role:n%2?'assistant':'user',text}]);
  }
  const CHUNK=256*1024,ITEMS=200,view=(r,list)=>list.flatMap(r=>r.items.map((item,i)=>({...item,at:r.at,i})));
  const index=cursor=>{let lo=0,hi=records.length;while(lo<hi){const m=(lo+hi)>>1;if(records[m].at<cursor)lo=m+1;else hi=m;}return lo;};
  const size=()=>records.length?records.at(-1).at+records.at(-1).len:0;
  function before(cursor){
   let hi=index(cursor),lo=hi,bytes=0,count=0;
   while(lo>0&&(lo===hi||bytes+records[lo-1].len<=CHUNK)&&(lo===hi||count+records[lo-1].items.length<=ITEMS)){lo--;bytes+=records[lo].len;count+=records[lo].items.length;}
   const list=records.slice(lo,hi);return {items:view(null,list),start:list[0]?.at??cursor,end:cursor,size:size()};
  }
  function after(cursor){
   const lo=index(cursor);let hi=lo,bytes=0,count=0;
   while(hi<records.length&&(hi===lo||bytes+records[hi].len<=CHUNK)&&(hi===lo||count+records[hi].items.length<=ITEMS)){bytes+=records[hi].len;count+=records[hi].items.length;hi++;}
   const list=records.slice(lo,hi);return {items:view(null,list),start:cursor,end:hi<records.length?records[hi].at:size(),size:size()};
  }
  window.history1742={records,all:view(null,records).map(item=>item.at+':'+item.i),reads:[],renders:[],ignored:0,frames:[],held:null,hold:false,pending:0,longtasks:[],
   append(n){for(let k=0;k<n;k++){const text='Fresh '+k+' '+say(k);push(text.length+180,[{role:k%2?'assistant':'user',text}]);}this.all=view(null,records).map(item=>item.at+':'+item.i);}};
  const h=window.history1742;
  new PerformanceObserver(list=>{for(const e of list.getEntries())h.longtasks.push(e.duration);}).observe({entryTypes:['longtask']});
  window.ipc={postMessage:text=>{
   const m=JSON.parse(text);if(m.a!=='conversation-dialogue')return;
   const value=m.action==='status'?{state:'idle',watch:'no watch',turn:null,last_line:''}:m.after!==undefined?after(m.after):before(m.before??size());
   if(m.action==='transcript'){h.reads.push({before:m.before,after:m.after});if(!value.items.length&&value.start>0)h.ignored++;}
   const deliver=()=>{const t0=performance.now();taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:structuredClone(value)}});if(m.action==='transcript')h.renders.push(performance.now()-t0);h.pending--;};
   h.pending++;if(h.hold&&m.action==='transcript'&&!h.held){h.held=deliver;return;}
   setTimeout(deliver,Math.floor(rnd()*25));
  }};
 },{runtime,id,other});
 const state={font:16,theme:'dark',host:'fixture-host',version:'fixture',projects:[{root:'fixture-alpha',name:'Alpha',alive:true,complete:true,issues:'https://example.org/alpha/issues',tasks:[]}],ai:{history:[],runtime:{id:'claude',name:'Claude Code',available:true}},messages:{complete:true,entries:[]},
  conversations:{complete:true,items:[launched(id,runtime),launched(other,runtime)],navigation:{selected:id,list:'recent'}}};
 await page.evaluate(s=>{taskq.receive(s);taskq.receive({open:{page:'conversations'}});},state);
 // settled: no read in flight, none started by a late scroll event, replies arrive within 25 ms
 const settle=async()=>{for(;;){await page.waitForFunction(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(history1742.pending===0&&!document.querySelector('[data-c=earlier][disabled]'))))),null,{polling:50,timeout:20000});const r=await page.evaluate(()=>history1742.reads.length);await page.waitForTimeout(40);if(await page.evaluate(r=>history1742.reads.length===r&&history1742.pending===0,r))return;}};
 // DOM keys in order, unique and in file order; the bound holds at every sample
 const sample=()=>page.evaluate(()=>{const keys=[...document.querySelectorAll('.dialogue-transcript>[data-key]')].map(e=>e.dataset.key);return {keys,earlier:!!document.querySelector('[data-c=earlier]')};});
 const order=(keys,where)=>{const n=keys.map(k=>k.split(':').map(Number));for(let k=1;k<n.length;k++)if(!(n[k][0]>n[k-1][0]||n[k][0]===n[k-1][0]&&n[k][1]>n[k-1][1]))throw new Error(runtime+' '+where+': order or duplicate at '+keys[k-1]+' / '+keys[k]);};
 const anchor=()=>page.evaluate(()=>{const el=document.querySelector('.conversation-reader'),top=el.getBoundingClientRect().top;const node=[...el.querySelectorAll('.dialogue-transcript>[data-key]')].find(n=>n.getBoundingClientRect().bottom>top);return {key:node.dataset.key,y:node.getBoundingClientRect().top-top};});
 const offset=key=>page.evaluate(key=>{const el=document.querySelector('.conversation-reader'),node=el.querySelector('.dialogue-transcript>[data-key="'+key+'"]');return node?node.getBoundingClientRect().top-el.getBoundingClientRect().top:null;},key);
 const near=async(a,where)=>{const y=await offset(a.key);if(y===null||Math.abs(y-a.y)>2)throw new Error(runtime+' '+where+': anchor '+a.key+' moved from '+a.y+' to '+y);
  // no delayed second jump
  await page.waitForTimeout(150);const later=await offset(a.key);if(later===null||Math.abs(later-a.y)>2)throw new Error(runtime+' '+where+': delayed jump to '+later);};
 const scrollBy=dy=>page.evaluate(dy=>{const e=document.querySelector('.conversation-reader');e.scrollTop+=dy;e.dispatchEvent(new Event('scroll',{bubbles:true}));},dy);
 await settle();
 const total=await page.evaluate(()=>history1742.all.length);
 let s=await sample(),max=s.keys.length;order(s.keys,'open');
 assert(await page.evaluate(()=>{const e=document.querySelector('.conversation-reader');return e.scrollHeight-e.scrollTop-e.clientHeight<2;}),runtime+': opens at the newest record');
 assert.equal(await page.locator('.dialogue-transcript').getAttribute('aria-live'),'off',runtime+': page loads are not read out in full');

 // A1: prepend keeps the visible record within 2 px
 await scrollBy(-200);await settle();let a=await anchor();
 await page.evaluate(()=>{const e=document.querySelector('.conversation-reader');e.scrollTop=Math.min(e.scrollTop,40);e.dispatchEvent(new Event('scroll',{bubbles:true}));});
 a=await anchor();await settle();await near(a,'prepend');
 assert.equal(await page.locator('.sr-only[role=status]').first().innerText(),'Earlier messages loaded',runtime+': a prepend is announced briefly');
 // A1: font and width changes keep it too
 a=await anchor();await page.evaluate(()=>[...document.querySelectorAll('*')].find(e=>e.style?.getPropertyValue('--size'))?.style.setProperty('--size','20px'));await settle();await near(a,'font 20');
 a=await anchor();await page.setViewportSize({width:640,height:720});await settle();await near(a,'width 640');
 await page.setViewportSize({width:900,height:720});await page.evaluate(()=>[...document.querySelectorAll('*')].find(e=>e.style?.getPropertyValue('--size'))?.style.setProperty('--size','16px'));await settle();

 // A2: new output below a reader of older text does not move it; Jump to latest is the visible way back
 a=await anchor();await page.evaluate(()=>history1742.append(6));
 await page.waitForSelector('[data-c=latest]:has-text("New messages")',{timeout:8000});await settle();await near(a,'append');
 await page.screenshot({path:path.join(out,'history-new-messages-'+runtime+'.png')});
 // A1: returning to the thread reloads the range that holds the saved record
 a=await anchor();await page.locator('[data-c=select][data-id="'+other+'"]').click();await settle();
 const mark=await page.evaluate(()=>history1742.reads.length);await page.locator('[data-c=select][data-id="'+id+'"]').click();await settle();
 assert.equal(await page.evaluate(k=>history1742.reads[k]?.after,mark),Number(a.key.split(':')[0]),runtime+': return reads the range that starts at the saved record');
 await near(a,'return');

 // A3/A4: traverse to the start by keyboard-free scrolling; every record once, mounted nodes bounded
 const seen=new Set();let n=0,clicks=0;
 for(;;){
  s=await sample();order(s.keys,'backward');for(const k of s.keys)seen.add(k);max=Math.max(max,s.keys.length);
  if(!s.earlier&&await page.evaluate(()=>document.querySelector('.conversation-reader').scrollTop<1))break;
  if(++n>2000)throw new Error(runtime+': traversal did not reach the start');
  const first=s.keys[0];await page.evaluate(()=>{const e=document.querySelector('.conversation-reader');e.scrollTop=0;e.dispatchEvent(new Event('scroll',{bubbles:true}));});await settle();
  // the fill budget stopped automatic reads in a long run of skipped records; Load earlier continues
  s=await sample();if(s.keys[0]===first&&s.earlier&&await page.evaluate(()=>document.querySelector('.conversation-reader').scrollTop<1)){clicks++;await page.locator('[data-c=earlier]').click();await settle();}
 }
 s=await sample();const all=await page.evaluate(()=>history1742.all);
 assert.equal(s.keys[0],all[0],runtime+': the first record is reached');assert.equal(clicks,1,runtime+': only the long skipped run needs Load earlier');
 // every record from the first one mounted on return back to the file start was mounted on the way
 const upto=Math.max(...[...seen].map(k=>all.indexOf(k)));assert(upto>all.length-400,runtime+': traversal starts near the end');
 assert.deepEqual(all.slice(0,upto+1).filter(k=>!seen.has(k)),[],runtime+': no record omitted on the way to the start');
 assert(max<=800,runtime+': mounted items stay within WINDOW + one page: '+max);

 // A2: Jump to latest from the start replaces the window; a stale reply held across it is ignored
 await page.evaluate(()=>{history1742.hold=true;const e=document.querySelector('.conversation-reader');e.scrollTop=e.scrollHeight;e.dispatchEvent(new Event('scroll',{bubbles:true}));});
 await page.waitForFunction(()=>history1742.held||history1742.pending===0);
 // #1747: a scroll event of the old window before the newest page arrives (macOS headless delivered one) keeps following
 await page.locator('[data-c=latest]').waitFor();
 await page.evaluate(()=>{document.querySelector('[data-c=latest]').click();const e=document.querySelector('.conversation-reader');e.scrollTop=0;e.dispatchEvent(new Event('scroll',{bubbles:true}));});
 await page.evaluate(()=>{history1742.hold=false;const d=history1742.held;history1742.held=null;d?.();});await settle();
 s=await sample();order(s.keys,'latest');assert.equal(s.keys.at(-1),all.at(-1),runtime+': Jump to latest shows the newest record');
 assert(await page.evaluate(()=>{const e=document.querySelector('.conversation-reader');return e.scrollHeight-e.scrollTop-e.clientHeight<2;}),runtime+': Jump to latest reaches the end');
 assert.equal(await page.locator('[data-c=latest]').count(),0,runtime+': following hides Jump to latest');

 // A4: a selection for copy stays mounted while more history loads
 await page.evaluate(()=>{const nodes=document.querySelectorAll('.dialogue-transcript>[data-key] .conversation-text');const r=document.createRange();r.setStart(nodes[nodes.length-3].firstChild,0);r.setEnd(nodes[nodes.length-2].firstChild,5);getSelection().removeAllRanges();getSelection().addRange(r);});
 const copied=await page.evaluate(()=>getSelection().toString());
 for(let k=0;k<6;k++){await page.evaluate(()=>{const e=document.querySelector('.conversation-reader');e.scrollTop=0;e.dispatchEvent(new Event('scroll',{bubbles:true}));});await settle();max=Math.max(max,(await sample()).keys.length);}
 assert.equal(await page.evaluate(()=>getSelection().toString()),copied,runtime+': selection survives loads');
 await page.evaluate(()=>getSelection().removeAllRanges());
 // A4: keyboard reading moves the reader and loads earlier history
 // A4: keyboard reading moves the reader and reaches earlier history
 // #1747: headless Chromium on macOS sometimes drops the first PageUp (no scroll event, no view scrollTop write); a second
 // press scrolls. The visible record moving down is the check: scrollTop also grows when the scroll prepends a page.
 await page.locator('.dialogue-transcript').focus();const k0=(await sample()).keys[0],a0=await anchor();
 let moved=false;for(let k=0;k<2&&!moved;k++){await page.keyboard.press('PageUp');await settle();const y=await offset(a0.key);moved=y===null||y>a0.y+100;}
 assert(moved,runtime+': PageUp scrolls the transcript');
 for(let k=0;k<80&&(await sample()).keys[0]===k0;k++){await page.keyboard.press('PageUp');await settle();max=Math.max(max,(await sample()).keys.length);}
 assert.notEqual((await sample()).keys[0],k0,runtime+': PageUp reads earlier history');
 assert(max<=800,runtime+': mounted items stay bounded while a selection is held: '+max);
 assert(await page.evaluate(()=>history1742.ignored>0),runtime+': pages of only ignored records still reach earlier history');
 // Q-A6: receive-and-render time per page and frame intervals during continuous wheel scrolling, measured in headless Chromium, not asserted
 await page.mouse.move(450,300);await page.evaluate(()=>{const h=history1742;let last=performance.now();h.frames=[];h.run=true;const tick=t=>{h.frames.push(t-last);last=t;if(h.run)requestAnimationFrame(tick);};requestAnimationFrame(tick);});
 for(let k=0;k<60;k++){await page.mouse.wheel(0,-240);await page.waitForTimeout(16);}
 await settle();const frames=(await page.evaluate(()=>{history1742.run=false;return history1742.frames;})).slice(1).sort((x,y)=>x-y);
 assert.deepEqual(errors,[]);
 const long=await page.evaluate(()=>history1742.longtasks),renders=(await page.evaluate(()=>history1742.renders)).sort((x,y)=>x-y);
 await page.close();
 return {runtime,records:10000,items:total,reads:renders.length,mounted_max:max,load_render_ms:{median:Math.round(renders[renders.length>>1]),p95:Math.round(renders[Math.floor(renders.length*.95)]),max:Math.round(renders.at(-1))},longtasks:{count:long.length,max_ms:Math.round(Math.max(0,...long))},wheel_frame_ms:{frames:frames.length,median:Math.round(frames[frames.length>>1]),p95:Math.round(frames[Math.floor(frames.length*.95)]),max:Math.round(frames.at(-1))},result:'Pass: browser model; timings measured, not asserted'};
}
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
  const history=[];for(const runtime of ['claude','codex'])history.push(await historyCase(browser,html.toString(),runtime));
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({bundle_sha256:crypto.createHash('sha256').update(html).digest('hex'),fixture_sha256:crypto.createHash('sha256').update(fs.readFileSync('tests/fixtures/conversations.json')).digest('hex'),reference_sha256:'9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04',environment:{renderer:'Playwright Chromium headless',version:browser.version(),os:process.platform,arch:process.arch,scale:1,font:'system UI',native:'Not tested'},cells,history},null,2)+'\n');
  console.log('PASS F5 six deterministic browser fixtures: normal/files, native launch, interrupted, incomplete deletion, unavailable narrow and empty. Images require visual review.');
  console.log('PASS #1742 long history, Claude and Codex items: 10,000 records to the start and back to latest, anchors within 2 px, out-of-order and stale replies, bounded mount. Measured:');for(const r of history)console.log(JSON.stringify(r));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
