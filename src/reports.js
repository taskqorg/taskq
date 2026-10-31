'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const card=require('./answer-card.js'),composer=require('./pm-composer.js').composer;

// §7: both surfaces use this view; the board owns read/decision state, memory holds only an in-flight view.
const {create,body}=execution.module('reports',{
create:function({send=ui.send,draw,defer=ui.defer,onOpen,id=()=>Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')}){
 let projects=[],pages=new Map(),pending=new Map(),selected=null,notice='',draft=null,successor=null,readFailure='',readQueue=[];
 const receipts=new Map(),outcomes=new Map(),key=(root,n)=>root+'#'+n;
 const saved={},controllers=new Map(),displayedReads=new Set();
 const settled=r=>['accepted','drop','made','answered'].includes(r.decision);
 const namespace=(r,kind)=>'result:'+(projects.find(p=>p.root===r.root)?.issues||r.root)+'#'+r.n+'@'+r.sha+':'+kind;
 const receiptKey=(root,n,sha)=>key(root,n)+'@'+sha;
 const active=op=>selected?.root===op.root&&selected.n===op.n&&selected.sha===op.sha;
 function confirmed(op,r){return r.report?.n===op.n&&r.report?.sha===op.sha&&(op.action==='read'?r.report.read:op.action==='open'?true:op.action==='accept'?r.report.decision==='accepted':op.action==='later'?r.report.decision==='later'&&(!op.until||r.report.later_until===op.until):op.action==='drop'?r.report.decision==='drop':op.action==='make'?r.report.decision==='made':r.successor>0);}
 function outcome(op,text,next,shown=active(op)){const k=receiptKey(op.root,op.n,op.sha),prior=outcomes.get(k)||{},value={notice:text===undefined?prior.notice||'':text,successor:next===undefined?prior.successor||null:next};outcomes.set(k,value);if(shown){notice=value.notice;successor=value.successor;}}
 function targets(ps){const seen=new Set();return ps.filter(p=>{const board=p.issues||p.root;if(seen.has(board))return false;seen.add(board);return true;});}
 function request(root,action,extra={},resolve){
  if([...pending.values()].some(p=>p.id!==extra.id&&p.root===root&&p.n===extra.n&&p.sha===extra.sha&&(action==='read'||p.action!=='read'))) {resolve?.({state:'unknown',reason:'Another result operation needs readback'});return;}
  const op={id:id(),root,action,...extra};pending.set(op.id,op);send({a:'reports',...op});
  if(resolve)op.resolve=resolve;
  defer(()=>{if(pending.get(op.id)!==op)return;op.unknown=true;if(op.action==='read'){pending.delete(op.id);readFailure='Result read outcome unknown; waiting for service readback.';}op.resolve?.({state:'unknown',reason:'No bridge receipt received; check the same operation'});op.resolve=null;outcome(op,'Result outcome unknown; waiting for service readback.');api.readNext();draw();},180000);
 }
 function readShown(r){const k=receiptKey(r.root,r.n,r.sha);if((r.body||r.kind==='task')&&!r.read&&!displayedReads.has(k)){displayedReads.add(k);request(r.root,'read',{n:r.n,sha:r.sha});}}
 function summary(text,heading){
  const lines=(text||'').split('\n'),start=lines.findIndex(l=>l.replace(/^#+\s*/,'').trim().toLowerCase()===heading.toLowerCase());
  if(start<0)return '';
  const rest=lines.slice(start+1),end=rest.findIndex(l=>/^#{1,6}\s/.test(l));return rest.slice(0,end<0?undefined:end).join('\n').trim();
 }
 function field(text,label){const lines=(text||'').split('\n'),start=lines.findIndex(l=>l.startsWith(label+':'));if(start<0)return 'Unknown: not recorded in the accepted hand-in.';const rest=lines.slice(start+1),end=rest.findIndex(l=>/^(Changed|Checks|Open|Report):/.test(l)&&!l.startsWith(label+':'));return [lines[start],...rest.slice(0,end<0?undefined:end)].map(l=>l.startsWith(label+':')?l.slice(label.length+1).trim():l).join('\n');}
 function remember(op,report){
  const k=receiptKey(op.root,op.n,op.sha),won=receipts.get(k);
  const collected=pages.get(op.root)?.rows.find(r=>r.n===op.n&&r.sha===op.sha);
  if(Number.isSafeInteger(collected?.comments_through)&&(!Number.isSafeInteger(report.comments_through)||collected.comments_through>report.comments_through))return collected;
  if(won?.sha===op.sha&&won.visible===false)return won;
  // Receipts prove marker state, never the close time or immutable source. Late read replies cannot undo Later/Make.
  const fields={sha:report.sha};
  for(const field of ['read','decision','visible','pending_decision','tasks_made','comments_through','later_until'])if(Object.hasOwn(report,field))fields[field]=report[field];
  if(op.action==='read'&&won?.sha===op.sha){if(['later','accepted'].includes(won.decision))fields.decision=won.decision;if(won.decision==='later')fields.later_until=won.later_until;if(won.pending_decision==='make')fields.pending_decision='make';}
  if(Number.isSafeInteger(won?.comments_through)&&Number.isSafeInteger(fields.comments_through))fields.comments_through=Math.max(won.comments_through,fields.comments_through);
  const merged={...(won?.sha===op.sha?won:{}),...fields};receipts.set(k,merged);return merged;
 }
 const api=execution.module('reports.view',{
  sync(ps){
   projects=targets(ps);
   for(const p of projects){
    const data=p.reports;
    pages.set(p.root,{rows:[...(data?.reports||[]),...(data?.history||[])].map(r=>{const k=receiptKey(p.root,r.n,r.sha),won=receipts.get(k);if(Number.isSafeInteger(r.comments_through)&&Number.isSafeInteger(won?.comments_through)&&r.comments_through>=won.comments_through)receipts.delete(k);return receipts.has(k)?{...r,...won}:r;}),unknown:data?.complete!==true,reason:data?.reason||(data?.complete===true?'':'Reports not collected yet')});
   }
   if(selected){const row=api.rows(true).find(r=>key(r.root,r.n)===key(selected.root,selected.n)&&r.sha===selected.sha);selected=row?{...selected,...row}:null;}
   for(const op of pending.values())if(op.unknown&&!op.checking&&!pages.get(op.root)?.unknown){const row=pages.get(op.root)?.rows.find(r=>r.n===op.n&&r.sha===op.sha);if(row&&(confirmed(op,{report:row})||settled(row)&&['accept','later','drop','make'].includes(op.action))){op.checking=true;defer(()=>api.receive({id:op.id,root:op.root,report:row}),0);}}
  },
  refresh(){api.sync(projects);draw();},
  rows(history=false){return projects.flatMap(p=>(pages.get(p.root)?.rows||[]).filter(r=>history||r.visible!==false).map(r=>({...r,root:p.root}))).sort((a,b)=>(b.closed||0)-(a.closed||0)||b.n-a.n);},
  unread(){return api.rows().filter(r=>!r.read);},
  // R6 Decide: mirrors reports::decide; an undated or past Later returns, the oldest close first.
  undecided(r){const until=Date.parse(r.later_until||'');return r.kind!=='task'&&r.visible!==false&&!r.pending_decision&&(!r.decision||r.decision==='none'||r.decision==='later'&&!(until>Date.now()));},
  decide(rows=api.rows()){return rows.filter(api.undecided).sort((a,b)=>(a.closed||0)-(b.closed||0)||a.n-b.n);},
  count(){return projects.every(p=>pages.has(p.root)&&!pages.get(p.root).unknown)?api.unread().length:null;},
  reason(){return projects.flatMap(p=>pages.get(p.root)?.reason?[p.name+': '+pages.get(p.root).reason]:[]).join('; ');},
  current(){return selected;},
  feedback(){return notice;},
  readFailure(){return readFailure;},
  markRead(rows){
   readFailure='';const seen=new Set([...readQueue,...pending.values()].map(r=>receiptKey(r.root,r.n,r.sha)));
   for(const r of rows)if(!seen.has(receiptKey(r.root,r.n,r.sha))){readQueue.push(r);seen.add(receiptKey(r.root,r.n,r.sha));}
   api.readNext();
  },
  // M14: serialize bulk reads through the existing bounded native command queue.
  readNext(){if([...pending.values()].some(p=>p.action==='read'))return;const r=readQueue.shift();if(r)request(r.root,'read',{n:r.n,sha:r.sha});},
  open(root,n,sha){return execution.action('reports.card.open',()=>{const r=api.rows(true).find(r=>r.root===root&&r.n===n&&(!sha||r.sha===sha));if(!r)return false;selected={...r};displayedReads.delete(receiptKey(root,n,r.sha));const prior=outcomes.get(receiptKey(root,n,r.sha));notice=prior?.notice||'';draft=null;successor=prior?.successor||api.savedSuccessor(r);onOpen?.(selected);draw();readShown(r);return true;})();},
  savedSuccessor(r){const n=Object.values(saved).find(v=>v.request?.root===r.root&&v.request?.n===r.n&&v.request?.sha===r.sha&&v.receipt?.state==='accepted'&&v.successor>0)?.successor;return n?{n,url:r.url?.replace(/\d+$/,String(n))}:null;},
  focus(root,n){const r=api.rows(true).find(r=>r.root===root&&r.n===n);if(!r||active(r))return;selected={...r};const prior=outcomes.get(receiptKey(root,n,r.sha));notice=prior?.notice||'';successor=prior?.successor||api.savedSuccessor(r);},
  move(step){const rows=api.rows(true),i=rows.findIndex(r=>r.root===selected?.root&&r.n===selected?.n),r=rows[i+step];if(r)api.open(r.root,r.n);},
  act(action,text,until){if(!selected)return;outcome(selected,'',null);request(selected.root,action,{n:selected.n,sha:selected.sha,...(text===undefined?{}:{text}),...(until===undefined?{}:{until})});draw();},
  receive(r){
   const op=pending.get(r.id);if(!op||op.root!==r.root)return false;const shown=active(op);
   const reply={state:r.reason?(r.receipt?.outcome==='rejected'?'rejected':r.receipts?.length?'partial':'unknown'):'accepted',reason:r.reason};
   if(r.report?.n!==op.n||r.report?.sha!==op.sha){reply.state='unknown';reply.reason=r.reason||'No matching result receipt';}
   else if(!r.reason&&!confirmed(op,r)){reply.state='rejected';reply.reason='The displayed action was not confirmed; review the current result decision.';}
   if(op.action!=='read'&&['unknown','partial'].includes(reply.state))op.unknown=true;else pending.delete(r.id);
   for(const [k,v] of Object.entries(saved))if(v.op?.id===r.id&&v.request?.root===r.root){saved[k]={...v,receipt:reply,...(reply.state==='accepted'&&v.op.text?{draft:''}:{}),...(reply.state==='accepted'&&r.successor>0?{successor:r.successor}:{} )};send({a:'answer-local',key:k,local:saved[k]});}
   op.resolve?.(reply);op.resolve=null;
   for(const c of controllers.values())c.ui.receiveReceipt(c.board,r.id,reply);
   if(reply.state!=='accepted'){
    if(op.action==='read')readFailure=reply.reason;
    if(r.report?.n===op.n&&r.report.sha===op.sha)remember(op,r.report);
    const made=Array.isArray(r.receipts)?r.receipts:[],uncertain=r.receipt?.task;
    outcome(op,reply.reason+(made.length?' · Created held drafts: '+made.map(n=>'#'+n).join(', '):'')+(uncertain>0&&!made.includes(uncertain)?' · Unverified draft: #'+uncertain:''),undefined,shown);
    api.readNext();draw();return true;
   }
   if(r.report?.n===op.n&&r.report.sha===op.sha){
    const p=pages.get(r.root),report=remember(op,r.report);
    if(p)p.rows=p.rows.map(row=>row.n!==op.n||row.sha!==op.sha?row:{...row,...report,closed:row.closed});
    if(selected&&selected.sha===op.sha&&key(selected.root,selected.n)===key(r.root,op.n))selected={...selected,...report,closed:selected.closed};
    if(op.action!=='read')outcome(op,report.visible===false?(report.decision==='made'?'Created held drafts: '+(report.tasks_made||[]).map(n=>'#'+n).join(', '):'Report dropped.'):(op.action==='later'?(op.until?'Later: back in Decide on '+op.until.slice(0,10)+'.':'Kept in Reports for later.'):''),undefined,shown);
    if(r.successor>0){outcome(op,'Created held draft. Read it back and start it separately.',{n:r.successor,url:r.successor_url||(typeof r.report.url==='string'?r.report.url.replace(/\d+$/,String(r.successor)):null)},shown);if(shown&&draft===op.action)draft=null;}
    if(op.action==='accept')outcome(op,'Result accepted.',undefined,shown);
   }else {if(op.action==='read')readFailure='Result read receipt unknown; waiting for service readback.';outcome(op,'Report receipt unknown; waiting for service readback.',undefined,shown);}
   api.readNext();draw();return true;
  },
  click(b,ps){
   const action=b.dataset.report;if(!action)return false;
   if(action==='open')api.open(b.dataset.root,Number(b.dataset.n));
   else if(action==='reason-send')api.reasonSend();
   else if(action==='date-send'&&selected){const k=namespace(selected,'decision');controllers.get(k)?.ui.act(k,selected.n,'submit');}
   else if(action==='date-cancel'&&selected){const k=namespace(selected,'decision');controllers.get(k)?.ui.act(k,selected.n,'cancel-result');}
   else if(action==='reason-cancel'&&selected){const k=namespace(selected,'correction');controllers.get(k)?.ui.act(k,selected.n,'cancel-result');}
   else if(action==='unresolved')onOpen?.({next:true});
   else if(action==='previous')api.move(-1);
   else if(action==='next')api.move(1);
   return true;
  },
  threads(){return api.rows(true).map(r=>({id:'task:'+(projects.find(p=>p.root===r.root)?.issues||r.root)+'/'+r.n,root:r.root,board:projects.find(p=>p.root===r.root)?.issues,conversation:r.conversation,task:r.n,report:r.n,title:r.title,result:r.result,updated:r.closed*1000,finished:true,needsDecision:r.kind!=='task'&&!settled(r),status:({accepted:'Accepted',drop:'Dropped',made:'Drafts created',answered:'Decided'})[r.decision]||(r.kind==='task'?'Finished':'Report'),marker:r.kind!=='task'&&!settled(r)?'Needs decision':!r.read?'Unread result':''}));},
  hydrate(locals){for(const [k,v] of Object.entries(locals||{}))if(k.startsWith('result:')&&!saved[k]&&typeof v.draft==='string'){saved[k]={...v};if(v.op&&['sending','unknown','partial'].includes(v.receipt?.state)){saved[k].receipt={state:'unknown',reason:'Workspace reopened; check the original operation'};if(v.request?.id===v.op.id&&typeof v.request.root==='string'&&Number.isSafeInteger(v.request.n)&&typeof v.request.sha==='string')pending.set(v.op.id,{...v.request,unknown:true});}}},
  cardRow(r,kind){
   const choices=kind==='decision'?[['Accept','+ Acknowledge this result; - no new work.','accept'],['Later',r.kind==='task'?'+ Keep it for another decision; - revisit it yourself.':'+ Return to Decide on a date; - choose the date before Send.','later'],['Drop','+ Dismiss this decision; - no follow-up work.','drop']]:kind==='correction'?[['Reopen','+ Request correction; - creates a linked held successor.','reopen'],['Follow up','+ Add related work; - creates a separate held draft.','followup']]:[['Make tasks','+ Create the displayed held drafts; - read back before separate Start.','make'],['Keep report','+ Create nothing; - proposed work stays unfiled.','keep']];
   return {...r,state:'closed',revision:r.sha+':'+kind,resultDecision:true,repeatable:kind!=='decision'||!settled(r),question:{parsed:true,done:kind==='correction'?'The original task stays closed.':'Work is closed; reading does not accept it.',problem:kind==='decision'?'What should happen to this result?':kind==='correction'?'Correction or additional work?':'Create these held drafts?',resultReason:kind==='correction',resultDate:kind==='decision'&&r.kind!=='task',choices:choices.map(([label,detail,action],i)=>({id:i+1,label,text:label,detail,action,...(kind==='correction'||action==='later'&&r.kind!=='task'?{reason:action}:{}),local:action==='keep',recommended:i===(kind==='correction'?1:kind==='decision'&&r.outcome!=='success'?1:0)})),evidence:[]}};
  },
  capability(r,kind){const p=projects.find(p=>p.root===r.root),ops=[...pending.values(),...Object.values(saved).filter(v=>['sending','unknown','partial'].includes(v.receipt?.state)).map(v=>v.request).filter(Boolean)],blockers=ops.filter(op=>op.root===r.root&&op.n===r.n&&op.sha===r.sha&&op.action!=='read');return {answer:!!p&&p.alive!==false&&p.complete!==false&&!pages.get(r.root)?.unknown&&!r.reason&&blockers.every(op=>op.id===saved[namespace(r,kind)]?.op?.id)&&!(kind==='decision'&&settled(r))&&!(kind==='make'&&(!r.tasks?.length||settled(r)))&&!(r.pending_decision==='make'&&kind!=='make'),composer:true,reason:'Result or project unavailable, already decided, or another decision needs readback'};},
  mount(menu){
   if(!selected)return;
   for(const slot of menu.querySelectorAll('[data-result-card]')){
    const r={...selected},kind=slot.dataset.resultCard,k=namespace(r,kind),row=api.cardRow(r,kind);let c=controllers.get(k);
    if(!c){const el=menu.ownerDocument.createElement('div');c={el,board:k,ui:card.mount(el,{newId:id,capability:()=>api.capability(api.rows(true).find(row=>row.root===r.root&&row.n===r.n&&row.sha===r.sha)||r,kind),load:()=>saved[k],save:(_b,_r,local)=>{const prior=saved[k],same=prior?.op?.id===local.op?.id;saved[k]={...local,...(same&&prior?.request?{request:prior.request}:{}),...(same&&prior?.successor?{successor:prior.successor}:{})};send({a:'answer-local',key:k,local:saved[k]});if(local.intent!==draft){draft=local.intent;draw();}},send:(_board,op)=>{
      const action=op.choice?row.question.choices.find(c=>c.id===op.choice)?.action:saved[k]?.intent;
      if(!action)return Promise.resolve({state:'rejected',reason:'Choose the result action first'});
      const current=api.rows(true).find(row=>row.root===r.root&&row.n===r.n&&row.sha===r.sha);
      if(!current||!api.capability(current,kind).answer)return Promise.resolve({state:'rejected',reason:'Result or project changed; nothing sent. Review its current state.'});
      const prior=saved[k]?.request;
      if(prior?.id!==op.id&&action==='later'&&r.kind!=='task'&&card.textProblem(row.question,op.text||''))return Promise.resolve({state:'rejected',reason:'Choose a future revisit date'});
      const requestData=prior?.id===op.id?prior:{id:op.id,root:r.root,n:r.n,sha:r.sha,action,...(op.text!==undefined?{text:op.text}:{}),...(action==='later'&&r.kind!=='task'?{until:op.text+'T12:00:00Z'}:{})};
      if(requestData.root!==r.root||requestData.n!==r.n||requestData.sha!==r.sha||requestData.action!==action||requestData.text!==op.text||action==='later'&&r.kind!=='task'&&requestData.until!==op.text+'T12:00:00Z')return Promise.resolve({state:'rejected',reason:'Saved result request differs from the displayed source; nothing sent'});
      saved[k]={...saved[k],request:requestData};return new Promise(resolve=>request(r.root,action,{...requestData,local_key:k,local:saved[k]},resolve));
    }})};controllers.set(k,c);}
    if(c.el.parentNode!==slot)slot.append(c.el);c.ui.update([{board:k,row}]);
    if(kind==='correction'&&menu.ownerDocument.defaultView.innerWidth<=620)slot.closest('details').open=true;
   }
   readShown(selected);
  },
  reasonInput(text,caret){if(!selected)return;const k=namespace(selected,'correction'),c=controllers.get(k);if(c)c.ui.input(k,selected.n,text,caret);draw();},
  reasonSend(){if(selected){const k=namespace(selected,'correction');controllers.get(k)?.ui.act(k,selected.n,'submit');}},
  dateInput(text){if(selected){const k=namespace(selected,'decision');controllers.get(k)?.ui.input(k,selected.n,text);draw();}},
  section(ps){
   api.sync(ps);const r=selected;if(!r)return h('p',null,api.count()===null?api.reason()||'Results unknown; waiting for collection.':'No results waiting for review.');
   const ordinary=r.kind==='task',position=api.rows(true).findIndex(row=>row.root===r.root&&row.n===r.n),k=namespace(r,'correction'),local=saved[k]||card.fresh(api.cardRow(r,'correction')),context=api.cardRow(r,'correction'),cap=api.capability(r,'correction');
   const decision=api.cardRow(r,'decision'),date=saved[namespace(r,'decision')]||card.fresh(decision),dateCap=api.capability(r,'decision'),dateLive=['actionable','changed'].includes(card.status(decision,date,dateCap));
   const slot=kind=>h('div',{'data-result-card':kind,key:namespace(r,kind)});
   return h('article',{class:'report-card','aria-label':ordinary?'Completed result':'Research report'},
    h('div',{class:'conversation-tools'},h('span',{class:'meta'},(ordinary?'Finished task':'Report')+' \u00b7 '+(r.read?'Read':'Unread')+' \u00b7 '+(({accepted:'Accepted',drop:'Dropped',made:'Drafts created',answered:'Decided'})[r.decision]||'Decision open')),
     h('a',{href:r.url,'data-url':r.url},'#'+r.n),h('button',{'data-report':'previous',disabled:position<=0},'Previous'),h('button',{'data-report':'next',disabled:position>=api.rows(true).length-1},'Next')),
    r.reason&&h('p',{role:'status'},'Result unknown: '+r.reason),
    !ordinary&&h('p',{class:'meta'},'Verdict: '+(r.experiment_verdict||'unknown')+' · '+(r.closed?Math.max(0,Math.floor((Date.now()/1000-r.closed)/86400))+'d':'age unknown')),
    ordinary?[...['Changed','Checks','Open'].map(label=>h('section',{key:label},h('h3',null,label),body(field(r.body,label)))),r.pr&&h('a',{href:r.pr,'data-url':r.pr},'Open PR'),r.verdict&&h('details',{'data-view-key':namespace(r,'verdict')},h('summary',null,'Review and delivery'),body(r.verdict))]:
     r.body?[h('h3',null,'Question'),body(summary(r.body,'Question')),h('h3',null,'Recommendation'),body(summary(r.body,'Recommendation'))]:h('p',null,'Report content unknown; waiting for background collection.'),
    settled(r)?h('p',{class:'meta'},'Decision recorded: '+r.decision+'. Retained in Finished.'):h('section',null,slot('decision'),date.intent==='later'&&h('div',{class:'result-date'},
     h('label',null,'Revisit on',h('input',{type:'date','data-c-field':'result-date','aria-label':'Revisit date',value:date.draft,min:new Date(Date.now()+86400000).toISOString().slice(0,10),disabled:!dateLive})),
     h('p',{class:'meta'},'The report returns to Decide on this date.'),
     card.textProblem(decision.question,date.draft)&&h('p',{role:'status'},card.textProblem(decision.question,date.draft)),
     h('div',{class:'conversation-tools'},h('button',{'data-report':'date-send',disabled:!dateLive||!!card.textProblem(decision.question,date.draft)},'Send'),h('button',{'data-report':'date-cancel',disabled:!dateLive},'Cancel')))),
    h('details',{class:'result-other',open:!!local.intent,'data-view-key':namespace(r,'other')},h('summary',null,'Other actions'),slot('correction')),
    local.intent&&composer({id:'result-reason',field:'result-reason',label:(local.intent==='reopen'?'Reopen':'Follow-up')+' reason',placeholder:'What needs to change?',text:local.draft,disabled:!cap.answer||['sending','unknown','partial'].includes(card.status(context,local,cap)),reason:'Creates a linked held successor. The original stays closed. Read it back before separate Start.',tools:h('div',{class:'conversation-tools'},h('button',{'data-report':'reason-send',disabled:!cap.answer||!!card.textProblem(context.question,local.draft)||!['actionable','changed'].includes(card.status(context,local,cap))},'Send'),h('button',{'data-report':'reason-cancel',disabled:['sending','unknown','partial'].includes(card.status(context,local,cap))},'Cancel'))}),
    !ordinary&&r.tasks&&h('section',null,h('h3',null,'Proposed tasks'),r.tasks.length?r.tasks.map((t,i)=>h('details',{key:i,'data-view-key':namespace(r,'proposal'+i)},h('summary',null,(i+1)+'. '+t.title),h('p',null,t.goal),h('ul',null,t.acceptance?.map(a=>h('li',null,a))),t.scope?.map(s=>h('p',null,s)))):h('p',null,'No follow-up tasks proposed.'),h('p',{class:'meta'},'Make tasks creates held drafts linked to this report. Start them separately.'),r.pending_decision==='make'&&h('p',{role:'status'},'Make tasks is pending. Retry Make tasks to reconcile the original claims.'),!settled(r)&&slot('make')),
    !ordinary&&r.body&&h('details',{'data-view-key':namespace(r,'full')},h('summary',null,'Full report'),body(r.body)),
    notice&&h('p',{role:'status'},notice),
    (notice||successor||r.tasks_made?.length)&&![...pending.values()].some(op=>active(op)&&op.action!=='read')&&h('div',{class:'conversation-tools result-next'},
     successor&&h('a',{href:successor.url,'data-url':successor.url},'1. Read held draft #'+successor.n+' ★'),
     !!r.tasks_made?.length&&h('details',{'data-view-key':namespace(r,'drafts')},h('summary',null,'1. Read held drafts ★'),r.tasks_made.map(n=>h('a',{href:r.url?.replace(/\d+$/,String(n)),'data-url':r.url?.replace(/\d+$/,String(n)),key:n},'Read held draft #'+n))),
     h('button',{'data-report':'unresolved'},(successor||r.tasks_made?.length?'2. Next unresolved item':'1. Next unresolved item ★')),
     h('button',{'data-report':'stay'},(successor||r.tasks_made?.length?'3':'2')+'. Stay here')));
  }
 });return api;
},

// Keep untrusted Markdown as text/vnodes; evidence URLs are the only active content. No raw HTML.
body:function(text){
 const prose=text.replace(/```taskq-tasks\n[\s\S]*?```/g,'').replace(/(^|\n)#{1,6}[ \t]+Proposed tasks[ \t]*(?=\n|$)/gi,'$1').replace(/(^|\n)(#{1,6} [^\n]+)\n/g,'$1$2\n\n');
 return prose.split(/\n\s*\n/).map((p,i)=>{
  const heading=/^#{1,6}\s+(.+)$/.exec(p.trim());
  if(heading)return h('h3',{key:i},heading[1]);
  const parts=p.split(/(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\)|https?:\/\/[^\s<>\])]+)/g).map((s,k)=>{const link=/^\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)$/.exec(s),url=link?.[2]||(/^https?:\/\//.test(s)?s:null);return url?h('a',{key:k,href:url,'data-url':url},link?.[1]||url):s;});
  return h('p',{key:i,style:{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}},parts);
 });
}
});
module.exports={create};
