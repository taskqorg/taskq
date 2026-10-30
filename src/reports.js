'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;

// §7: both surfaces use this view; the board owns read/decision state, memory holds only an in-flight view.
const {create,body}=execution.module('reports',{
create:function({send=ui.send,draw,defer=ui.defer,id=()=>Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')}){
 let expanded=false,projects=[],pages=new Map(),pending=new Map(),selected=null,notice='',full=false,draft=null,successor=null;
 const receipts=new Map(),outcomes=new Map(),key=(root,n)=>root+'#'+n;
 const receiptKey=(root,n,sha)=>key(root,n)+'@'+sha;
 const active=op=>selected?.root===op.root&&selected.n===op.n&&selected.sha===op.sha;
 function outcome(op,text,next,shown=active(op)){const k=receiptKey(op.root,op.n,op.sha),prior=outcomes.get(k)||{},value={notice:text===undefined?prior.notice||'':text,successor:next===undefined?prior.successor||null:next};outcomes.set(k,value);if(shown){notice=value.notice;successor=value.successor;}}
 function targets(ps){const seen=new Set();return ps.filter(p=>{const board=p.issues||p.root;if(seen.has(board))return false;seen.add(board);return true;});}
 function request(root,action,extra={}){
  if([...pending.values()].some(p=>p.root===root&&p.n===extra.n&&p.sha===extra.sha&&(action==='read'||p.action!=='read')))return;
  const op={id:id(),root,action,...extra};pending.set(op.id,op);send({a:'reports',...op});
  defer(()=>{if(pending.delete(op.id)){outcome(op,'Result outcome unknown; waiting for service readback.');draw();}},180000);
 }
 function summary(text,heading){
  const lines=(text||'').split('\n'),start=lines.findIndex(l=>l.replace(/^#+\s*/,'').trim().toLowerCase()===heading.toLowerCase());
  if(start<0)return '';
  const rest=lines.slice(start+1),end=rest.findIndex(l=>/^#{1,6}\s/.test(l));return rest.slice(0,end<0?undefined:end).join('\n').trim();
 }
 function field(text,label){const lines=(text||'').split('\n'),start=lines.findIndex(l=>l.startsWith(label+':'));if(start<0)return 'Unknown: not recorded in the accepted hand-in.';const rest=lines.slice(start+1),end=rest.findIndex(l=>/^(Changed|Checks|Open|Report):/.test(l)&&!l.startsWith(label+':'));return [lines[start],...rest.slice(0,end<0?undefined:end)].map(l=>l.startsWith(label+':')?l.slice(label.length+1).trim():l).join('\n');}
 function remember(op,report){
  const k=receiptKey(op.root,op.n,op.sha),won=receipts.get(k);
  if(won?.sha===op.sha&&won.visible===false)return won;
  // Receipts prove marker state, never the close time or immutable source. Late read replies cannot undo Later/Make.
  const fields={sha:report.sha};
  for(const field of ['read','decision','visible','pending_decision','tasks_made','comments_through'])if(Object.hasOwn(report,field))fields[field]=report[field];
  if(op.action==='read'&&won?.sha===op.sha){if(['later','accepted'].includes(won.decision))fields.decision=won.decision;if(won.pending_decision==='make')fields.pending_decision='make';}
  if(Number.isSafeInteger(won?.comments_through)&&Number.isSafeInteger(fields.comments_through))fields.comments_through=Math.max(won.comments_through,fields.comments_through);
  const merged={...(won?.sha===op.sha?won:{}),...fields};receipts.set(k,merged);return merged;
 }
 const api=execution.module('reports.view',{
  sync(ps){
   projects=targets(ps);
   for(const p of projects){
    const data=p.reports;
    pages.set(p.root,{rows:(data?.reports||[]).flatMap(r=>{const k=receiptKey(p.root,r.n,r.sha),won=receipts.get(k);if(Number.isSafeInteger(r.comments_through)&&Number.isSafeInteger(won?.comments_through)&&r.comments_through>=won.comments_through)receipts.delete(k);const row=receipts.has(k)?{...r,...won}:r;return row.visible===false?[]:[row];}),unknown:data?.complete!==true,reason:data?.reason||(data?.complete===true?'':'Reports not collected yet')});
   }
   if(selected){const row=api.rows().find(r=>key(r.root,r.n)===key(selected.root,selected.n)&&r.sha===selected.sha);selected=row?{...selected,...row}:null;}
  },
  toggle(ps){return execution.action('reports.list.open',()=>{api.sync(ps);expanded=!expanded;draw();})();},
  refresh(){api.sync(projects);draw();},
  rows(){return projects.flatMap(p=>(pages.get(p.root)?.rows||[]).map(r=>({...r,root:p.root}))).sort((a,b)=>(b.closed||0)-(a.closed||0)||b.n-a.n);},
  unread(){return api.rows().filter(r=>!r.read);},
  count(){return projects.every(p=>pages.has(p.root)&&!pages.get(p.root).unknown)?api.unread().length:null;},
  current(){return selected;},
  open(root,n){return execution.action('reports.card.open',()=>{const r=api.rows().find(r=>r.root===root&&r.n===n);if(!r)return;selected={...r};expanded=true;full=false;const prior=outcomes.get(receiptKey(root,n,r.sha));notice=prior?.notice||'';draft=null;successor=prior?.successor||null;draw();if((r.body||r.kind==='task')&&!r.read)request(root,'read',{n,sha:r.sha});})();},
  move(step){const rows=api.rows(),i=rows.findIndex(r=>r.root===selected?.root&&r.n===selected?.n),r=rows[i+step];if(r)api.open(r.root,r.n);},
  act(action,text){if(!selected)return;outcome(selected,'',null);request(selected.root,action,{n:selected.n,sha:selected.sha,...(text===undefined?{}:{text})});draw();},
  receive(r){
   const op=pending.get(r.id);if(!op||op.root!==r.root)return false;pending.delete(r.id);const shown=active(op);
   if(r.reason){
    if(r.report?.n===op.n&&r.report.sha===op.sha)remember(op,r.report);
    const made=Array.isArray(r.receipts)?r.receipts:[],uncertain=r.receipt?.task;
    outcome(op,r.reason+(made.length?' · Created held drafts: '+made.map(n=>'#'+n).join(', '):'')+(uncertain>0&&!made.includes(uncertain)?' · Unverified draft: #'+uncertain:''),undefined,shown);
    draw();return true;
   }
   if(r.report?.n===op.n&&r.report.sha===op.sha){
    const p=pages.get(r.root),report=remember(op,r.report);
    if(p)p.rows=p.rows.flatMap(row=>row.n!==op.n||row.sha!==op.sha?[row]:report.visible===false?[]:[{...row,...report,closed:row.closed}]);
    if(selected&&selected.sha===op.sha&&key(selected.root,selected.n)===key(r.root,op.n))selected=report.visible===false?null:{...selected,...report,closed:selected.closed};
    if(op.action!=='read')outcome(op,report.visible===false?(report.decision==='made'?'Created held drafts: '+(report.tasks_made||[]).map(n=>'#'+n).join(', '):'Report dropped.'):(op.action==='later'?'Kept in Reports for later.':''),undefined,shown);
    if(r.successor>0){outcome(op,'Created held draft. Read it back and start it separately.',{n:r.successor,url:r.successor_url||(typeof r.report.url==='string'?r.report.url.replace(/\d+$/,String(r.successor)):null)},shown);if(shown&&draft===op.action)draft=null;}
    if(op.action==='accept')outcome(op,'Result accepted.',undefined,shown);
   }else outcome(op,'Report receipt unknown; waiting for service readback.',undefined,shown);
   draw();return true;
  },
  click(b,ps){
   const action=b.dataset.report;if(!action)return false;
   if(action==='toggle')api.toggle(ps);
   else if(action==='refresh')api.refresh();
   else if(action==='open')api.open(b.dataset.root,Number(b.dataset.n));
   else if(action==='full'){full=!full;draw();}
   else if(action==='back'){selected=null;draw();}
   else if(action==='previous')api.move(-1);
   else if(action==='next')api.move(1);
   else if(['followup','reopen'].includes(action)){draft=action;draw();}
   else if(action==='create'){const text=b.closest('.report-card')?.querySelector('[data-result-reason]')?.value;api.act(draft,text);}
   else if(['accept','make','later','drop'].includes(action))api.act(action);
   return true;
  },
  attention(){return api.unread().map(r=>h('button',{class:'project report-row','data-report':'open','data-root':r.root,'data-n':r.n,key:key(r.root,r.n)},h('span',{class:'meta'},(r.kind==='task'?'Finished':'Report')+' · '+(projects.find(p=>p.root===r.root)?.name||'Project')+' · #'+r.n+(r.kind==='task'?' · New result':' · Needs decision')),h('span',null,r.title)));},
  threads(){return api.rows().map(r=>({id:'task:'+(projects.find(p=>p.root===r.root)?.issues||r.root)+'/'+r.n,root:r.root,report:r.n,title:r.title,result:r.result,updated:r.closed*1000,status:r.kind==='task'?'Finished':'Report',marker:r.read?'':r.kind==='task'?'New result':'Needs decision'}));},
  section(ps){
   api.sync(ps);const loaded=api.count()!==null,rows=api.rows(),busy=selected&&[...pending.values()].some(p=>p.root===selected.root&&p.n===selected.n&&p.action!=='read'),project=root=>projects.find(p=>p.root===root)?.name||'Project unavailable';
   const ordinary=selected?.kind==='task',position=rows.findIndex(r=>r.root===selected?.root&&r.n===selected?.n);
   return h('section',{class:'reports-section group','aria-label':'Results',key:'reports'},
    h('button',{class:'project',type:'button','data-report':'toggle','aria-expanded':String(expanded)},'Results · '+(loaded?rows.length:'unknown'),api.unread().length?' · '+api.unread().length+' new':''),
    expanded&&h('div',{class:'reports-content body'},
     selected&&h('div',{class:'options'},h('button',{'data-report':'back'},'All results'),h('button',{'data-report':'previous',disabled:position<=0},'Previous'),h('button',{'data-report':'next',disabled:position<0||position>=rows.length-1},'Next')),
     notice&&h('p',{role:'status'},notice),
     successor&&h(successor.url?'a':'span',{href:successor.url,'data-url':successor.url},'Read held draft #'+successor.n),
     projects.map(p=>pages.get(p.root)?.reason&&h('p',{role:'status',key:p.root},p.name+': '+pages.get(p.root).reason)),
     selected?h('article',{class:'report-card','aria-label':ordinary?'Completed result':'Research report'},h('h2',null,selected.title),h('p',{class:'meta'},project(selected.root)+' · ',h('a',{href:selected.url,'data-url':selected.url},'#'+selected.n),' · '+(selected.read?'Read':'Unread')),
      selected.reason&&h('p',{role:'status'},'Result unknown: '+selected.reason),
      ordinary?[...['Changed','Checks','Open'].map(label=>h('div',{key:label},h('h3',null,label),body(field(selected.body,label)))),
       selected.pr&&h('a',{href:selected.pr,'data-url':selected.pr},'Open PR'),
       selected.verdict&&h('details',null,h('summary',null,'Review and delivery'),body(selected.verdict)),
       h('div',{class:'options'},['accept','followup','reopen'].map(action=>h('button',{'data-report':action,disabled:!!busy||action==='accept'&&selected.decision==='accepted'},{accept:selected.decision==='accepted'?'Accepted':'Accept',followup:'Follow-up task',reopen:'Reopen'}[action]))),
       draft&&h('div',{key:draft},h('label',null,(draft==='reopen'?'Reopen':'Follow-up')+' reason',h('input',{'data-result-reason':'',maxLength:180,required:true,'aria-label':'Reason',placeholder:'What needs to change?'})),h('p',{class:'meta'},'Creates a linked held successor. The original stays closed. Read the draft back before starting it.'),h('button',{'data-report':'create',disabled:!!busy},'Create held draft'))]:[
      selected.body?[h('h3',null,'Question'),body(summary(selected.body,'Question')),h('h3',null,'Recommendation'),body(summary(selected.body,'Recommendation'))]:h('p',null,'Report content unknown; waiting for background collection.'),
      selected.tasks&&h('div',null,h('h3',null,'Proposed tasks'),selected.tasks.length?selected.tasks.map((t,i)=>h('details',{key:i},h('summary',null,(i+1)+'. '+t.title),h('p',null,t.goal),h('ul',null,t.acceptance?.map(a=>h('li',null,a))),t.scope?.map(s=>h('p',null,s)),h('p',{class:'meta'},[t.type,t.runtime,t.priority&&'Priority '+t.priority].filter(Boolean).join(' · ')))):h('p',null,'No follow-up tasks proposed.')),
      h('p',{class:'meta'},'Make tasks creates held drafts linked to this report. Start them separately.'),
      selected.pending_decision==='make'&&h('p',{role:'status'},'Make tasks is pending. Retry to reconcile the held drafts.'),
      h('div',{class:'options'},['make','later','drop'].map(action=>h('button',{'data-report':action,disabled:!!busy||!selected.body||action==='make'&&!selected.tasks?.length||action==='drop'&&selected.pending_decision==='make'},{make:selected.pending_decision==='make'?'Retry Make tasks':'Make tasks',later:'Later',drop:'Drop'}[action]))),
      selected.body&&h('div',null,h('button',{'data-report':'full','aria-expanded':String(full)},full?'Hide full report':'Full report'),full&&h('div',{class:'report-body'},body(selected.body))) ]):
     [rows.length?rows.map(r=>h('button',{class:'project report-row','data-report':'open','data-root':r.root,'data-n':r.n,key:key(r.root,r.n)},h('span',{class:'meta'},project(r.root)+' · #'+r.n+' · '+(r.read?'Read':'Unread')),h('span',null,r.title))):loaded&&h('p',null,'No results waiting for review.')]));
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
