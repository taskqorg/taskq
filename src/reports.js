/*! PrismJS 1.30.0
MIT LICENSE

Copyright (c) 2012 Lea Verou

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/
'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
// Use only tokenizer grammars; never let the dependency rewrite the document or fetch files.
if(typeof window!=='undefined')window.Prism={manual:true,disableWorkerMessageHandler:true};
const Prism=require('prismjs/components/prism-core');
require('prismjs/components/prism-markup');require('prismjs/components/prism-css');require('prismjs/components/prism-clike');require('prismjs/components/prism-javascript');
require('prismjs/components/prism-rust');require('prismjs/components/prism-python');require('prismjs/components/prism-bash');require('prismjs/components/prism-json');require('prismjs/components/prism-typescript');require('prismjs/components/prism-diff');require('prismjs/components/prism-sql');require('prismjs/components/prism-yaml');
const syntaxCache=new Map();
const card=require('./answer-card.js'),composer=require('./pm-composer.js').composer,time=require('./relative-time.js'),icons=require('./ui-icons.js'),dashboard=require('./task-dashboard.js');

// §7: both surfaces use this view; the board owns read/decision state, memory holds only an in-flight view.
const {create,body}=execution.module('reports',{
create:function({send=ui.send,draw,defer=ui.defer,onOpen,onMove,navigation=()=>({index:0,length:1}),reading=()=>{},transcript,id=()=>Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')}){
 let projects=[],pages=new Map(),pending=new Map(),selected=null,notice='',draft=null,successor=null,readFailure='',readQueue=[],reasonField=null;
 const textPages=new Map(),bodies=new Map(),receipts=new Map(),outcomes=new Map(),key=(root,n)=>root+'#'+n;
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
  defer(()=>{if(pending.get(op.id)!==op)return;op.unknown=true;if(['read','open'].includes(op.action)){pending.delete(op.id);readFailure='Result read outcome unknown; waiting for service readback.';}op.resolve?.({state:'unknown',reason:'No bridge receipt received; check the same operation'});op.resolve=null;outcome(op,'Result outcome unknown; waiting for service readback.');api.readNext();draw();},action==='open'?30000:180000);
 }
 function cacheBody(r){const k=receiptKey(r.root,r.n,r.sha);bodies.delete(k);bodies.set(k,{root:r.root,n:r.n,sha:r.sha,body:r.body,verdict:r.verdict,tasks:r.tasks,lazy:false});let bytes=0;for(const [key,row]of [...bodies].reverse()){bytes+=new TextEncoder().encode(JSON.stringify(row)).length;if((bodies.size>2||bytes>2*1024*1024)&&!active(row))bodies.delete(key);}for(const [key,c]of controllers)if(controllers.size>16&&!active(c)&&![...pending.values()].some(op=>op.root===c.root&&op.n===c.n&&op.sha===c.sha))controllers.delete(key);}
 function readShown(r){const k=receiptKey(r.root,r.n,r.sha);if(!r.lazy&&(r.body||r.kind==='task')&&!r.read&&!displayedReads.has(k)){displayedReads.add(k);request(r.root,'read',{n:r.n,sha:r.sha});}}
 function summary(text,heading){
  const lines=(text||'').split('\n'),start=lines.findIndex(l=>l.replace(/^#+\s*/,'').trim().toLowerCase()===heading.toLowerCase());
  if(start<0)return '';
  const rest=lines.slice(start+1),end=rest.findIndex(l=>/^#{1,6}\s/.test(l));return rest.slice(0,end<0?undefined:end).join('\n').trim();
 }
 function textPage(r,source){const key=receiptKey(r.root,r.n,r.sha)+':'+source;let page=textPages.get(key);if(!page){const match=new RegExp('^report:'+source+'-(\\d+):').exec(navigation(r).position?.key||'');page={start:match?Number(match[1]):0,open:false};}textPages.delete(key);textPages.set(key,page);while(textPages.size>24)textPages.delete(textPages.keys().next().value);return page;}
 function textEnd(text,start){let end=Math.min(text.length,start+32768),count=0;for(const match of text.slice(start,end).matchAll(/\n\s*\n/g))if(++count===63){end=start+match.index+match[0].length;break;}if(end<text.length&&/[\uD800-\uDBFF]/.test(text[end-1]))end++;return end;}
 function textBody(r,text,source){
  const page=textPage(r,source),start=page.start,end=textEnd(text,start),key=source+(start?'-'+start:'');
  const turn=next=>{page.start=next;reading('report:'+source+(next?'-'+next:'')+':0',r);draw();};
  return [body(text.slice(start,end),key),(start>0||end<text.length)&&h('div',{class:'conversation-tools'},h('span',{class:'meta'},'Text '+(start+1)+'–'+end+' / '+text.length),start>0&&h('button',{onClick:()=>{let before=0;for(let at=0;at<start;){before=at;at=textEnd(text,at);}turn(before);}},'Previous text'),end<text.length&&h('button',{onClick:()=>turn(end)},'Next text'))];
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
  sync(ps,spare=16*1024*1024){
   projects=targets(ps);let retainedBytes=0,retainedRows=0;for(const root of pages.keys())if(!projects.some(p=>p.root===root))pages.delete(root);
   for(const p of projects){
    const data=p.reports;
    const incoming=[...(data?.reports||[]),...(data?.history||[])],retained=data?.complete===true?[]:(pages.get(p.root)?.rows||[]).filter(r=>!incoming.some(v=>v.n===r.n)&&(active(r)||++retainedRows<=192&&(retainedBytes+=new TextEncoder().encode(JSON.stringify(r)).length)<=spare/2));
    pages.set(p.root,{rows:[...incoming,...retained].map(r=>{const k=receiptKey(p.root,r.n,r.sha),won=receipts.get(k);if(Number.isSafeInteger(r.comments_through)&&Number.isSafeInteger(won?.comments_through)&&r.comments_through>=won.comments_through)receipts.delete(k);return receipts.has(k)?{...r,...won}:r;}),unknown:data?.complete!==true,reason:data?.reason||(data?.complete===true?'':'Reports not collected yet')});
   }
   if(selected){const row=api.rows(true).find(r=>key(r.root,r.n)===key(selected.root,selected.n)&&r.sha===selected.sha);selected=row?{...selected,...row,...bodies.get(receiptKey(row.root,row.n,row.sha))}:null;}
   for(const op of pending.values())if(op.unknown&&!op.checking&&!pages.get(op.root)?.unknown){const row=pages.get(op.root)?.rows.find(r=>r.n===op.n&&r.sha===op.sha);if(row&&(confirmed(op,{report:row})||settled(row)&&['accept','later','drop','make'].includes(op.action))){op.checking=true;defer(()=>api.receive({id:op.id,root:op.root,report:row}),0);}}
  },
  bounds(){return {bodySources:bodies.size,bodyBytes:[...bodies.values()].reduce((n,r)=>n+new TextEncoder().encode(JSON.stringify(r)).length,0),controllers:controllers.size,textPages:textPages.size};},
  refresh(){api.sync(projects);draw();},
  rows(history=false){return projects.flatMap(p=>(pages.get(p.root)?.rows||[]).filter(r=>history||r.visible!==false).map(r=>({...r,root:p.root}))).sort((a,b)=>(b.closed||0)-(a.closed||0)||b.n-a.n);},
  unread(){return api.rows().filter(r=>!r.read);},
  // R6 Decide: mirrors reports::decide; an undated or past Later returns, the oldest close first.
  undecided(r){const until=Date.parse(r.later_until||'');return r.kind!=='task'&&r.visible!==false&&!r.pending_decision&&(!r.decision||r.decision==='none'||r.decision==='later'&&!(until>Date.now()));},
  decide(rows=api.rows()){return rows.filter(api.undecided).sort((a,b)=>(a.closed||0)-(b.closed||0)||a.n-b.n);},
  count(){return projects.every(p=>pages.has(p.root)&&!pages.get(p.root).unknown)?api.unread().length:null;},
  sources(){return projects.map(p=>({root:p.root,name:p.name,loaded:api.rows(true).filter(r=>r.root===p.root).length,reason:pages.get(p.root)?.unknown?pages.get(p.root).reason:''}));},
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
  open(root,n,sha){return execution.action('reports.card.open',()=>{const r=api.rows(true).find(r=>r.root===root&&r.n===n&&(!sha||r.sha===sha));if(!r)return false;selected={...r,...bodies.get(receiptKey(root,n,r.sha))};if(r.lazy&&!selected.body)request(root,'open',{n,sha:r.sha});displayedReads.delete(receiptKey(root,n,r.sha));const prior=outcomes.get(receiptKey(root,n,r.sha));notice=prior?.notice||'';draft=null;successor=prior?.successor||api.savedSuccessor(r);onOpen?.(selected);draw();readShown(r);return true;})();},
  savedSuccessor(r){const n=Object.values(saved).find(v=>v.request?.root===r.root&&v.request?.n===r.n&&v.request?.sha===r.sha&&v.receipt?.state==='accepted'&&v.successor>0)?.successor;return n?{n,url:r.url?.replace(/\d+$/,String(n))}:null;},
  focus(root,n){const r=api.rows(true).find(r=>r.root===root&&r.n===n);if(!r||active(r))return;selected={...r,...bodies.get(receiptKey(root,n,r.sha))};if(r.lazy&&!selected.body)request(root,'open',{n,sha:r.sha});const prior=outcomes.get(receiptKey(root,n,r.sha));notice=prior?.notice||'';successor=prior?.successor||api.savedSuccessor(r);},
  move(step){onMove?.(step);},
  act(action,text,until){if(!selected)return;outcome(selected,'',null);request(selected.root,action,{n:selected.n,sha:selected.sha,...(text===undefined?{}:{text}),...(until===undefined?{}:{until})});draw();},
  receive(r){
   const op=pending.get(r.id);if(!op||op.root!==r.root)return false;const shown=active(op);
   const reply={state:r.reason?(r.receipt?.outcome==='rejected'?'rejected':r.receipts?.length?'partial':'unknown'):'accepted',reason:r.reason};
   if(r.report?.n!==op.n||r.report?.sha!==op.sha){reply.state='unknown';reply.reason=r.reason||'No matching result receipt';}
   else if(!r.reason&&!confirmed(op,r)){reply.state='rejected';reply.reason='The displayed action was not confirmed; review the current result decision.';}
   if(!['read','open'].includes(op.action)&&['unknown','partial'].includes(reply.state))op.unknown=true;else pending.delete(r.id);
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
    const p=pages.get(r.root),report=remember(op,r.report);if(op.action==='open'&&r.report.body!==undefined){const body={...r.report,root:r.root,lazy:false};cacheBody(body);if(shown)selected={...selected,...body};}
    if(p)p.rows=p.rows.map(row=>row.n!==op.n||row.sha!==op.sha?row:{...row,...report,closed:row.closed});
    if(selected&&selected.sha===op.sha&&key(selected.root,selected.n)===key(r.root,op.n))selected={...selected,...report,closed:selected.closed};
    if(op.action!=='read')outcome(op,report.visible===false?(report.decision==='made'?'Created held drafts: '+(report.tasks_made||[]).map(n=>'#'+n).join(', '):'Report dropped.'):(op.action==='later'?(op.until?'Later: back in Decide on '+op.until.slice(0,10)+'.':'Kept in Reports for later.'):''),undefined,shown);
    if(r.successor>0){outcome(op,'Created held draft. Read it back and start it separately.',{n:r.successor,url:r.successor_url||(typeof r.report.url==='string'?r.report.url.replace(/\d+$/,String(r.successor)):null)},shown);if(shown&&draft===op.action)draft=null;}
    if(op.action==='accept')outcome(op,'Result accepted.',undefined,shown);
   }else {if(op.action==='read')readFailure='Result read receipt unknown; waiting for service readback.';outcome(op,'Report receipt unknown; waiting for service readback.',undefined,shown);}
   api.readNext();draw();if(op.action==='open'&&shown)readShown(selected);return true;
  },
  click(b,ps){
   const action=b.dataset.report;if(!action)return false;
   if(action==='source-retry'&&selected){request(selected.root,'open',{n:selected.n,sha:selected.sha});draw();}
   else if(action==='open')api.open(b.dataset.root,Number(b.dataset.n));
   else if(action==='reason-send')api.reasonSend();
   else if(action==='date-send'&&selected){const k=namespace(selected,'decision');controllers.get(k)?.ui.act(k,selected.n,'submit');}
   else if(action==='date-cancel'&&selected){const k=namespace(selected,'decision');controllers.get(k)?.ui.act(k,selected.n,'cancel-result');}
   else if(action==='reason-cancel'&&selected){const k=namespace(selected,'correction');controllers.get(k)?.ui.act(k,selected.n,'cancel-result');}
   else if(action==='unresolved')onOpen?.({next:true});
   else if(action==='previous')api.move(-1);
   else if(action==='next')api.move(1);
   return true;
  },
  threads(){return api.rows(true).map(r=>({id:'task:'+(projects.find(p=>p.root===r.root)?.issues||r.root)+'/'+r.n,root:r.root,board:projects.find(p=>p.root===r.root)?.issues,conversation:r.conversation,task:r.n,report:r.n,title:r.title,result:r.result,updated:r.closed*1000,finished:true,needsDecision:api.undecided(r),status:({accepted:'Accepted',drop:'Dropped',made:'Drafts created',answered:'Decided'})[r.decision]||(r.kind==='task'?'Finished':'Report'),unread:!r.read,outcome:r.outcome,closed:r.closed,marker:api.undecided(r)?'Needs decision':!r.read?'New':''}));},
  hydrate(locals){for(const [k,v] of Object.entries(locals||{}))if(k.startsWith('result:')&&!saved[k]&&typeof v.draft==='string'){saved[k]={...v};if(v.op&&['sending','unknown','partial'].includes(v.receipt?.state)){saved[k].receipt={state:'unknown',reason:'Workspace reopened; check the original operation'};if(v.request?.id===v.op.id&&typeof v.request.root==='string'&&Number.isSafeInteger(v.request.n)&&typeof v.request.sha==='string')pending.set(v.op.id,{...v.request,unknown:true});}}},
  cardRow(r,kind){
   const choices=kind==='decision'?[['Accept','+ Acknowledge this result; - no new work.','accept'],['Later',r.kind==='task'?'+ Keep it for another decision; - revisit it yourself.':'+ Return to Decide on a date; - choose the date before Send.','later'],['Drop','+ Dismiss this decision; - no follow-up work.','drop']]:kind==='correction'?[['Reopen','+ Request correction; - creates a linked held successor.','reopen'],['Follow up','+ Add related work; - creates a separate held draft.','followup']]:[['Make tasks','+ Create the displayed held drafts; - read back before separate Start.','make'],['Keep report','+ Create nothing; - proposed work stays unfiled.','keep']];
   return {...r,state:'closed',revision:r.sha+':'+kind,resultDecision:true,repeatable:kind!=='decision'||!settled(r),question:{parsed:true,done:kind==='correction'?'The original task stays closed.':'Work is closed; reading does not accept it.',problem:kind==='decision'?'What should happen to this result?':kind==='correction'?'Correction or additional work?':'Create these held drafts?',resultReason:kind==='correction',resultDate:kind==='decision'&&r.kind!=='task',choices:choices.map(([label,detail,action],i)=>({id:i+1,label,text:label,detail,action,...(kind==='correction'||action==='later'&&r.kind!=='task'?{reason:action}:{}),local:action==='keep',recommended:i===(kind==='correction'?1:kind==='decision'&&r.outcome!=='success'?1:0)})),evidence:[]}};
  },
  capability(r,kind){
   const p=projects.find(p=>p.root===r.root),ops=[...pending.values(),...Object.values(saved).filter(v=>['sending','unknown','partial'].includes(v.receipt?.state)).map(v=>v.request).filter(Boolean)];
   const blocked=ops.some(op=>op.root===r.root&&op.n===r.n&&op.sha===r.sha&&op.action!=='read'&&op.id!==saved[namespace(r,kind)]?.op?.id);
   const reason=r.lazy&&!bodies.has(receiptKey(r.root,r.n,r.sha))?'Reading the exact result source':!p?'Registered project unavailable':p.alive===false?'Project observation stale':p.complete===false?'Project observation incomplete':pages.get(r.root)?.unknown?pages.get(r.root).reason:r.reason?'Result unavailable: '+r.reason:blocked?'Another result operation needs readback':kind==='decision'&&settled(r)?'Report already decided':kind==='make'&&!r.tasks?.length?'No follow-up tasks proposed':kind==='make'&&settled(r)?'Report already decided':r.pending_decision==='make'&&kind!=='make'?'Make tasks needs readback':'';
   return {answer:!reason,composer:true,reason};
  },
  mount(menu){
   if(!selected)return;
   for(const slot of menu.querySelectorAll('[data-result-card]')){
    const {body,verdict,...r}=selected,kind=slot.dataset.resultCard,k=namespace(r,kind),row=api.cardRow(r,kind);let c=controllers.get(k);
    if(!c){const el=menu.ownerDocument.createElement('div');c={root:r.root,n:r.n,sha:r.sha,el,board:k,ui:card.mount(el,{newId:id,capability:()=>api.capability(api.rows(true).find(row=>row.root===r.root&&row.n===r.n&&row.sha===r.sha)||r,kind),load:()=>saved[k],save:(_b,_r,local)=>{const prior=saved[k],same=prior?.op?.id===local.op?.id;saved[k]={...local,...(same&&prior?.request?{request:prior.request}:{}),...(same&&prior?.successor?{successor:prior.successor}:{})};send({a:'answer-local',key:k,local:saved[k]});if(local.intent!==draft){draft=local.intent;draw();}},send:(_board,op)=>{
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
   const field=menu.querySelector('[data-c-field=result-reason]');if(field&&field!==reasonField){reasonField=field;const caret=saved[namespace(selected,'correction')]?.caret;if(Array.isArray(caret))field.setSelectionRange(...caret);}
   readShown(selected);
  },
  reasonInput(text,caret){if(!selected)return;const k=namespace(selected,'correction'),c=controllers.get(k);if(text===null&&JSON.stringify(saved[k]?.caret)===JSON.stringify(caret))return;if(c)c.ui.input(k,selected.n,text,caret);draw();},
  reasonSend(){if(selected){const k=namespace(selected,'correction');controllers.get(k)?.ui.act(k,selected.n,'submit');}},
  dateInput(text){if(selected){const k=namespace(selected,'decision');controllers.get(k)?.ui.input(k,selected.n,text);draw();}},
  section(ps){
   api.sync(ps);const r=selected;if(!r)return h('p',null,api.count()===null?api.reason()||'Results unknown; waiting for collection.':'No results waiting for review.');
   if(r.lazy&&!bodies.has(receiptKey(r.root,r.n,r.sha)))return h('article',{class:'report-card'},h('p',{role:'status'},notice||'Reading the exact result source…'),h('button',{'data-report':'source-retry',disabled:[...pending.values()].some(active)},'Retry source read'));
   const proposals=textPage(r,'proposals');const ordinary=r.kind==='task',nav=navigation(r),k=namespace(r,'correction'),local=saved[k]||card.fresh(api.cardRow(r,'correction')),context=api.cardRow(r,'correction'),cap=api.capability(r,'correction');
   const decision=api.cardRow(r,'decision'),date=saved[namespace(r,'decision')]||card.fresh(decision),dateCap=api.capability(r,'decision'),dateLive=['actionable','changed'].includes(card.status(decision,date,dateCap));
   const [mark,tone,outcome]=dashboard.outcome(r);
   const slot=kind=>h('div',{'data-result-card':kind,key:namespace(r,kind)});
   return h('article',{class:'report-card','aria-label':ordinary?'Completed result':'Research report'},
    h('div',{class:'conversation-tools'},h('span',{class:'meta'},(ordinary?'Finished':'Report')+' · '+(r.read?'Read':'New')+(!ordinary?' · '+(({accepted:'Accepted',drop:'Dropped',made:'Drafts created',answered:'Decided',later:'Later'})[r.decision]||'Decision open'):'')),
     h('a',{href:r.url,'data-url':r.url},'#'+r.n),h('button',{'data-report':'previous',disabled:nav.index<=0},'Previous'),h('button',{'data-report':'next',disabled:nav.index>=nav.length-1},'Next')),
    ordinary&&h('p',{class:'meta'},h('span',{class:tone},icons.icon(mark),outcome),r.merged&&h('span',null,' · '+r.merged),time.stamp(time.seconds(r.closed),'Task closed')),
    r.reason&&h('p',{role:'status'},'Result unknown: '+r.reason),
    !ordinary&&h('p',{class:'meta'},'Verdict: '+(r.experiment_verdict||'unknown'),time.stamp(time.seconds(r.closed),'Report closed')),
    ordinary?[...['Changed','Checks','Open'].map(label=>h('section',{key:label},h('h3',null,label),textBody(r,field(r.body,label),label.toLowerCase()))),r.pr&&h('a',{href:r.pr,'data-url':r.pr},'Open PR'),r.verdict&&h('details',{'data-view-key':namespace(r,'verdict')},h('summary',null,'Review and delivery'),textBody(r,r.verdict,'verdict'))]:
     r.body?[h('h3',null,'Question'),textBody(r,summary(r.body,'Question'),'question'),h('h3',null,'Recommendation'),textBody(r,summary(r.body,'Recommendation'),'recommendation')]:h('p',null,'Report content unknown; waiting for background collection.'),
    (!ordinary||saved[namespace(r,'decision')]?.op&&['sending','unknown','partial'].includes(saved[namespace(r,'decision')]?.receipt?.state))&&(settled(r)?h('p',{class:'meta'},'Decision recorded: '+r.decision+'. Retained in Finished.'):h('section',null,slot('decision'),date.intent==='later'&&h('div',{class:'result-date'},
     h('label',null,'Revisit on',h('input',{type:'date','data-c-field':'result-date','aria-label':'Revisit date',value:date.draft,min:new Date(Date.now()+86400000).toISOString().slice(0,10),disabled:!dateLive})),
     h('p',{class:'meta'},'The report returns to Decide on this date.'),
     card.textProblem(decision.question,date.draft)&&h('p',{role:'status'},card.textProblem(decision.question,date.draft)),
     h('div',{class:'conversation-tools'},h('button',{'data-report':'date-send',disabled:!dateLive||!!card.textProblem(decision.question,date.draft)},'Send'),h('button',{'data-report':'date-cancel',disabled:!dateLive},'Cancel'))))),
    h('details',{class:'result-other',open:!!local.intent,'data-view-key':namespace(r,'other')},h('summary',null,'Other actions'),slot('correction')),
    local.intent&&composer({id:'result-reason',field:'result-reason',label:(local.intent==='reopen'?'Reopen':'Follow-up')+' reason',placeholder:'What needs to change?',text:local.draft,disabled:!cap.answer||['sending','unknown','partial'].includes(card.status(context,local,cap)),reason:'Creates a linked held successor. The original stays closed. Read it back before separate Start.',tools:h('div',{class:'conversation-tools'},h('button',{'data-report':'reason-send',disabled:!cap.answer||!!card.textProblem(context.question,local.draft)||!['actionable','changed'].includes(card.status(context,local,cap))},'Send'),h('button',{'data-report':'reason-cancel',disabled:['sending','unknown','partial'].includes(card.status(context,local,cap))},'Cancel'))}),
    !ordinary&&Array.isArray(r.tasks)&&h('section',null,h('h3',null,'Proposed tasks'),r.tasks.length?[h('p',{class:'meta','data-reading-key':'report:proposals-'+proposals.start+':0'},'Tasks '+(proposals.start+1)+'–'+Math.min(r.tasks.length,proposals.start+64)+' / '+r.tasks.length),r.tasks.slice(proposals.start,proposals.start+64).map((t,i)=>{const index=proposals.start+i,page=textPage(r,'proposal'+index);return h('details',{key:index,open:page.open,'data-view-key':namespace(r,'proposal'+index)},h('summary',{onClick:e=>{e.preventDefault();page.open=!page.open;draw();}},(index+1)+'. '+t.title),page.open&&textBody(r,[t.goal,...(t.acceptance||[]).map(a=>'- '+a),...(t.scope||[])].join('\n\n'),'proposal'+index));}),h('div',{class:'conversation-tools'},proposals.start>0&&h('button',{onClick:()=>{proposals.start=Math.max(0,proposals.start-64);reading('report:proposals-'+proposals.start+':0',r);draw();}},'Previous proposals'),proposals.start+64<r.tasks.length&&h('button',{onClick:()=>{proposals.start+=64;reading('report:proposals-'+proposals.start+':0',r);draw();}},'Next proposals'))]:h('p',null,'No follow-up tasks proposed.'),r.tasks.length>0&&h('p',{class:'meta'},'Make tasks creates held drafts linked to this report. Start them separately.'),r.pending_decision==='make'&&h('p',{role:'status'},'Make tasks is pending. Retry Make tasks to reconcile the original claims.'),(r.tasks.length>0||r.pending_decision==='make')&&!settled(r)&&slot('make')),
    !ordinary&&r.body&&h('details',{'data-view-key':namespace(r,'full'),open:textPage(r,'full').open,onToggle:e=>{const page=textPage(r,'full');if(page.open!==e.currentTarget.open){page.open=e.currentTarget.open;draw();}}},h('summary',{'data-reading-key':'report:full',onClick:e=>{e.preventDefault();const page=textPage(r,'full');page.open=!page.open;e.currentTarget.parentElement.open=page.open;draw();}},'Full report'),textPage(r,'full').open&&textBody(r,r.body,'full')),
    transcript?.(r),notice&&h('p',{role:'status'},notice),
    !!(notice||successor||r.tasks_made?.length)&&![...pending.values()].some(op=>active(op)&&op.action!=='read')&&h('div',{class:'conversation-tools result-next'},
     successor&&h('a',{href:successor.url,'data-url':successor.url},'1. Read held draft #'+successor.n+' ★'),
     !!r.tasks_made?.length&&h('details',{'data-view-key':namespace(r,'drafts')},h('summary',null,'1. Read held drafts ★'),r.tasks_made.map(n=>h('a',{href:r.url?.replace(/\d+$/,String(n)),'data-url':r.url?.replace(/\d+$/,String(n)),key:n},'Read held draft #'+n))),
     h('button',{'data-report':'unresolved'},(successor||r.tasks_made?.length?'2. Next unresolved item':'1. Next unresolved item ★')),
     h('button',{'data-report':'stay'},(successor||r.tasks_made?.length?'3':'2')+'. Stay here')));
  }
 });return api;
},

// Keep untrusted Markdown as text/vnodes; evidence URLs are the only active content. No raw HTML.
body:function(text,source='body',nativeAt){
 if(nativeAt===undefined)text=text.replace(/```taskq-tasks\n[\s\S]*?```/g,'').replace(/(^|\n)#{1,6}[ \t]+Proposed tasks[ \t]*(?=\n|$)/gi,'$1');
 const lines=text.split('\n'),starts=[];let offset=0;for(const line of lines){starts.push(offset);offset+=line.length+1;}
 const blocks=[],props=at=>nativeAt===undefined?(source===null?{}:{'data-reading-key':'report:'+source+':'+blocks.length}):{'data-native-part':nativeAt+at,...(source?{'data-reading-key':'report:'+source+':part:'+(nativeAt+at)}:{})};
 const plain=(text,at)=>{const nodes=[];for(let n=0;n<text.length;){let end=Math.min(text.length,n+1024);if(end<text.length&&/[\uD800-\uDBFF]/.test(text[end-1]))end++;nodes.push(nativeAt===undefined?text.slice(n,end):h('span',{key:at+n,...props(at+n)},text.slice(n,end)));n=end;}return nodes;};
 const inline=(text,at)=>{const nodes=[];let end=0;for(const m of text.matchAll(/`(?<code>[^`\n]+)`|\*\*(?<strong>[^*\n]+)\*\*|\*(?<em>[^*\n]+)\*|_(?<italic>[^_\n]+)_|\[(?<label>[^\]\n]+)\]\((?<url>https?:\/\/[^\s)]+)\)|https?:\/\/[^\s<>\]\)]+/g)){
  nodes.push(...plain(text.slice(end,m.index),at+end));const g=m.groups,value=g.code??g.strong??g.em??g.italic??g.label??m[0],tag=g.code!==undefined?'code':g.strong!==undefined?'strong':g.em!==undefined||g.italic!==undefined?'em':'a';
  nodes.push(h(tag,{key:at+m.index,...(tag==='a'?{href:g.url||m[0],'data-url':g.url||m[0]}:{})},plain(value,at+m.index+(g.strong!==undefined?2:tag==='a'&&!g.label?0:1))));end=m.index+m[0].length;
 }return [...nodes,...plain(text.slice(end),at+end)];};
 const code=(text,language,at)=>{
  const name=language.toLowerCase().split(/\s/)[0],grammar=Object.hasOwn(Prism.languages,name)&&Prism.languages[name];
  // shortcut: synchronous grammar work is capped at 8 KiB; use a worker if measured reader latency requires it.
  if(!grammar||text.length>8192)return plain(text,at);
  const key=name+'\n'+text;let tokens=syntaxCache.get(key);
  try {if(!tokens){tokens=Prism.tokenize(text,grammar);syntaxCache.set(key,tokens);while(syntaxCache.size>16)syntaxCache.delete(syntaxCache.keys().next().value);}}catch{return plain(text,at);}
  let cursor=at;const render=token=>{if(typeof token==='string'){const nodes=plain(token,cursor);cursor+=token.length;return nodes;}if(Array.isArray(token))return token.map(render);return h('span',{class:'syntax-'+token.type},render(token.content));};
  return render(tokens);
 };
 const list=line=>/^\s*(?:([-*+])|(\d+)[.)])\s+(.+)$/.exec(line);
 for(let i=0;i<lines.length;){
  if(!lines[i].trim()){i++;continue;}const start=i,at=starts[i],fence=/^\s*(`{3,}|~{3,})([^\n]*)$/.exec(lines[i]);let node;
  if(lines[i].trim()==='<oai-mem-citation>'&&lines.slice(i+1).some(l=>l.trim()==='</oai-mem-citation>')){i++;while(i<lines.length&&lines[i].trim()!=='</oai-mem-citation>')i++;i++;node=h('details',{'data-view-key':'metadata:'+source+':'+at},h('summary',{class:'source-toggle'},'Provider citation metadata'),h('pre',null,plain(lines.slice(start,i).join('\n'),at)));}
  else if(fence){i++;const begin=i;while(i<lines.length&&!lines[i].trim().startsWith(fence[1]))i++;node=h('pre',null,h('code',{'aria-label':(fence[2].trim()||'Text')+' code'},code(lines.slice(begin,i).join('\n'),fence[2].trim(),starts[begin]??text.length)));if(i<lines.length)i++;}
  else if(/^#{1,6}\s/.test(lines[i])){const heading=/^#{1,6}\s+/.exec(lines[i])[0];node=h('h3',null,inline(lines[i].slice(heading.length),at+heading.length));i++;}
  else if(list(lines[i])){const ordered=!!list(lines[i])[2],items=[];while(i<lines.length){const m=list(lines[i]);if(!m||!!m[2]!==ordered)break;items.push(h('li',{key:i,...(ordered?{value:Number(m[2])}:{}),...props(starts[i])},inline(m[3],starts[i]+lines[i].indexOf(m[3]))));i++;}node=h(ordered?'ol':'ul',null,items);}
  else {i++;while(i<lines.length&&lines[i].trim()&&!/^\s*(`{3,}|~{3,})|^#{1,6}\s/.test(lines[i])&&!list(lines[i])&&lines[i].trim()!=='<oai-mem-citation>')i++;node=h('p',null,inline(lines.slice(start,i).join('\n'),at));}
  Object.assign(node.props,{...props(at),style:{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}});node.key=at;blocks.push(node);
 }
 return blocks;

}
});
module.exports={create,body};
