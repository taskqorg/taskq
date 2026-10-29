'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;

// §7: both surfaces use this view; the board owns read/decision state, memory holds only an in-flight view.
const {create,body}=execution.module('reports',{
create:function({send=ui.send,draw,defer=ui.defer,id=()=>Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')}){
 let expanded=false,projects=[],pages=new Map(),pending=new Map(),selected=null,notice='';
 const key=(root,n)=>root+'#'+n;
 function targets(ps){const seen=new Set();return ps.filter(p=>{const board=p.issues||p.root;if(seen.has(board))return false;seen.add(board);return true;});}
 function request(root,action,extra={}){
  if([...pending.values()].some(p=>p.root===root))return;
  const op={id:id(),root,action,...extra};pending.set(op.id,op);send({a:'reports',...op});
  defer(()=>{if(pending.delete(op.id)){notice='Report outcome unknown; refresh before retrying.';draw();}},180000);draw();
 }
 const api={
  toggle(ps){projects=targets(ps);expanded=!expanded;if(expanded)api.refresh();draw();},
  refresh(){notice='';for(const p of projects)request(p.root,'list',{page:1});},
  rows(){return projects.flatMap(p=>(pages.get(p.root)?.rows||[]).map(r=>({...r,root:p.root}))).sort((a,b)=>(b.closed||0)-(a.closed||0)||b.n-a.n);},
  current(){return selected;},
  open(root,n){if([...pending.values()].some(p=>p.root===root))return;const r=api.rows().find(r=>r.root===root&&r.n===n);if(!r)return;selected={...r};notice='';request(root,'open',{n,sha:r.sha});},
  act(action){if(!selected)return;notice='';request(selected.root,action,{n:selected.n,sha:selected.sha});},
  receive(r){
   const op=pending.get(r.id);if(!op||op.root!==r.root)return false;pending.delete(r.id);
   if(r.reason){if(selected&&selected.root===op.root&&selected.n===op.n&&r.report?.sha===op.sha)selected={...selected,pending_decision:r.report.pending_decision};notice=r.reason+(Array.isArray(r.receipts)&&r.receipts.length?' · Created held drafts: '+r.receipts.map(n=>'#'+n).join(', '):'');draw();return true;}
   if(op.action==='list'){
    if(!Array.isArray(r.reports)){notice='Report list unknown; refresh.';draw();return true;}
    const prior=op.page===1?[]:pages.get(r.root)?.rows||[];
    pages.set(r.root,{rows:[...new Map([...prior,...r.reports].map(r=>[r.n,r])).values()],page:op.page,more:r.more===true,next:r.next});
   }else if(r.report?.n===op.n&&r.report.sha===op.sha){
    const p=pages.get(r.root),report=r.report;
    if(p)p.rows=p.rows.flatMap(row=>row.n!==op.n?[row]:report.visible===false?[]:[{...row,...report,closed:row.closed}]);
    if(selected&&key(selected.root,selected.n)===key(r.root,op.n))selected=report.visible===false?null:{...selected,...report,closed:selected.closed};
    notice=report.visible===false?(report.decision==='made'?'Created held drafts: '+(report.tasks_made||[]).map(n=>'#'+n).join(', '):'Report dropped.'):(op.action==='later'?'Kept in Reports for later.':'');
   }else notice='Report receipt unknown; refresh before retrying.';
   draw();return true;
  },
  click(b,ps){
   const action=b.dataset.report;if(!action)return false;
   if(action==='toggle')api.toggle(ps);
   else if(action==='refresh')api.refresh();
   else if(action==='open')api.open(b.dataset.root,Number(b.dataset.n));
   else if(action==='more'){const p=pages.get(b.dataset.root);if(p?.more)request(b.dataset.root,'list',{page:p.page+1,before:p.next});}
   else if(action==='back'){selected=null;draw();}
   else if(['make','later','drop'].includes(action))api.act(action);
   return true;
  },
  section(ps){
   projects=targets(ps);const loaded=projects.every(p=>pages.has(p.root)),rows=api.rows(),busy=selected&&(!projects.some(p=>p.root===selected.root)||[...pending.values()].some(p=>p.root===selected.root)),project=root=>projects.find(p=>p.root===root)?.name||'Project unavailable';
   return h('section',{class:'reports-section group','aria-label':'Reports',key:'reports'},
    h('button',{class:'project',type:'button','data-report':'toggle','aria-expanded':String(expanded)},'Reports',expanded&&loaded?' · '+rows.length+(projects.some(p=>pages.get(p.root)?.more)?'+':''):''),
    expanded&&h('div',{class:'reports-content body'},
     h('div',{class:'options'},h('button',{'data-report':'refresh',disabled:pending.size>0},'Refresh reports'),selected&&h('button',{'data-report':'back'},'All reports')),
     notice&&h('p',{role:'status'},notice),pending.size>0&&h('p',{class:'meta',role:'status'},'Reading or updating the board…'),
     selected?h('article',{class:'report-card','aria-label':'Research report'},h('h2',null,selected.title),h('p',{class:'meta'},project(selected.root)+' · ',h('a',{href:selected.url,'data-url':selected.url},'#'+selected.n),' · '+(selected.read?'Read':'Unread')),h('div',{class:'report-body'},selected.body?body(selected.body):h('p',null,'Report content unknown; opening requires a successful board read.')),
      selected.tasks&&h('div',null,h('h3',null,'Proposed tasks'),selected.tasks.length?selected.tasks.map((t,i)=>h('details',{key:i},h('summary',null,(i+1)+'. '+t.title),h('p',null,t.goal),h('ul',null,t.acceptance?.map(a=>h('li',null,a))),t.scope?.map(s=>h('p',null,s)),h('p',{class:'meta'},[t.type,t.runtime,t.priority&&'Priority '+t.priority].filter(Boolean).join(' · ')))):h('p',null,'No follow-up tasks proposed.')),
      h('p',{class:'meta'},'Make tasks creates held drafts linked to this report. Start them separately.'),
      selected.pending_decision==='make'&&h('p',{role:'status'},'Make tasks is pending. Retry to reconcile the held drafts.'),
      h('div',{class:'options'},['make','later','drop'].map(action=>h('button',{'data-report':action,disabled:!!busy||!selected.body||action==='make'&&!selected.tasks?.length||action==='drop'&&selected.pending_decision==='make'},{make:selected.pending_decision==='make'?'Retry Make tasks':'Make tasks',later:'Later',drop:'Drop'}[action])))):
     [rows.length?rows.map(r=>h('button',{class:'project report-row','data-report':'open','data-root':r.root,'data-n':r.n,key:key(r.root,r.n),disabled:[...pending.values()].some(p=>p.root===r.root)},h('span',{class:'meta'},project(r.root)+' · #'+r.n+' · '+(r.read?'Read':'Unread')),h('span',null,r.title))):pending.size===0&&loaded&&h('p',null,'No reports waiting for a decision.'),pending.size===0&&!loaded&&h('p',{role:'status'},'Refresh reports to read all projects.'),projects.map(p=>{const page=pages.get(p.root);return page?.more&&h('button',{'data-report':'more','data-root':p.root,disabled:[...pending.values()].some(o=>o.root===p.root)},'More reports · '+p.name);})]));
  }
 };return api;
},

// Keep untrusted Markdown as text/vnodes; evidence URLs are the only active content. No raw HTML.
body:function(text){
 const prose=text.replace(/```taskq-tasks\n[\s\S]*?```/g,'').replace(/(^|\n)(#{1,6} [^\n]+)\n/g,'$1$2\n\n');
 return prose.split(/\n\s*\n/).map((p,i)=>{
  const heading=/^#{1,6}\s+(.+)$/.exec(p.trim());
  if(heading)return h('h3',{key:i},heading[1]);
  const parts=p.split(/(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\))/g).map((s,k)=>{const link=/^\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)$/.exec(s);return link?h('a',{key:k,href:link[2],'data-url':link[2]},link[1]):s;});
  return h('p',{key:i,style:{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}},parts);
 });
}
});
module.exports={create};
