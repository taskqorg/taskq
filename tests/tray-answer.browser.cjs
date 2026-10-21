// Chain: real tray HTML -> shared card -> existing IPC. Instrument with Playwright; not native tray/board proof.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
const html=fs.readFileSync('build/web/tray.html','utf8');
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
      const state={page:root,font,theme,projects:[{root,name:'project',issues:board,alive:true,complete:true,focus:[],labels:[],tasks:[rows[0]],cap_saved:{error:'not configured'}}],ai:{draft:''},notify:{projects:{}}};
      await page.evaluate(s=>taskq.receive(s),state);
      // #1430: every poll rebuilt the menu and the card, so the textarea being typed in was replaced and native edits
      // landed on the detached node; Send stayed disabled. Risk kept: a refresh that touches DOM it did not change.
      await page.evaluate(()=>{window.mutations=0;new MutationObserver(l=>mutations+=l.length).observe(document.body,{subtree:true,childList:true,attributes:true,characterData:true});});
      await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await page.evaluate(()=>mutations),0,'an unchanged refresh changes no DOM');
      await page.locator('.qac-other summary').click();const text=page.locator('.qac textarea'),typed=await text.elementHandle();await typed.focus();
      for(const ch of 'yes'){await page.keyboard.type(ch);await page.evaluate(s=>taskq.receive(s),state);}
      assert(await typed.evaluate(t=>t.isConnected&&document.activeElement===t&&t.value==='yes'),'refreshes keep the node being typed in');
      assert.equal(await page.locator('.qac button[type=submit]').isDisabled(),false,'Send enables for a valid answer');
      // #1456: a custom answer over the § 5 answer limits (4 lines, 240 characters) disabled Send with an empty card,
      // so the owner could not tell why. Risk kept: Send disabled while the card shows no reason.
      const why=()=>page.locator('.qac-hint').textContent(),send=page.locator('.qac button[type=submit]');
      await text.fill('');await typed.focus();
      for(const line of ['1. one','2. two','3. three','4. four','5. five']){await page.keyboard.type(line);await page.keyboard.press('Shift+Enter');await page.evaluate(s=>taskq.receive(s),state);}
      assert(await send.isDisabled());assert.match(await why(),/At most 4 lines/,'a disabled Send states why');
      await text.fill('x'.repeat(241));await page.evaluate(s=>taskq.receive(s),state);
      assert(await send.isDisabled());assert.match(await why(),/At most 240 characters/);
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
      assert(await page.locator('.qac button[type=submit]').isDisabled());
      assert.match(await page.locator('.qac-receipt').textContent(),/question changed/,'a stale card states why Send is off');
      await page.locator('[data-qac=review]').click();await text.fill('2');
      await page.locator('.qac button[type=submit]').click();
      await page.waitForFunction(()=>messages.some(m=>m.a==='answer'));
      const op=await page.evaluate(()=>messages.find(m=>m.a==='answer').op);
      assert.equal(op.text,'2');assert.equal(op.choice,undefined);assert.equal(op.revision,rows[1].revision);
      assert.equal(await page.locator('.qac').getAttribute('data-qac-state'),'sending');
      await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await text.inputValue(),'2');
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
      const rowTop=()=>page.locator('.tqp-task').first().evaluate(e=>e.getBoundingClientRect().top);
      const before=await rowTop();
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:op.id,state:'accepted'});
      await page.waitForFunction(()=>[...document.querySelectorAll('[role=status]')].some(e=>e.textContent.includes('accepted')));
      // #1148 M3: the accepted toast overlays; task rows keep their position.
      assert.equal(await page.locator('.toast').evaluate(e=>getComputedStyle(e).position),'fixed');
      assert.equal(await rowTop(),before,'a receipt must not move task rows');
      assert.equal(await page.locator('.qac').count(),0);
      assert.equal(await page.locator('.qac textarea').count(),0);
      assert.equal(await page.locator('.tqp-task a[data-url]').first().getAttribute('href'),board+'/'+rows[1].n);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
      // #1148 M3: Answered starts collapsed on Attention, whose other groups open; an explicit toggle survives refresh and Back.
      const answeredOpen=()=>page.locator('details.answered').evaluate(d=>d.open);
      await page.evaluate(s=>taskq.receive({...s,page:'attention'}),state);
      assert.equal(await answeredOpen(),false,'Attention must not expand Answered');
      await page.locator('details.answered > summary').click();
      await page.evaluate(s=>taskq.receive({...s,page:undefined}),state);
      assert.equal(await answeredOpen(),true,'refresh keeps an explicit toggle');
      await page.locator('.shell-overflow > summary').click();await page.locator('[data-route=settings]').click();await page.locator('[data-a=back]').click();
      assert.equal(await answeredOpen(),true,'Back keeps an explicit toggle');
      await page.locator('details.answered > summary').click();
      await page.evaluate(s=>taskq.receive(s),state);
      const ask={...rows[1],n:42,state:'ask',card:'Reviewer cannot continue.',question:{parsed:false,text:'Reviewer cannot continue.',choices:[]}};
      state.projects[0].tasks=[ask,...['doing','review','ready','later','waiting'].map((state,i)=>({...rows[1],n:50+i,state}))];
      await page.evaluate(s=>taskq.receive(s),state);
      assert.equal(await page.locator('.qac').count(),1);
      const reason=page.getByRole('textbox',{name:'Requeue reason'});
      assert(await page.getByRole('button',{name:'Requeue',exact:true}).isDisabled());
      await reason.fill('Retry after fixing the check');
      await page.getByRole('button',{name:'Requeue',exact:true}).click();
      await page.waitForFunction(()=>messages.filter(m=>m.a==='answer').length===4);
      const fallback=await page.evaluate(()=>messages.filter(m=>m.a==='answer').at(-1).op);
      assert.equal(fallback.action,'requeue');assert.equal(fallback.text,'Retry after fixing the check');
      await page.evaluate(r=>taskq.receive({answerReceipt:r}),{root,id:fallback.id,state:'rejected',reason:'test refusal'});
      await page.getByRole('button',{name:'Close…',exact:true}).click();
      assert.equal(await page.evaluate(()=>messages.filter(m=>m.a==='answer').length),4,'opening confirmation writes nothing');
      await page.getByRole('button',{name:'Keep task',exact:true}).click();
      assert.equal(await page.getByRole('button',{name:'Close without publishing',exact:true}).count(),0);
      await page.getByRole('button',{name:'Close…',exact:true}).click();
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
      assert.deepEqual(errors,[]);
      console.log('PASS tray HTML draft/focus, stale review, literal text, sending refresh, same-ID retry, accepted readback, board link;',theme,font+'px');
      await page.close();
    }
  }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
