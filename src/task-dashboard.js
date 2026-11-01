'use strict';
const {h,Fragment}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const views=require('./task-views.js'),blocks=require('./project-blocks.js'),popover=require('./anchored-popover.js'),{icon,button}=require('./ui-icons.js');
// MP14: closure outcome labels; only a success with a sourced merge receipt reads Merged (git-merge, Review purple).
const outcomes={success:['circle-check','','Completed'],not_planned:['circle-slash','','Not planned'],unknown:['circle-question-mark','','Outcome unknown']};
module.exports=execution.module('taskDashboard',{
 outcome:function(t){return t.outcome==='success'&&typeof t.merged==='string'&&t.merged?['git-merge','review','Merged']:outcomes[t.outcome]||outcomes.unknown;},
 // P-W4: `owned` is the View window surface; the menu sends `views-open` when native offers that window, else keeps the overlay.
 create:function(view,{menu,present,send,app,owned=false,window=globalThis,document=window.document}){
  const crypto=window.crypto||globalThis.crypto,CSS=window.CSS||globalThis.CSS;let restore=true,requests=0,heldOffer=null,heldReview=null;const pinReads=new Map();
  const api=execution.module('taskDashboard.instance',{
   model:function(){const m=view.dashboard||(view.dashboard=views.normalize(null,{taskFilter:view.taskFilter}));m.collapsed=m.collapsed.filter(id=>view.S.projects.some(p=>views.identity(p)===id));return m;},
   current:function(){return views.current(api.model());},
   criteria:function(){return api.current().working;},
   set:function(patch){Object.assign(api.criteria(),patch);present();},
   projects:function(){return views.scope(view.S.projects,api.criteria());},
   unique:function(ps){return ps.filter((p,i)=>ps.findIndex(q=>views.identity(q)===views.identity(p))===i);},
   live:function(){return app.unique(view.S.projects.flatMap(app.tasks)).filter(t=>t.state!=='closed');},
   ref:function(t){return t.boardIdentity+'#'+t.n;},
   rows:function(){return app.unique(api.projects().flatMap(app.tasks)).filter(t=>t.state!=='closed'&&views.matches(t,api.criteria().query)).sort(views.compare);},
   heldScope:function(){
    const c=api.criteria(),ps=api.projects(),targets=api.rows().filter(t=>t.state==='held').map(t=>({key:t.key,revision:t.revision,label:t.project+' #'+t.n+' '+t.title}));
    const known=ps.length>0&&ps.every(p=>p.alive===true&&p.complete===true&&p.tasks_live!==false&&!p.partial?.length&&!view.S.display.all.partial.includes(p.name))&&c.projects.every(id=>ps.some(p=>views.identity(p)===id))&&targets.every(t=>Number.isSafeInteger(t.revision));
    const scope=ps.map(p=>p.name).join(', ')+' · view '+api.current().name+' · '+(c.query?'search “'+c.query+'”':'all searches');
    return {targets,known,scope,signature:JSON.stringify([api.current().id,c.projects,c.query,targets])};
   },
   admissionReview:function(){return heldReview&&h('section',{class:'held-review',role:'region','aria-label':'Review Held admission'},h('strong',null,'Start '+heldReview.targets.length+' held'),h('p',null,heldReview.scope),h('p',{class:'meta'},'Admit only these drafts. Existing focus, search and view stay unchanged.'),h('ul',null,heldReview.targets.map(t=>h('li',{key:t.key},h('a',{href:t.key,'data-url':t.key},t.label),h('span',{class:'meta'},' · revision '+t.revision)))),heldReview.error&&h('p',{role:'alert'},heldReview.error),heldReview.sent&&h('p',{role:'status'},'Start requested once; check each receipt below.'),h('div',{class:'row-actions'},h('button',{'data-a':'held-confirm',disabled:!!heldReview.sent},'Confirm Start '+heldReview.targets.length+' held'),h('button',{'data-a':'held-cancel'},heldReview.sent?'Close Held admission':'Cancel Held admission')));},
   anchor:function(target){const r=target.getBoundingClientRect();return {left:r.left,top:r.top,bottom:r.bottom};},
   // MP13: details open only from click, Enter or Space on the identity (native button activation); `focus` moves focus into them.
   open:function(kind,target,key=null,focus=true){view.popup={kind,key,anchor:api.anchor(target)};if(kind==='task')api.current().open=key;present();if(focus)menu.querySelector('#taskq-question-details,#taskq-popover')?.focus({preventScroll:true});},
   close:function(focus=true){const p=view.popup;if(!p)return;view.popup=null;if(p.kind==='task')api.current().open=null;present();if(focus){const target=p.kind==='task'?menu.querySelector('[data-a="task-detail"][data-key="'+CSS.escape(p.key)+'"]'):menu.querySelector('[data-a="'+(p.kind==='views'?'view-list':'view-editor')+'"]');(target||menu.querySelector('[data-a=view-list]'))?.focus({preventScroll:true});}},
   // M2: collapse is presentation only; the name and the two adjacent shortcuts keep independent targets.
   projectHeader:function(p,shown,tasks,reason){const board=p.board_kind==='gitlab'?'GitLab':'GitHub',id=views.identity(p),open=!api.model().collapsed.includes(id),controls='project-tasks-'+encodeURIComponent(id),folder=typeof p.root==='string'&&!!p.root.trim();
    const questions=tasks.filter(t=>t.state==='ask').length,label=reason?'Matching and total task counts and open questions unknown: '+reason:shown+' tasks shown matching the current filter; '+tasks.length+' open tasks in total; '+questions+' open questions';
    const statuses=!open&&h('div',{class:'project-statuses'},reason?h('span',{role:'img',title:'Status counts unknown: '+reason,'aria-label':'Status counts unknown: '+reason},'?'):view.S.display.states.map(({id:state,name})=>{
     const n=tasks.filter(t=>t.state===state).length;if(!n)return null;const [mark,tone]=blocks.marks[state]||['circle-question-mark',''];return h('span',{class:'project-state '+tone,'data-state':state,role:'img',title:name+': '+n,'aria-label':name+': '+n,key:state},icon(mark),n);
    }));
    return h('header',{class:'project-group project-shortcuts'},button('arrow-down',(open?'Collapse ':'Expand ')+p.name+' tasks',{'data-a':'project-collapse','data-project':id,'aria-expanded':String(open),'aria-controls':controls,class:'compact-action project-collapse'}),
     h('div',{class:'project-heading'},h('h2',null,h('button',{class:'project-overview','data-route':p.root,title:'Open '+p.name+' project overview'},p.name)),h('span',{class:'project-counts',title:label,'aria-label':label},h('span',null,(reason?'?':shown)+' / '+(reason?'?':tasks.length)),(reason||questions>0)&&h('span',{class:'project-questions'},' (?'+(reason?'?':questions)+')')),statuses),
     h('div',{class:'project-actions'},h('a',{class:'project-board compact-action',href:p.issues,'data-url':p.issues,title:'Open '+p.name+' board on '+board,'aria-label':'Open '+p.name+' board on '+board},icon(p.board_kind)),button('folder',folder?'Open '+p.name+' project folder on this computer':'Project folder unavailable: registered root missing',{'data-a':'folder','data-root':p.root,disabled:!folder,class:'compact-action project-folder'})));
   },
   bar:function(){const m=api.model(),v=api.current(),c=v.working,searching=!!c.query||!!view.searchOpen;
    return h(Fragment,null,h('div',{class:'viewbar'},h('div',{class:'view-picks'},h('button',{'data-a':'view-list','aria-haspopup':'dialog','aria-expanded':String(view.popup?.kind==='views')},v.name,views.dirty(v)?' *':''),m.views.filter(x=>x.pinned&&x.id!==m.selected).map(x=>h('button',{'data-a':'view-select','data-id':x.id,key:x.id},x.name))),views.dirty(v)&&button('rotate-ccw','Reset '+v.name+' to its saved criteria',{'data-a':'view-reset'}),button('search','Search tasks',{'data-a':'search-open','aria-controls':'task-query','aria-expanded':String(searching)}),button('sliders-horizontal','Edit view conditions',{'data-a':'view-editor','aria-haspopup':'dialog','aria-expanded':String(view.popup?.kind==='editor')})),
     // #1775: search is one icon until opened; a query keeps the field visible
     searching&&h('div',{class:'task-search'},icon('search'),h('input',{id:'task-query',type:'search','data-task-query':'',maxLength:1024,'aria-label':'Search tasks',placeholder:'Search tasks',value:c.query}),c.query&&button('x','Clear task search',{'data-a':'search-clear'})));
   },
   // P-W1: global pins once, first, as ordinary rows in the one shared scroll; unavailable references keep a sourced outcome or explicit unknown.
   // Exact reads use canonical board identity, never a pin URL or the loaded Completed page.
   pinRead:function(pin,retry=false){
    let e=pinReads.get(pin.ref);const now=Date.now();if(e?.loading||e&&!retry&&now-e.at<60000)return e;
    const cut=pin.ref.lastIndexOf('#'),n=Number(pin.ref.slice(cut+1)),p=view.S.projects.find(p=>views.identity(p)===pin.ref.slice(0,cut));
    if(!p||!Number.isSafeInteger(n)||n<=0)return {reason:'Registered board unavailable'};
    e={...e,ref:pin.ref,root:p.root,task:n,kind:p.board_kind,project:p.name,id:'pin-'+(++requests),at:now,loading:true,reason:null};pinReads.set(pin.ref,e);
    e.timer=ui.defer(()=>api.receive({root:e.root,id:e.id,reason:'No board response within 65 seconds'}),65000);
    send({a:'completed',root:e.root,id:e.id,task:n});return e;
   },
   unpin:function(ref){ui.cancel(pinReads.get(ref)?.expiry);api.model().pins=api.model().pins.filter(p=>p.ref!==ref);},
   pins:function(){const m=api.model();if(!m.pins.length)return null;const live=api.live();
    const rows=m.pins.map(pin=>{const t=live.find(t=>api.ref(t)===pin.ref);if(t){delete pin.closedSeen;const prior=pinReads.get(pin.ref);ui.cancel(prior?.timer);ui.cancel(prior?.expiry);pinReads.delete(pin.ref);return app.taskRow(t,true);}
     const e=api.pinRead(pin),c=e?.value,closed=c?.state==='closed';
     if(closed){pin.closedSeen=Number.isFinite(c.closed)&&c.closed>0?c.closed:pin.closedSeen||view.S.now;const remaining=pin.closedSeen+86400-view.S.now;if(!e.expiry&&remaining>0)e.expiry=ui.defer(()=>{const current=pinReads.get(pin.ref);if(current)current.expiry=null;if(current?.value?.state==='closed'&&!api.live().some(t=>api.ref(t)===pin.ref)){api.unpin(pin.ref);present();}},remaining*1000);if(Number.isFinite(remaining)&&remaining<=0){api.unpin(pin.ref);return null;}}
     const [mark,tone,label]=closed?api.outcome(c):['circle-question-mark','',c?.state==='open'?'Open · refresh project':e?.loading?'Reading board…':'Unavailable · state unknown'];
     return h('div',{class:'task-row completed-row',key:pin.ref,'data-pin':pin.ref},h('span',{class:tone,title:label,'aria-label':label},icon(mark)),h('a',{class:'detail-title',href:c?.url||pin.url,'data-url':c?.url||pin.url,'data-pin-open':pin.ref},c?.title||pin.title||pin.ref),
      h('div',{class:'meta'},e.project?e.project+' #'+e.task+' · ':'',label,closed&&c.close?' · '+c.close:'',e?.reason?' · '+e.reason:'',closed&&c.error?' · Details unavailable: '+c.error:'',e?.reason&&h('button',{'data-a':'pin-retry','data-ref':pin.ref},'Retry board read')),
      h('div',{class:'row-actions'},closed&&h(Fragment,null,h('a',{href:c.url,'data-url':c.url,'data-pin-open':pin.ref},'Open issue'),c.pr&&h('a',{href:c.pr,'data-url':c.pr,'data-pin-open':pin.ref},'Open PR'),h('button',{'data-a':'pin-transcript','data-ref':pin.ref},'Open transcript')),button('pin-off','Unpin '+(c?.title||pin.title||pin.ref),{'data-a':'task-unpin','data-ref':pin.ref})));
    });return rows.some(Boolean)&&h('section',{class:'task-pins','aria-label':'Pinned tasks'},h('h2',null,icon('pin'),' Pinned'),rows);
   },
   openCompleted:function(task){const p=task&&view.S.projects.find(p=>p.root===task.root);send({a:'workspace',open:task?{page:'conversations',root:task.root,board:p?.issues,task:task.task,...(task.value?.sha?{sha:task.value.sha}:{})}:{page:'conversations',filter:'finished',criteria:api.criteria()}});},
   list:function(){if(api.current().id==='completed')return h(Fragment,null,api.pins(),api.bar(),h('button',{'data-a':'completed-open'},'Open Completed in Conversations'));
    const ps=api.projects(),c=api.criteria(),all=api.rows(),pinned=new Set(api.model().pins.map(p=>p.ref)),matching=all.filter(t=>!c.states.length||c.states.includes(t.state)),rows=matching.filter(t=>!pinned.has(api.ref(t))),live=api.live();
    // #1592: partial names and the deduplicated, ordered rows come from the display model; the view only filters them.
    const loaded=ps.filter(p=>p.complete===true&&p.tasks_live!==false&&p.issues),names=loaded.map(p=>p.name),partial=[...new Set([...loaded.flatMap(p=>p.partial||[]),...view.S.display.all.partial.filter(n=>names.includes(n))])],missing=c.projects.filter(id=>!view.S.projects.some(p=>views.identity(p)===id));
    if(missing.length)partial.push('Selected projects unavailable');
    const counted=all.filter(t=>loaded.some(p=>p.root===t.root)),known=(loaded.length>0||ps.length===0)&&!partial.length;
    const errors=ps.filter(p=>!loaded.includes(p)).map(p=>p.name+': '+(p.error||'Project snapshot incomplete or stale'));
    const counts={counts:view.S.display.states.map(({id,name})=>({state:id,name,n:known?counted.filter(t=>t.state===id).length:null})),open:known?counted.length:null};
    const listing=c.group==='combined'?rows.map(t=>app.taskRow(t,true)):api.unique(ps).map(p=>{
     const id=views.identity(p),group=rows.filter(t=>t.boardIdentity===id),sources=(loaded.some(q=>views.identity(q)===id)?loaded:ps).filter(q=>views.identity(q)===id),reason=sources.flatMap(q=>[...(q.partial||[]),...(q.complete!==true||q.tasks_live===false?[q.error||'Project snapshot incomplete or stale']:[]),...(view.S.display.all.partial.includes(q.name)?['Project task data incomplete']:[])]).join('; ');
     return group.length?h('section',{class:'all-tasks-project',key:id},api.projectHeader(p,matching.filter(t=>t.boardIdentity===id).length,live.filter(t=>t.boardIdentity===id),reason),h('div',{class:'project-tasks',id:'project-tasks-'+encodeURIComponent(id),hidden:api.model().collapsed.includes(id)},group.map(t=>app.taskRow(t,false)))):null;
    });
    heldOffer=api.heldScope();const heldCount=heldOffer.known?heldOffer.targets.length:'?';
    const start={count:heldCount,label:'Start '+heldCount+' held · '+heldOffer.scope,disabled:!heldOffer.known||!heldOffer.targets.length||all.some(t=>t.state==='held'&&app.startPending(t))};
    return h(Fragment,null,api.pins(),api.bar(),h('header',{class:'filter-header tqp'},h('div',{class:'state-filters',dangerouslySetInnerHTML:{__html:blocks.counters(counts.counts,'','Selected projects',partial,c.states,counts.open,start)}})),api.admissionReview(),errors.map(reason=>h('p',{class:'meta',role:'status',key:reason},reason)),app.answerNotices(),rows.length?listing:h('div',{class:'body meta'},!known?'Tasks unknown':'No matching tasks',' ',h('button',{'data-a':'filter-reset'},'Reset')));
   },
   receive:function(page){const pin=[...pinReads.values()].find(e=>e.id===page.id&&e.root===page.root);if(!pin?.loading)return;ui.cancel(pin.timer);pin.loading=false;pin.reason=page.reason;pin.value=!page.reason&&page.tasks?.[0]?.n===pin.task?page.tasks[0]:null;if(!pin.value&&!pin.reason)pin.reason='Board response omitted the task';present();},
   outcome:function(t){return module.exports.outcome(t);},
   conditions:function(){const c=api.criteria(),done=api.current().id==='completed';return h(Fragment,null,
    h('fieldset',null,h('legend',null,'Projects'),h('div',{class:'facet-list'},h('button',{'data-a':'view-facet','data-facet':'projects','data-value':'','aria-pressed':String(!c.projects.length)},'All projects'),api.unique(view.S.projects).map(p=>h('div',{class:'project-choice',key:views.identity(p)},h('label',null,h('input',{type:'checkbox','data-project':views.identity(p),'aria-label':'Include '+p.name,disabled:!views.identity(p),checked:!c.projects.length||c.projects.includes(views.identity(p))})),h('button',{'data-a':'view-facet','data-facet':'projects','data-value':views.identity(p),disabled:!views.identity(p),'aria-label':'Show only '+p.name,'aria-pressed':String(c.projects.length===1&&c.projects[0]===views.identity(p))},p.name))))),
    done?h(Fragment,null,h('fieldset',null,h('legend',null,'Period'),h('div',{class:'options'},[['7d','7 days'],['30d','30 days'],['all','All time']].map(([value,label])=>h('button',{'data-a':'view-period','data-value':value,'aria-pressed':String(c.period===value)},label)))),
     h('fieldset',null,h('legend',null,'Outcome'),h('div',{class:'facet-list'},h('button',{'data-a':'view-facet','data-facet':'outcomes','data-value':'','aria-pressed':String(!c.outcomes.length)},'All outcomes'),Object.entries(outcomes).map(([id,[mark,,label]])=>h('button',{'data-a':'view-facet','data-facet':'outcomes','data-value':id,'aria-pressed':String(c.outcomes.includes(id))},icon(mark),label))))):
    // #1700 A4: each state choice keeps its token tone (Review purple), as on the task rows.
    h('fieldset',null,h('legend',null,'States'),h('div',{class:'facet-list'},h('button',{'data-a':'all-tasks','aria-pressed':String(!c.states.length)},'All states'),view.S.display.states.map(({id:s,name:n})=>h('button',{'data-a':'view-facet','data-facet':'states','data-value':s,'aria-pressed':String(c.states.includes(s))},h('span',{class:blocks.marks[s][1]||null},icon(blocks.marks[s][0])),n)))));
   },
   actions:function(){const c=api.criteria(),v=api.current();return h(Fragment,null,v.id!=='completed'&&h('fieldset',null,h('legend',null,'Display'),h('div',{class:'options'},[['combined','All together'],['project','By project']].map(([value,label])=>h('button',{'data-a':'view-group','data-value':value,'aria-pressed':String(c.group===value)},label)))),
    !owned&&api.naming(),h('div',{class:'options'},h('button',{'data-a':'view-save',disabled:!views.dirty(v)},icon('save'),'Save'),h('button',{'data-a':'view-save-as','aria-expanded':String(!!view.naming)},'Save as…'),h('button',{'data-a':'view-reset'},icon('rotate-ccw'),'Reset'),h('button',{'data-a':'popup-close'},'Done')));
   },
   naming:function(){const v=api.current();return view.naming&&h('div',{class:'view-naming'},h('input',{'data-view-name':'','aria-label':'New view name',value:v.draft,maxLength:80,placeholder:'View name'}),h('button',{'data-a':'view-add',disabled:!v.draft.trim()||api.model().views.length>=64},'Create'),h('button',{'data-a':'view-cancel'},'Cancel'));},
   cancelNaming:function(){view.naming=false;present();menu.querySelector('[data-a="view-save-as"]')?.focus();},
   editor:function(){return h(Fragment,null,h('div',{class:'popover-heading'},h('strong',null,'View'),button('x','Close view editor',{'data-a':'popup-close'})),api.conditions(),api.actions());},
   // P-V1: Delete beside each custom view; the shortcut pin stays a separate control; one persisted Undo receipt.
   presets:function(){const m=api.model();return h(owned?'fieldset':Fragment,null,owned?h('legend',null,'Saved views'):h('div',{class:'popover-heading'},h('strong',null,'Saved views'),button('x','Close saved views',{'data-a':'popup-close'})),
    m.views.map(v=>h('div',{class:'saved-view',key:v.id},h('button',{'data-a':'view-select','data-id':v.id,'aria-pressed':String(v.id===m.selected)},v.name,views.dirty(v)?' *':''),button(v.pinned?'pin':'pin-off',(v.pinned?'Remove shortcut for ':'Add shortcut for ')+v.name,{'data-a':'view-pin','data-id':v.id,'aria-pressed':String(v.pinned)}),!views.builtin(v.id)&&button('trash-2','Delete view '+v.name,{'data-a':'view-delete','data-id':v.id}))),
    m.undo&&h('div',{class:'view-receipt',role:'status'},'Deleted ',h('strong',null,m.undo.view.name),button('undo-2','Undo deleting '+m.undo.view.name,{'data-a':'view-undo'})),
    owned?api.naming():h(Fragment,null,api.naming(),h('button',{'data-a':'view-save-as','aria-expanded':String(!!view.naming)},'Save as…')),view.viewError&&h('p',{role:'alert'},view.viewError));},
   // #1781: details carry a short question cue; the full question and options read in Conversations (input-scroll 04).
   question:function(t){if(t.state!=='ask')return t.card&&h('p',{class:'task-card-text'},t.card);const q=t.question,text=(q?.parsed?q.problem:q?.text)||t.card||'Open the question to read and answer.';return h('p',{class:'task-card-text question-cue'},text.length>170?text.slice(0,167)+'…':text);},
   // § 2 visual evidence: ask images as thumbnails that open full size; the link alone without the service's copy.
   images:function(q){const im=(q?.evidence||[]).filter(e=>e.image);return im.length>0&&h('p',{class:'task-images'},im.map(e=>h('a',{key:e.url,href:e.url,'data-url':e.url,title:'Open '+e.label+' full size'},/^data:image\/(png|jpeg|gif|webp);base64,/.test(e.src||'')?h('img',{class:'task-thumb',src:e.src,alt:e.label}):e.label+' ↗')));},
   // M3/MP13: every open task has one detail surface in the main list scroll.
   attention:function(t){const im=t.state==='ask'&&api.images(t.question),open=view.popup?.kind==='task'&&view.popup.key===t.key;return h(Fragment,null,im,open&&h('section',{id:'taskq-question-details',class:'task-inline-details question-task-details '+(blocks.marks[t.state]?.[1]||''),tabindex:-1,'aria-label':'Task details for #'+t.n},api.detail(t)));},
   detail:function(t){if(!t)return null;
    const p=view.S.projects.find(p=>p.root===t.root),pinned=views.pinned(api.model(),api.ref(t));
    return h(Fragment,null,h('div',{class:'popover-heading'},app.answerSlot(t,false),button(pinned?'pin-off':'pin',pinned?'Unpin task':'Pin task',{'data-a':'task-pin','aria-pressed':String(pinned)}),button('x','Close task details',{'data-a':'popup-close'})),
     t.wait&&h('p',{role:'status'},t.wait),t.publication?.reason&&!t.wait&&h('p',{role:'status'},t.publication.reason),t.enqueued&&!t.publication?.reason&&h('p',{class:'meta'},'CI progress unknown in this snapshot.'),t.failed&&h('p',{class:'alert'},t.failed),t.later&&!t.until&&h('p',{role:'status'},t.later),api.question(t),t.result&&h('p',{class:'task-card-text'},'Result: '+t.result),
     h('div',{class:'task-destinations'},blocks.destinations(t).map(d=>d.url?h('a',{href:d.url,'data-url':d.url,'data-destination':d.kind},icon(d.kind==='native'?'external-link':'network'),d.label):h('p',{class:'meta','data-destination':d.kind},d.label+' unavailable: '+d.reason))),
     (t.blocked||[]).map(b=>h('p',null,h('a',{href:b.url,'data-url':b.url},b.ref),' · '+b.reason)),
     h('div',{class:'row-actions'},t.state==='held'&&app.startButton([t]),h('span',{dangerouslySetInnerHTML:{__html:require('./task-controls.js').render(t,{key:t.key,url:t.url,board:p.board_kind,except:['start']})}})),
     h('details',{class:'task-more','data-view-key':'more:'+t.key},h('summary',null,icon('info'),'Task info'),h('dl',null,h('dt',null,'Runtime'),h('dd',null,t.runtime||'unknown'),h('dt',null,'Host'),h('dd',null,t.host||'unknown'),h('dt',null,'Role'),h('dd',null,t.role||'unknown'),h('dt',null,'Session'),h('dd',null,t.session||t.session_reason||'Session unavailable'),
      h('dt',null,'Attempt'),h('dd',null,t.attempt||'unknown'),h('dt',null,'Revision'),h('dd',null,t.revision??'unknown'),h('dt',null,'Total age'),h('dd',null,t.total_age??'unknown'),h('dt',null,'Steps'),h('dd',null,Array.isArray(t.steps)?t.steps.filter(s=>s.complete===true).length+' completed':'unknown')),
      Array.isArray(t.steps)&&t.steps.length?h('ol',{class:'task-steps'},t.steps.map(s=>h('li',null,s.state,' · ',s.age??'duration unknown',s.complete?'':' · current'))):h('p',{class:'meta'},'Ordered stage history unavailable in this snapshot.')),
     app.answerNotices(p.issues));
   },
   // P-W4: the menu takes the View window's model whole; a switched view restores its own scroll and open task.
   adopt:function(value){const prior=view.dashboard?.selected;view.dashboard=views.normalize(value);if(prior!==view.dashboard.selected){view.restoreScroll=api.current().scroll;restore=true;}present();},
   popup:function(){const p=view.popup;if(!p||p.kind==='task')return null;return popover.render({label:p.kind==='editor'?'View editor':'Saved views',children:p.kind==='editor'?api.editor():api.presets()});},
   place:function(){
    // A3: the view's open task returns on view switch and restart, never on a data refresh.
    if(restore&&view.S&&view.page==='home'){restore=false;const key=api.current().open,trigger=key&&menu.querySelector('[data-a="task-detail"][data-key="'+CSS.escape(key)+'"]');if(!view.popup&&trigger){api.open('task',trigger,key,false);return;}}
    if(view.popup){const selector=view.popup.kind==='task'?'[data-a="task-detail"][data-key="'+CSS.escape(view.popup.key)+'"]':'[data-a="'+(view.popup.kind==='views'?'view-list':'view-editor')+'"]',trigger=menu.querySelector(selector);if(trigger)view.popup.anchor=api.anchor(trigger);}for(const b of menu.querySelectorAll('[data-a="task-detail"]')){b.setAttribute('aria-expanded',String(view.popup?.kind==='task'&&view.popup.key===b.dataset.key));b.setAttribute('aria-controls','taskq-question-details');b.removeAttribute('aria-haspopup');}if(view.popup&&view.popup.kind!=='task')popover.place(menu.querySelector('#taskq-popover'),view.popup.anchor);},
   click:function(e){const link=e.target.closest('[data-pin-open]');if(link&&pinReads.get(link.dataset.pinOpen)?.value?.state==='closed'){api.unpin(link.dataset.pinOpen);ui.defer(present,0);return false;}const b=e.target.closest('button');if(!b||b.disabled)return false;const d=b.dataset,m=api.model();if(d.a==='search-open'){view.searchOpen=!!api.criteria().query||!view.searchOpen;present();menu.querySelector(view.searchOpen?'[data-task-query]':'[data-a=search-open]')?.focus({preventScroll:true});return true;}if(d.a==='search-clear'){api.criteria().query='';view.searchOpen=false;present();menu.querySelector('[data-a=search-open]')?.focus({preventScroll:true});return true;}
    if(d.a==='held-review'){if(heldOffer?.known&&heldOffer.targets.length){heldReview=structuredClone(heldOffer);present();menu.querySelector('[data-a=held-confirm]')?.focus();}return true;}
    if(d.a==='held-cancel'){heldReview=null;present();menu.querySelector('[data-a=held-review]')?.focus();return true;}
    if(d.a==='held-confirm'){
     if(!heldReview||heldReview.sent)return true;
     const current=api.heldScope();
     if(!current.known||current.signature!==heldReview.signature||!app.startHeld(heldReview.targets))heldReview.error='Held targets or scope changed; cancel and review the current drafts.';
     else heldReview.sent=true;
     present();return true;
    }
    if(d.a==='project-collapse'){const closing=!m.collapsed.includes(d.project);m.collapsed=closing?[...m.collapsed,d.project]:m.collapsed.filter(id=>id!==d.project);if(closing&&view.popup?.kind==='task'&&api.live().some(t=>t.key===view.popup.key&&t.boardIdentity===d.project))api.close(false);else present();return true;}
    if(d.a==='task-detail'){if(view.popup?.kind==='task'&&view.popup.key===d.key)api.close();else api.open('task',b,d.key,true);return true;}
    if((d.a==='view-editor'||d.a==='view-list')&&view.viewsWindow){send({a:'views-open'});return true;}
    if(d.a==='view-editor'||d.a==='view-list'){if(view.popup?.kind===(d.a==='view-editor'?'editor':'views'))api.close();else api.open(d.a==='view-editor'?'editor':'views',b);return true;}
    if(d.a==='popup-close'){if(owned)send({a:'hide'});else api.close();return true;}
    // P-W1: pinning or unpinning closes the details; the pin is a global reference, not a detail latch.
    if(d.a==='task-pin'){const t=api.live().find(t=>t.key===view.popup?.key);if(t)views.toggle(m,{ref:api.ref(t),title:t.title,url:t.url});api.close();return true;}
    if(d.a==='task-unpin'){api.unpin(d.ref);present();return true;}
    if(d.a==='pin-retry'){api.pinRead({ref:d.ref},true);present();return true;}
    if(d.a==='completed-open'){api.openCompleted();return true;}
    if(d.a==='pin-transcript'){const entry=pinReads.get(d.ref);if(entry?.value?.state==='closed'){api.openCompleted(entry);api.unpin(d.ref);present();}return true;}
    if(d.a==='view-select'){if(!owned)api.current().scroll=menu.querySelector('.content')?.scrollTop||0;m.selected=d.id;view.popup=null;if(d.id==='completed')api.openCompleted();view.restoreScroll=api.current().scroll;restore=true;present();menu.querySelector('[data-a="view-list"]')?.focus();return true;}
    if(d.a==='view-pin'){const v=m.views.find(v=>v.id===d.id);if(v)v.pinned=!v.pinned;present();return true;}
    if(d.a==='view-delete'){if(views.remove(m,d.id)){if(m.selected==='all')view.restoreScroll=api.current().scroll;present();menu.querySelector((owned?'':'#taskq-popover ')+'[data-a="view-undo"]')?.focus();}return true;}
    if(d.a==='view-undo'){views.undo(m);present();menu.querySelector((owned?'':'#taskq-popover ')+'[data-a="view-select"][aria-pressed="true"]')?.focus();return true;}
    if(d.a==='view-save-as'){view.naming=true;present();menu.querySelector('[data-view-name]')?.focus();return true;}
    if(d.a==='view-cancel'){api.cancelNaming();return true;}
    if(d.a==='view-add'){if(views.add(m,api.current().draft,'view-'+Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join(''))){view.naming=false;view.popup=null;present();menu.querySelector('[data-a="view-select"][aria-pressed="true"], [data-a="view-list"]')?.focus();}return true;}
    if(d.a==='view-facet'){const value=d.value,key=d.facet,current=api.criteria()[key];if(!['states','projects','outcomes'].includes(key))return true;api.set({[key]:!value?[]:key==='projects'?[value]:current.includes(value)?current.filter(x=>x!==value):[...current,value]});return true;}
    if(d.a==='view-group'){api.set({group:d.value});return true;}
    if(d.a==='view-period'){api.set({period:d.value});return true;}
    if(d.a==='view-save'){views.save(m);present();return true;}
    if(d.a==='view-reset'||d.a==='filter-reset'){views.reset(m);present();return true;}
    if(d.a==='all-tasks'){api.set({states:[]});return true;}
    return false;
   },
   mount:function(){
    ui.listen(menu,'focusin',e=>{if(e.target.hasAttribute('data-task-query'))view.searchOpen=true;});
    ui.listen(menu,'focusout',e=>{if(e.target.hasAttribute('data-task-query'))ui.defer(()=>{if(!api.criteria().query&&view.searchOpen&&!document.activeElement?.closest('.task-search')){view.searchOpen=false;present();}},0);});
    ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&e.target.hasAttribute?.('data-task-query')){e.preventDefault();e.stopImmediatePropagation();if(!api.criteria().query)view.searchOpen=false;present();menu.querySelector('[data-a=search-open]')?.focus({preventScroll:true});}},true);
    ui.listen(menu,'change',e=>{if(!e.target.hasAttribute('data-project'))return;const value=e.target.dataset.project,current=api.criteria().projects,selected=current.length?current:api.unique(view.S.projects).map(views.identity).filter(Boolean);api.set({projects:e.target.checked?[...selected,value]:selected.filter(x=>x!==value)});});
    ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&view.naming){e.preventDefault();e.stopImmediatePropagation();api.cancelNaming();}},true);
    ui.listen(menu,'input',e=>{if(e.target.hasAttribute('data-task-query')){api.criteria().query=e.target.value;present();}if(e.target.hasAttribute('data-view-name')){api.current().draft=e.target.value.slice(0,80);present();}});
    ui.listen(document,'pointerdown',e=>{if(view.popup&&view.popup.kind!=='task'&&!e.target.closest('#taskq-popover,#taskq-question-details,[data-a="task-detail"],[data-a="view-editor"],[data-a="view-list"]'))api.close(false);});
    ui.listen(window,'resize',api.place);ui.listen(menu,'scroll',api.place,true);
   }
  });return api;
 }
});
