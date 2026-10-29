// §5/#1817: real Conversations/tray bundles -> native-copy IPC -> immutable reply. Backend is an explicit fake.
const fs=require('node:fs'),assert=require('node:assert/strict'),crypto=require('node:crypto'),{chromium}=require('playwright');
const base=require('./fixtures/task-workspace.json').state;
(async()=>{
 const browser=await chromium.launch(require('./browser.cjs')());
 try{
  for(const surface of ['workspace','tray']){
   const page=await browser.newPage({viewport:{width:900,height:850},colorScheme:'dark'}),errors=[],copies=new Map();
   page.on('pageerror',e=>errors.push(e.message));
   const board='https://github.com/invented/reply-test/issues',root='invented-project',row={n:7,title:'Send the report',state:'ask',revision:20,url:board+'/7',question:{parsed:true,done:'Checked the inputs',problem:'Please send the files',text:'Please send the files',choices:[],evidence:[]}};
   const state={...base,font:16,theme:'dark',palette:'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--review:#BCA3EB',host:'invented-host',projects:[{...base.projects[0],root,name:'Invented project',issues:board,alive:true,complete:true,tasks:[row]}],ai:{history:[]},conversations:{complete:true,items:[]},messages:{complete:true,entries:[]}};
   await page.exposeFunction('copyReplyFile',async m=>{
    if(m.file.name==='refused.bin'){await page.evaluate(r=>taskq.receive({replyFileReceipt:r}),{id:m.id,state:'rejected',kind:'storage',reason:'Disk full; free disk space and select the file again'});return;}
    const bytes=Buffer.from(m.data,'base64'),file={...m.file,hash:crypto.createHash('sha256').update(bytes).digest('hex')};
    copies.set(file.id,bytes);
    await page.evaluate(r=>taskq.receive({replyFileReceipt:r}),{id:m.id,state:'accepted',file});
   });
   // The tray's Open question button opens this same Conversations composer in its native window.
   if(surface==='tray'){
    await page.setContent(fs.readFileSync('build/web/tray.html','utf8'));
    await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>messages.push(JSON.parse(s))};});
    await page.evaluate(s=>taskq.receive({...s,page:s.projects[0].root}),state);
    await page.locator('[data-a=workspace][data-task="7"]').first().click();
    const open=await page.evaluate(()=>messages.find(m=>m.a==='workspace')?.open);assert.equal(open.task,7);assert.equal(open.page,'answer');assert.equal(open.board,board);
   }
   await page.setContent(fs.readFileSync('build/web/workspace.html','utf8'));
   await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>{const m=JSON.parse(s);messages.push(m);if(m.a==='reply-file')window.copyReplyFile(m);}};});
   await page.evaluate(s=>taskq.receive(s),state);
   await page.evaluate(c=>taskq.receive({open:c}),{page:'answer',board,task:7,revision:20});
   const pick=page.getByLabel('Attach images or files',{exact:true}),send=page.locator('[data-a=answer-send],.qac button[type=submit]').first();
   await pick.setInputFiles([{name:'photo.png',mimeType:'image/png',buffer:fs.readFileSync('tests/fixtures/reply-photo.png')},{name:'report.pdf',mimeType:'application/pdf',buffer:Buffer.alloc(20*1024*1024,0xab)}]);
   await page.waitForFunction(()=>messages.filter(m=>m.a==='reply-file').length===2).catch(async e=>{console.error('read diagnostic',surface,errors,await page.evaluate(()=>messages.filter(m=>m.a==='answer-local').slice(-3).map(m=>({key:m.key,files:m.local.files})))) ;throw e;});
   await page.waitForFunction(()=>{const local=messages.filter(m=>m.a==='answer-local').at(-1)?.local;return local?.files?.length===2&&local.files.every(f=>f.hash&&!f.pending);}).catch(async e=>{console.error('copy diagnostic',surface,errors,await page.evaluate(()=>messages.filter(m=>m.a==='answer-local').slice(-4).map(m=>({key:m.key,files:m.local.files})))) ;throw e;});
   assert.equal(await send.isDisabled(),false,'file-only reply enables Send');
   const text=page.locator('[data-c-field=answer],[data-qac=input]').first();await text.fill('Mixed text plus files');
   if(process.env.TASKQ_EVIDENCE_1817)await page.screenshot({path:process.env.TASKQ_EVIDENCE_1817+'/'+surface+'-before.png'});
   await send.click();await page.waitForFunction(()=>messages.some(m=>m.a==='answer'));
   const op=await page.evaluate(()=>messages.find(m=>m.a==='answer').op);
   assert.equal(op.text,'Mixed text plus files');assert.equal(op.files.length,2);assert.equal(op.files[1].bytes,20*1024*1024);
   assert(op.files.every(f=>!Object.hasOwn(f,'data')&&copies.has(f.id)),'send contains native-copy identities, not base64 or source paths');
   await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:op.id,state:'rejected',reason:'report.pdf: board HTTP 413; provider refuses this upload',next:'Choose a provider-supported file and retry'});
   assert.match(await page.locator('.qac-receipt,#answer-draft-reason').allTextContents().then(x=>x.join(' ')),/HTTP 413.*provider-supported/,'exact refusal and fix stay next to Send');
   if(process.env.TASKQ_EVIDENCE_1817)await page.screenshot({path:process.env.TASKQ_EVIDENCE_1817+'/'+surface+'-refusal.png'});
   for(const name of ['photo.png','report.pdf'])await page.getByRole('button',{name:'Remove '+name,exact:true}).click();
   await text.evaluate(el=>{const files=new DataTransfer();files.items.add(new File(['invented pasted image'],'paste.png',{type:'image/png'}));el.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,clipboardData:files}));});
   await text.evaluate(el=>{const files=new DataTransfer();files.items.add(new File(['invented dropped bytes'],'drop.bin',{type:'application/octet-stream'}));el.dispatchEvent(new DragEvent('drop',{bubbles:true,dataTransfer:files}));});
   await page.waitForFunction(()=>{const f=messages.filter(m=>m.a==='answer-local').at(-1)?.local.files;return f?.length===2&&f.every(f=>f.hash&&!f.pending);});
   await page.getByLabel('Attach images or files',{exact:true}).setInputFiles({name:'refused.bin',mimeType:'application/octet-stream',buffer:Buffer.from('invented bytes')});
   await page.waitForFunction(()=>messages.filter(m=>m.a==='answer-local').at(-1)?.local.files?.some(f=>f.error?.includes('Disk full')));
   assert(await send.isDisabled());assert.match((await page.locator('#answer-draft-reason,.qac-hint').allTextContents()).join(' '),/Disk full.*select the file again/);
   assert(await page.evaluate(()=>messages.some(m=>m.a==='web-observation'&&m.row.operation==='js.reply.files.refused.storage')),'telemetry records the attachment refusal kind');
   const observed=await page.evaluate(()=>messages.filter(m=>m.a==='web-observation'));
   for(const secret of ['Mixed text plus files','report.pdf','invented dropped bytes'])assert(!JSON.stringify(observed).includes(secret),'telemetry must omit reply content and file names');
   assert.deepEqual(errors,[]);await page.close();console.log('PASS '+surface+': image + 20 MiB document, paste/drop/pick, mixed reply, native-copy IDs and preserved refusal');
  }
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
