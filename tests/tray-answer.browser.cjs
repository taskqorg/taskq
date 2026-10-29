// Chain: real tray HTML -> shared card -> existing IPC. Instrument with Playwright; not native tray/board proof.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const html=fs.readFileSync('build/web/workspace.html','utf8');
const fx=JSON.parse(fs.readFileSync('tests/fixtures/answer-card.json','utf8'));
(async()=>{
  const browser=await chromium.launch(require('./browser.cjs')());
  try{
    for(const theme of ['light','dark'])for(const font of [16,20]){
      const page=await browser.newPage({viewport:{width:font===20?360:320,height:900},colorScheme:theme});
      const errors=[];page.on('pageerror',e=>errors.push(e.message));
      await page.setContent(html);
      await page.evaluate(()=>{window.messages=[];window.ipc={postMessage:s=>window.messages.push(JSON.parse(s))};});
      const board='https://github.com/o/r/issues',root='project',rows=fx.cases.find(c=>c.name.startsWith('stale')).rows;
      const palette=theme==='dark'?'--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--review:#BCA3EB':'--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--review:#8250DF';
      const state={font,theme,palette,projects:[{root,name:'project',issues:board,alive:true,complete:true,focus:[],labels:[],tasks:[rows[0]],cap_saved:{error:'not configured'}}],ai:{draft:''},notify:{projects:{}}};
      await page.evaluate(s=>taskq.receive(s),state);
      await page.evaluate(c=>taskq.receive({open:c}),{page:'answer',board,task:rows[0].n,revision:rows[0].revision});
      // #1430: every poll rebuilt the menu and the card, so the textarea being typed in was replaced and native edits
      // landed on the detached node; Send stayed disabled. Risk kept: a refresh that touches DOM it did not change.
      await page.evaluate(()=>{window.mutations=0;new MutationObserver(l=>mutations+=l.length).observe(document.body,{subtree:true,childList:true,attributes:true,characterData:true});});
      await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await page.evaluate(()=>mutations),0,'an unchanged refresh changes no DOM');
      // #1662 A3: the answer is typed in the shared composer below the card
      const text=page.locator('[data-c-field=answer]'),typed=await text.elementHandle();await typed.focus();
      for(const ch of 'yes'){await page.keyboard.type(ch);await page.evaluate(s=>taskq.receive(s),state);}
      assert(await typed.evaluate(t=>t.isConnected&&document.activeElement===t&&t.value==='yes'),'refreshes keep the node being typed in');
      assert.equal(await page.locator('[data-a=answer-send]').isDisabled(),false,'Send enables for a valid answer');
      // #1816 / § 5: long replies send; an empty reply still explains how to enable Send.
      const why=()=>page.locator('#answer-draft-reason').textContent(),send=page.locator('[data-a=answer-send]');
      await text.fill('');await typed.focus();
      for(const line of ['1. one','2. two','3. three','4. four','5. five']){await page.keyboard.type(line);await page.keyboard.press('Shift+Enter');await page.evaluate(s=>taskq.receive(s),state);}
      assert.equal(await send.isDisabled(),false);assert.equal(await why(),'');
      await text.fill('word\n'.repeat(400));await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await send.isDisabled(),false);assert.equal(await why(),'');
      const long='word '.repeat(53)+'ok';assert.equal(long.length,267);
      await text.fill(long);await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await send.isDisabled(),false);assert.equal(await why(),'');
      if(process.env.TASKQ_EVIDENCE_1816&&theme==='dark'&&font===16)await page.screenshot({path:process.env.TASKQ_EVIDENCE_1816+'/01-before-send.png'});
      await text.fill('');assert(await send.isDisabled());assert.match(await why(),/Type an answer/);
      await text.fill('a valid answer');await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await send.isDisabled(),false);assert.equal(await why(),'');
      await text.fill('keep draft');await text.focus();
      await text.evaluate(t=>t.setSelectionRange(2,4));
      const scroll=()=>page.locator('.content').evaluate(c=>c.scrollTop),top=await scroll();
      await page.evaluate(s=>taskq.receive(s),state);
      assert.deepEqual(await text.evaluate(t=>({draft:t.value,focused:document.activeElement===t,caret:[t.selectionStart,t.selectionEnd]})),{draft:'keep draft',focused:true,caret:[2,4]});
      // a changed refresh repaints and keeps draft, caret, focus and scroll
      state.projects[0].tasks=[{...rows[0],title:rows[0].title+' (edited)'}];await page.evaluate(s=>taskq.receive(s),state);
      assert((await page.locator('.qac-head').textContent()).includes('(edited)'));
      assert.deepEqual(await text.evaluate(t=>({draft:t.value,focused:document.activeElement===t,caret:[t.selectionStart,t.selectionEnd]})),{draft:'keep draft',focused:true,caret:[2,4]});
      assert.equal(await scroll(),top);
      state.projects[0].tasks=[rows[1]];await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'stale');
      assert.equal(await text.inputValue(),'keep draft');
      assert(await send.isDisabled());
      assert.match(await page.locator('.qac-receipt').textContent(),/question changed/,'a stale card states why Send is off');
      await page.locator('[data-qac=review]').click();await text.fill(long);
      await send.click();
      await page.waitForFunction(()=>messages.some(m=>m.a==='answer'));
      const op=await page.evaluate(()=>messages.find(m=>m.a==='answer').op);
      assert.equal(op.text,long);assert.equal(op.choice,undefined);assert.equal(op.revision,rows[1].revision);
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'sending');
      await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await text.inputValue(),long);
      // A1: no question card remains outside ask, even while its receipt is pending.
      state.projects[0].tasks=[{...rows[1],state:'doing',revision:rows[1].revision+1}];
      await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await page.locator('.qac').count(),0);
      assert((await page.locator('[role=status]').allTextContents()).some(t=>t.includes('sending')));
      // The refresh past the op's revision reconciles once with the same op.
      assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='answer').map(m=>m.op)),[op,op]);
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:op.id,state:'unknown',reason:'response lost'});
      await page.locator('[data-a=answer-retry]').click();
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===3);
      assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='answer').map(m=>m.op)),[op,op,op]);
      const rowTop=()=>page.locator('.content').evaluate(e=>e.getBoundingClientRect().top);
      const before=await rowTop();
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:op.id,state:'accepted'});
      await page.waitForFunction(()=>[...document.querySelectorAll('[role=status]')].some(e=>e.textContent.includes('accepted')));
      // #1741 Q-A7: selected-thread receipts are inline once; global Answered/toast stays covered by tray-answer.test.cjs.
      assert.equal(await page.locator('.toast').count(),0,'no duplicate workspace toast');
      assert.equal(await rowTop(),before,'a receipt must not move task rows');
      assert.equal(await page.locator('.qac').count(),0);
      assert.equal(await page.locator('.qac textarea, [data-c-field=answer]').count(),0);
      if(process.env.TASKQ_EVIDENCE_1816&&theme==='dark'&&font===16)await page.screenshot({path:process.env.TASKQ_EVIDENCE_1816+'/02-after-success.png'});
      assert.equal(await page.locator('.conversation-detail .body a[data-url]').first().getAttribute('href'),board+'/'+rows[1].n);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
      // Receipt technical details retain their explicit toggle through refresh and Back.
      const answeredOpen=()=>page.locator('details.ids[data-view-key^="receipt:"]').evaluate(d=>d.open);
      await page.evaluate(()=>taskq.receive({open:{page:'messages'}}));
      assert.equal(await answeredOpen(),false,'receipt details start collapsed');
      await page.locator('details.ids[data-view-key^="receipt:"] > summary').click();
      await page.evaluate(s=>taskq.receive({...s,page:undefined}),state);
      assert.equal(await answeredOpen(),true,'refresh keeps an explicit toggle');
      await page.locator('.shell-overflow > summary').click();await page.locator('.overflow-actions [data-route=sessions]').click();await page.locator('[data-a=back]').click();
      assert.equal(await answeredOpen(),true,'Back keeps an explicit toggle');
      await page.locator('details.ids[data-view-key^="receipt:"] > summary').click();
      await page.evaluate(s=>taskq.receive(s),state);
      const ask={...rows[1],n:42,state:'ask',card:'Reviewer cannot continue.',question:{parsed:false,text:'Reviewer cannot continue.',choices:[]}};
      state.projects[0].tasks=[ask,...['doing','review','ready','later','waiting'].map((state,i)=>({...rows[1],n:50+i,state}))];
      await page.evaluate(s=>taskq.receive(s),state);
      await page.evaluate(c=>taskq.receive({open:c}),{page:'answer',board,task:42});
      assert.equal(await page.locator('.qac').count(),1);
      const reason=page.getByRole('textbox',{name:'Requeue reason'});
      assert(await page.getByRole('button',{name:'Requeue',exact:true}).isDisabled());
      await reason.fill('word\n'.repeat(400).trim());
      await page.getByRole('button',{name:'Requeue',exact:true}).click();
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===4);
      const fallback=await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op);
      assert.equal(fallback.action,'requeue');assert.equal(fallback.text,'word\n'.repeat(400).trim());
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:fallback.id,state:'rejected',reason:'test refusal'});
      await page.getByRole('button',{name:'Close…',exact:true}).click();
      assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='answer').length),4,'opening confirmation writes nothing');
      await page.getByRole('button',{name:'Keep task',exact:true}).click();
      assert.equal(await page.getByRole('button',{name:'Close without publishing',exact:true}).count(),0);
      await page.getByRole('button',{name:'Close…',exact:true}).click();
      if(process.env.TASKQ_EVIDENCE_1464&&theme==='light'&&font===20)await page.screenshot({path:process.env.TASKQ_EVIDENCE_1464+'/fallback-close-light-20-360.png'});
      await page.getByRole('button',{name:'Close without publishing',exact:true}).focus();
      await page.keyboard.press('Enter');
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===5);
      const cancel=await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op);
      assert.equal(cancel.action,'close');assert.equal(cancel.confirmed,true);
      state.projects[0].tasks[0]={...ask,state:'later'};await page.evaluate(s=>taskq.receive(s),state);
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:cancel.id,state:'unknown',reason:'Parked; close not verified'});
      assert.equal(await page.locator('.qac').count(),0);
      assert((await page.locator('[role=status]').allTextContents()).some(t=>t.includes('Parked; close not verified')));
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
      state.projects[0].tasks=[{...rows[1],n:61,state:'ask'}];await page.evaluate(s=>taskq.receive(s),state);
      await page.evaluate(c=>taskq.receive({open:c}),{page:'answer',board,task:61});
      await page.locator('.qac [data-qac="choose"]').first().click();
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===6);
      const late=await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op);
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:late.id,state:'unknown',reason:'No bridge receipt received'});
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'unknown');
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root:'other',id:late.id,state:'accepted'});
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'unknown');
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:late.id,state:'accepted',reason:'late native receipt'});
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'accepted','late native receipt must update the mounted card without retry');
      assert.equal(await page.locator('[data-qac="retry"]').count(),0);
      assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='answer').length),6,'late receipt writes nothing');
      // #1662 A4: a receipt named only its state, so two answers in flight read alike. Risk kept: a receipt without its choice.
      assert.match(await page.locator('.qac-receipt').textContent(),/#61 answer 1\. Keep v1 until both move · Answer accepted/);
      // #1662 A4: an owner action (Problem, no options) had no text field in the tray. Risk kept: an ask the owner cannot answer.
      const own={...rows[1],n:70,title:'Board token missing',revision:1,card:'Problem: add the board token',question:{...rows[1].question,text:'Problem: add the board token',done:null,problem:'add the board token',choices:[]}};
      state.projects[0].tasks=[own];await page.evaluate(s=>taskq.receive(s),state);
      await page.evaluate(c=>taskq.receive({open:c}),{page:'answer',board,task:70});
      assert.equal(await page.locator('.qac [data-qac=choose]').count(),0);
      await text.fill('Token added');await page.evaluate(s=>taskq.receive(s),state);
      await page.getByRole('textbox',{name:'Your answer'}).press('Enter');
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===7);
      const action=await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op);
      assert.equal(action.text,'Token added');assert.equal(action.action,undefined);
      // #1662 A4: a bridge refusal before dispatch read as delivery unknown. Risk kept: unsent read as maybe sent, or a new id on retry.
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:action.id,state:'failed',reason:'Answer queue is full',next:'Nothing reached the board; send again'});
      assert.match(await why(),/Answer queue is full.*Nothing reached the board; send again/,'the exact refusal and fix stay beside Send');
      assert(await page.evaluate(()=>messages.some(m=>m.a==='web-observation'&&m.row.operation==='js.workspace.answer.refused.failed')),'telemetry records the refusal kind');
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'failed');
      assert.match(await page.locator('.qac-receipt').textContent(),/#70 text answer · Not sent/);
      await page.locator('.qac [data-qac=retry]').click();
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===8);
      assert.deepEqual(await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op),action,'Send again keeps the operation id and payload');
      // the composer repaints on the next tick after the card saves
      await page.waitForFunction(()=>['send','requeue'].every(a=>document.querySelector('[data-a=answer-'+a+']').disabled),null,{timeout:2000});
      if(process.env.TASKQ_EVIDENCE_1662&&font===16)await page.screenshot({path:process.env.TASKQ_EVIDENCE_1662+'/owner-action-'+theme+'.png'});
      // A long kept answer can steer the working agent after another client answered first.
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:action.id,state:'stale',answered:{by:'other client',text:'Continue'}});
      state.projects[0].tasks=[{...own,state:'doing',revision:2,answered:{by:'other client',text:'Continue'}}];
      await page.evaluate(s=>taskq.receive(s),state);
      const steer=page.locator('[data-c-field=steer]');
      await steer.fill(long);await page.getByRole('button',{name:'Steer',exact:true}).click();
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===9);
      const steering=await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op);
      assert.equal(steering.action,'steer');assert.equal(steering.text,long);
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:steering.id,state:'rejected',reason:'Worker is no longer running',next:'Refresh the task'});
      assert.match(await page.locator('#steer-draft-reason').textContent(),/Worker is no longer running.*Refresh the task/);
      assert.deepEqual(errors,[]);
      console.log('PASS tray HTML draft/focus, stale review, literal text, sending refresh, same-ID retry, accepted readback, board link;',theme,font+'px');
      await page.close();
    }
  }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
