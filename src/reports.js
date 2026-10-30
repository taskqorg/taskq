'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;

// §7: both surfaces use this view; the board owns read/decision state, memory holds only an in-flight view.
const {create,body}=execution.module('reports',{
create:function({send=ui.send,draw,defer=ui.defer,id=()=>Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')}){
 let expanded=false,projects=[],pages=new Map(),pending=new Map(),selected=null,notice='',full=false;
 const receipts=new Map(),key=(root,n)=>root+'#'+n;
 const receiptKey=(root,n,sha)=>key(root,n)+'@'+sha;
 function targets(ps){const seen=new Set();return ps.filter(p=>{const board=p.issues||p.root;if(seen.has(board))return false;seen.add(board);return true;});}
 function request(root,action,extra={}){
  if([...pending.values()].some(p=>p.root===root&&p.n===extra.n&&p.sha===extra.sha&&(action==='read'||p.action!=='read')))return;
  const op={id:id(),root,action,...extra};pending.set(op.id,op);send({a:'reports',...op});
  defer(()=>{if(pending.delete(op.id)){notice='Report outcome unknown; waiting for service readback.';draw();}},180000);
 }
 function summary(text,heading){
  const lines=(text||'').split('\n'),start=lines.findIndex(l=>l.replace(/^#+\s*/,'').trim().toLowerCase()===heading.toLowerCase());
  if(start<0)return '';
  const rest=lines.slice(start+1),end=rest.findIndex(l=>/^#{1,6}\s/.test(l));return rest.slice(0,end<0?undefined:end).join('\n').trim();
 }
 function remember(op,report){
  const k=receiptKey(op.root,op.n,op.sha),won=receipts.get(k);
  if(won?.sha===op.sha&&won.visible===false)return won;
  // Receipts prove marker state, never the close time or immutable source. Late read replies cannot undo Later/Make.
  const fields={sha:report.sha};
  for(const field of ['read','decision','visible','pending_decision','tasks_made','comments_through'])if(Object.hasOwn(report,field))fields[field]=report[field];
  if(op.action==='read'&&won?.sha===op.sha){if(won.decision==='later')fields.decision='later';if(won.pending_decision==='make')fields.pending_decision='make';}
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
  toggle(ps){api.sync(ps);expanded=!expanded;draw();},
  refresh(){api.sync(projects);draw();},
  rows(){return projects.flatMap(p=>(pages.get(p.root)?.rows||[]).map(r=>({...r,root:p.root}))).sort((a,b)=>(b.closed||0)-(a.closed||0)||b.n-a.n);},
  unread(){return api.rows().filter(r=>!r.read);},
  count(){return projects.every(p=>pages.has(p.root)&&!pages.get(p.root).unknown)?api.unread().length:null;},
  current(){return selected;},
  open(root,n){const r=api.rows().find(r=>r.root===root&&r.n===n);if(!r)return;selected={...r};expanded=true;full=false;notice='';draw();if(r.body&&!r.read)request(root,'read',{n,sha:r.sha});},
  act(action){if(!selected)return;notice='';request(selected.root,action,{n:selected.n,sha:selected.sha});draw();},
  receive(r){
   const op=pending.get(r.id);if(!op||op.root!==r.root)return false;pending.delete(r.id);
   if(r.reason){
    if(r.report?.n===op.n&&r.report.sha===op.sha)remember(op,r.report);
    const made=Array.isArray(r.receipts)?r.receipts:[],uncertain=r.receipt?.task;
    notice=r.reason+(made.length?' ? Created held drafts: '+made.map(n=>'#'+n).join(', '):'')+(uncertain>0&&!made.includes(uncertain)?' ? Unverified draft: #'+uncertain:'');
    draw();return true;
   }
   if(r.report?.n===op.n&&r.report.sha===op.sha){
    const p=pages.get(r.root),report=remember(op,r.report);
    if(p)p.rows=p.rows.flatMap(row=>row.n!==op.n||row.sha!==op.sha?[row]:report.visible===false?[]:[{...row,...report,closed:row.closed}]);
    if(selected&&selected.sha===op.sha&&key(selected.root,selected.n)===key(r.root,op.n))selected=report.visible===false?null:{...selected,...report,closed:selected.closed};
    notice=report.visible===false?(report.decision==='made'?'Created held drafts: '+(report.tasks_made||[]).map(n=>'#'+n).join(', '):'Report dropped.'):(op.action==='later'?'Kept in Reports for later.':'');
   }else notice='Report receipt unknown; waiting for service readback.';
   draw();return true;
  },
  click(b,ps){
   const action=b.dataset.report;if(!action)return false;
   if(action==='toggle')api.toggle(ps);
   else if(action==='refresh')api.refresh();
   else if(action==='open')api.open(b.dataset.root,Number(b.dataset.n));
   else if(action==='full'){full=!full;draw();}
   else if(action==='back'){selected=null;draw();}
   else if(['make','later','drop'].includes(action))api.act(action);
   return true;
  },
  attention(){return api.unread().map(r=>h('button',{class:'project report-row','data-report':'open','data-root':r.root,'data-n':r.n,key:key(r.root,r.n)},h('span',{class:'meta'},'Report ? '+(projects.find(p=>p.root===r.root)?.name||'Project')+' ? #'+r.n+' ? Needs decision'),h('span',null,r.title)));},
  threads(){return api.rows().map(r=>({id:'report:'+key(r.root,r.n),root:r.root,report:r.n,title:r.title,updated:r.closed*1000,status:'Report',marker:r.read?'':'Needs decision'}));},
  section(ps){
   api.sync(ps);const loaded=api.count()!==null,rows=api.rows(),busy=selected&&[...pending.values()].some(p=>p.root===selected.root&&p.n===selected.n&&p.action!=='read'),project=root=>projects.find(p=>p.root===root)?.name||'Project unavailable';
   return h('section',{class:'reports-section group','aria-label':'Reports',key:'reports'},
    h('button',{class:'project',type:'button','data-report':'toggle','aria-expanded':String(expanded)},'Reports ? '+(loaded?rows.length:'unknown'),api.unread().length?' ? '+api.unread().length+' new':''),
    expanded&&h('div',{class:'reports-content body'},
     selected&&h('button',{'data-report':'back'},'All reports'),
     notice&&h('p',{role:'status'},notice),
     projects.map(p=>pages.get(p.root)?.reason&&h('p',{role:'status',key:p.root},p.name+': '+pages.get(p.root).reason)),
     selected?h('article',{class:'report-card','aria-label':'Research report'},h('h2',null,selected.title),h('p',{class:'meta'},project(selected.root)+' ? ',h('a',{href:selected.url,'data-url':selected.url},'#'+selected.n),' ? '+(selected.read?'Read':'Unread')),
      selected.reason&&h('p',{role:'status'},'Report unknown: '+selected.reason),
      selected.body?[h('h3',null,'Question'),body(summary(selected.body,'Question')),h('h3',null,'Recommendation'),body(summary(selected.body,'Recommendation'))]:h('p',null,'Report content unknown; waiting for background collection.'),
      selected.tasks&&h('div',null,h('h3',null,'Proposed tasks'),selected.tasks.length?selected.tasks.map((t,i)=>h('details',{key:i},h('summary',null,(i+1)+'. '+t.title),h('p',null,t.goal),h('ul',null,t.acceptance?.map(a=>h('li',null,a))),t.scope?.map(s=>h('p',null,s)),h('p',{class:'meta'},[t.type,t.runtime,t.priority&&'Priority '+t.priority].filter(Boolean).join(' ? ')))):h('p',null,'No follow-up tasks proposed.')),
      h('p',{class:'meta'},'Make tasks creates held drafts linked to this report. Start them separately.'),
      selected.pending_decision==='make'&&h('p',{role:'status'},'Make tasks is pending. Retry to reconcile the held drafts.'),
      h('div',{class:'options'},['make','later','drop'].map(action=>h('button',{'data-report':action,disabled:!!busy||!selected.body||action==='make'&&!selected.tasks?.length||action==='drop'&&selected.pending_decision==='make'},{make:selected.pending_decision==='make'?'Retry Make tasks':'Make tasks',later:'Later',drop:'Drop'}[action]))),
      selected.body&&h('div',null,h('button',{'data-report':'full','aria-expanded':String(full)},full?'Hide full report':'Full report'),full&&h('div',{class:'report-body'},body(selected.body)))):
     [rows.length?rows.map(r=>h('button',{class:'project report-row','data-report':'open','data-root':r.root,'data-n':r.n,key:key(r.root,r.n)},h('span',{class:'meta'},project(r.root)+' ? #'+r.n+' ? '+(r.read?'Read':'Unread')),h('span',null,r.title))):loaded&&h('p',null,'No reports waiting for a decision.')]));
  }
 });return api;
},

// Keep untrusted Markdown as text/vnodes; evidence URLs are the only active content. No raw HTML.
body:function(text){
 const prose=text.replace(/```taskq-tasks\n[\s\S]*?```/g,'').replace(/(^|\n)#{1,6}[ \t]+Proposed tasks[ \t]*(?=\n|$)/gi,'$1').replace(/(^|\n)(#{1,6} [^\n]+)\n/g,'$1$2\n\n');
 return prose.split(/\n\s*\n/).map((p,i)=>{
  const heading=/^#{1,6}\s+(.+)$/.exec(p.trim());
  if(heading)return h('h3',{key:i},heading[1]);
  const parts=p.split(/(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\))/g).map((s,k)=>{const link=/^\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)$/.exec(s);return link?h('a',{key:k,href:link[2],'data-url':link[2]},link[1]):s;});
  return h('p',{key:i,style:{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}},parts);
 });
}
});
module.exports={create};
