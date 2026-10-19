'use strict';
const ui=require('./ui-runtime.js'),execution=ui.execution;
const api=execution.module('workspace',{
 escape:s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])),
 terminal:a=>['result','refuse','clarify','applied'].includes(a?.state)&&a?.native_state!=='unknown',
 effects:a=>[...(a.effects||[]),...(a.ops||[]).map(o=>Object.entries(o).map(([key,value])=>key+': '+(typeof value==='object'?JSON.stringify(value):value)).join(' · ')),...(a.readback||[]),...(a.receipts||[]).map(o=>Object.entries(o).map(([key,value])=>key+': '+(typeof value==='object'?JSON.stringify(value):value)).join(' · '))],
 transcript:conversation=>conversation?.error?'<p role="alert">'+api.escape(conversation.error)+'</p>':(conversation?.chain||[]).map(q=>{
  const a=q.answer||{};
  return '<article><h2>You</h2><div class="text">'+api.escape(q.text)+'</div><h2>Operator · '+api.escape(a.state||'queued')+(a.rt?' · '+api.escape(a.rt):'')+'</h2><div class="text">'+api.escape(a.text||a.reason||'Waiting for the service')+'</div>'+(a.reason&&a.text?'<p>'+api.escape(a.reason)+'</p>':'')+api.effects(a).map(line=>'<pre>'+api.escape(line)+'</pre>').join('')+'</article>';
 }).join(''),
 mount:()=>{
  let state={conversations:[],projects:[],history:[]},selected=null,draft='',pending=false,error='',search='',list=false;
  const send=ui.send,document=globalThis.document,app=document.querySelector('.workspace');
  const controller=execution.module('workspace.controller',{
   current:()=>state.conversations.find(c=>selected&&c.root===selected.root&&(c.first===selected.first||c.id===selected.id)),
   render:()=>{
    const c=controller.current(),last=c?.chain.at(-1),a=last?.answer||{},focus=document.activeElement?.id,caret=focus==='draft'?[document.activeElement.selectionStart,document.activeElement.selectionEnd]:null;
    const scroll=app.querySelector('.transcript')?.scrollTop||0;
    app.classList.toggle('list-open',list);
    app.innerHTML='<aside><button class="narrow" data-a="list">Back to conversation</button><h1>Conversations</h1><button data-a="new">New conversation</button><input id="search" aria-label="Search conversations" placeholder="Search" value="'+api.escape(search)+'">'+state.conversations.filter(c=>(c.project+' '+c.chain?.[0]?.text).toLowerCase().includes(search.toLowerCase())).map(c=>'<button class="conversation" data-a="select" data-root="'+api.escape(c.root)+'" data-id="'+api.escape(c.id)+'"'+(c===controller.current()?' aria-current="true"':'')+'><strong>'+api.escape(c.project)+'</strong><span>'+api.escape(c.chain?.[0]?.text||c.error)+'</span><small>'+api.escape(c.chain?.at(-1)?.answer?.state||'queued')+'</small></button>').join('')+'</aside><main><header><button class="narrow" data-a="list">Conversations</button><h1>'+api.escape(c?.project||'New conversation')+'</h1><small>AI conversation · work sessions stay in project tasks</small></header><section class="transcript" tabindex="0" aria-label="Full exchange">'+api.transcript(c)+'</section><section class="composer">'+(error||state.error?'<p role="alert">'+api.escape(error||state.error)+'</p>':'')+(a.state==='proposal'?'<div class="review"><strong>Review changes · revision '+api.escape(a.rev)+'</strong><button data-a="confirm"'+(pending||!Number.isSafeInteger(a.rev)?' disabled':'')+'>Confirm revision '+api.escape(a.rev)+'</button></div>':'')+(['applying','apply failed','unknown','failed'].includes(a.state)?'<p>Delivery '+api.escape(a.state)+'. Reconcile the shown outcomes before any retry.</p>':'')+(!c?'<label>Project <select id="project">'+state.projects.map(p=>'<option value="'+api.escape(p.root)+'"'+(selected?.root===p.root?' selected':'')+'>'+api.escape(p.name)+'</option>').join('')+'</select></label>':'')+'<textarea id="draft" aria-label="Message in this conversation" placeholder="Write a message; Shift+Enter for a new line">'+api.escape(draft)+'</textarea><div class="actions"><button data-a="send"'+(pending||!draft.trim()||!!c&&!api.terminal(a)||!!c?.error||!state.projects.length?' disabled':'')+'>Send</button><button data-a="refresh">Refresh</button><details><summary>Command history</summary>'+state.history.map((t,i)=>'<button data-a="history" data-index="'+i+'">'+api.escape(t)+'</button>').join('')+'</details></div></section></main>';
    const transcript=app.querySelector('.transcript');transcript.scrollTop=scroll;
    if(focus){const el=document.getElementById(focus);el?.focus({preventScroll:true});if(caret)el?.setSelectionRange(...caret);}
   },
   receive:s=>{
    if(s.font)document.documentElement.style.fontSize=s.font+'px';
    if(s.theme)document.documentElement.style.colorScheme=s.theme;
    if(s.error!==undefined)error=s.error;
    if(s.receipt)pending=false;
    if(s.data){state=s.data;state.history=state.history||[];
     if(s.select||!selected){selected=state.selected;const c=controller.current();selected=c?{root:c.root,id:c.id,first:c.first}:selected;draft=c?.draft??state.draft??'';}
     else if(s.receipt&&s.sent){selected=state.selected;const c=controller.current();selected=c?{root:c.root,id:c.id,first:c.first}:selected;draft=c?.draft??'';}
    }
    controller.render();
   },
   draft:()=>{const c=controller.current();send({a:'draft',root:c?.root||selected?.root||'',id:c?.id||'',text:draft});},
   action:a=>{
    const c=controller.current(),last=c?.chain.at(-1);
    if(a==='send'){
     if(pending||!draft.trim()||c&&!api.terminal(last?.answer)||c?.error)return;
     pending=true;send({a:'send',root:c?.root||document.getElementById('project')?.value,id:c?.id||'',text:draft});
    }else if(a==='confirm'){
     if(pending||last?.answer?.state!=='proposal')return;
     pending=true;send({a:'confirm',root:c.root,id:c.id,rev:last.answer.rev});
    }else if(a==='refresh')send({a:'refresh'});
    controller.render();
   }
  });
  globalThis.taskq={receive:controller.receive,measure:execution.module('workspace',{measure:name=>{const d=document.documentElement;if(d.scrollWidth>d.clientWidth)send({a:'overflow',shot:name,x:d.scrollWidth-d.clientWidth});}}).measure};
  ui.listen(app,'click',e=>{
   const b=e.target.closest('button');if(!b||b.disabled)return;const d=b.dataset;
   if(d.a==='list'){list=!list;controller.render();return;}
   if(d.a==='new'){list=false;controller.draft();selected={root:state.projects[0]?.root};draft=state.draft||'';error='';controller.render();return;}
   if(d.a==='select'){list=false;controller.draft();send({a:'select',root:d.root,id:d.id});return;}
   if(d.a==='history'){draft=state.history[Number(d.index)]||'';controller.draft();controller.render();return;}
   controller.action(d.a);
  });
  ui.listen(app,'input',e=>{
   if(e.target.id==='draft'){draft=e.target.value;controller.draft();const c=controller.current();app.querySelector('[data-a="send"]').disabled=pending||!draft.trim()||!!c&&!api.terminal(c.chain.at(-1)?.answer)||!!c?.error;}
   if(e.target.id==='search'){search=e.target.value;controller.render();}
  });
  ui.listen(app,'change',e=>{if(e.target.id==='project')selected={root:e.target.value};});
  ui.listen(app,'keydown',e=>{if(e.target.id==='draft'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing){e.preventDefault();controller.action('send');}});
  send({a:'ready'});
 }
});
module.exports=api;
