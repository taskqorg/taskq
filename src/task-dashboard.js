'use strict';
const {h,Fragment}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const views=require('./task-views.js'),blocks=require('./project-blocks.js'),popover=require('./anchored-popover.js'),{icon,button}=require('./ui-icons.js');
// MP14: closure outcome labels; only a success with a sourced merge receipt reads Merged (git-merge, Review purple).
const outcomes={success:['circle-check','','Completed'],not_planned:['circle-slash','','Not planned'],unknown:['circle-question-mark','','Outcome unknown']};
module.exports=execution.module('taskDashboard',{
 // P-W4: `owned` is the View window surface; the menu sends `views-open` when native offers that window, else keeps the overlay.
 create:function(view,{menu,present,send,app,owned=false,window=globalThis,document=window.document}){
  const crypto=window.crypto||globalThis.crypto,CSS=window.CSS||globalThis.CSS;let restore=true,requests=0;
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
   anchor:function(target){const r=target.getBoundingClientRect();return {left:r.left,top:r.top,bottom:r.bottom};},
   // MP13: details open only from click, Enter or Space on the identity (native button activation); `focus` moves focus into them.
   open:function(kind,target,key=null,focus=true){view.popup={kind,key,anchor:api.anchor(target)};if(kind==='task')api.current().open=key;present();if(focus)menu.querySelector('#taskq-question-details,#taskq-popover')?.focus({preventScroll:true});},
   close:function(focus=true){const p=view.popup;if(!p)return;view.popup=null;if(p.kind==='task')api.current().open=null;present();if(focus){const target=p.kind==='task'?menu.querySelector('[data-a="task-detail"][data-key="'+CSS.escape(p.key)+'"]'):menu.querySelector('[data-a="'+(p.kind==='views'?'view-list':'view-editor')+'"]');target?.focus({preventScroll:true});}},
   // M2: collapse is presentation only; the name and the two adjacent shortcuts keep independent targets.
   projectHeader:function(p,shown,total,reason){const board=p.board_kind==='gitlab'?'GitLab':'GitHub',id=views.identity(p),open=!api.model().collapsed.includes(id),controls='project-tasks-'+encodeURIComponent(id),folder=typeof p.root==='string'&&!!p.root.trim();
    return h('header',{class:'project-group project-shortcuts'},button('arrow-down',(open?'Collapse ':'Expand ')+p.name+' tasks',{'data-a':'project-collapse','data-project':id,'aria-expanded':String(open),'aria-controls':controls,class:'compact-action project-collapse'}),
     h('div',{class:'project-heading'},h('h2',null,h('button',{class:'project-overview','data-route':p.root,title:'Open '+p.name+' project overview'},p.name)),h('span',{class:'project-counts',title:reason||'Matches the current filter; total open tasks','aria-label':reason?'Matching and total task counts unknown: '+reason:shown+' tasks match the current filter; '+total+' open tasks in total'},h('span',null,(reason?'?':shown)+' shown'),h('span',null,' · '+(reason?'?':total)+' total'))),
     h('div',{class:'project-actions'},h('a',{class:'project-board compact-action',href:p.issues,'data-url':p.issues,title:'Open '+p.name+' board on '+board,'aria-label':'Open '+p.name+' board on '+board},icon(p.board_kind)),button('folder',folder?'Open '+p.name+' project folder on this computer':'Project folder unavailable: registered root missing',{'data-a':'folder','data-root':p.root,disabled:!folder,class:'compact-action project-folder'})));
   },
   bar:function(){const m=api.model(),v=api.current(),c=v.working,searching=!!c.query||!!view.searchOpen;
    return h(Fragment,null,h('div',{class:'viewbar'},h('div',{class:'view-picks'},h('button',{'data-a':'view-list','aria-haspopup':'dialog','aria-expanded':String(view.popup?.kind==='views')},v.name,views.dirty(v)?' *':''),m.views.filter(x=>x.pinned&&x.id!==m.selected).map(x=>h('button',{'data-a':'view-select','data-id':x.id,key:x.id},x.name))),views.dirty(v)&&button('rotate-ccw','Reset '+v.name+' to its saved criteria',{'data-a':'view-reset'}),button('search','Search tasks',{'data-a':'search-open','aria-controls':'task-query','aria-expanded':String(searching)}),button('sliders-horizontal','Edit view conditions',{'data-a':'view-editor','aria-haspopup':'dialog','aria-expanded':String(view.popup?.kind==='editor')})),
     // #1775: search is one icon until opened; a query keeps the field visible
     searching&&h('div',{class:'task-search'},icon('search'),h('input',{id:'task-query',type:'search','data-task-query':'',maxLength:1024,'aria-label':'Search tasks',placeholder:'Search tasks',value:c.query}),c.query&&button('x','Clear task search',{'data-a':'search-clear'})));
   },
   // P-W1: global pins once, first, as ordinary rows in the one shared scroll; unavailable references keep a sourced outcome or explicit unknown.
   pins:function(){const m=api.model();if(!m.pins.length)return null;const live=api.live(),done=api.history().rows;
    return h('section',{class:'task-pins','aria-label':'Pinned tasks'},h('h2',null,icon('pin'),' Pinned'),m.pins.map(pin=>{const t=live.find(t=>api.ref(t)===pin.ref);if(t)return app.taskRow(t,true);const c=done.find(c=>c.ref===pin.ref);
     return h('div',{class:'task-row completed-row',key:pin.ref,'data-pin':pin.ref},icon(c?api.outcome(c)[0]:'circle-question-mark'),h('a',{class:'detail-title',href:pin.url,'data-url':pin.url},pin.title||pin.ref),h('div',{class:'meta'},c?api.outcome(c)[2]:'Unavailable · state unknown',' ',button('pin-off','Unpin '+(pin.title||pin.ref),{'data-a':'task-unpin','data-ref':pin.ref})));}));
   },
   list:function(){if(api.current().id==='completed')return h(Fragment,null,api.pins(),api.bar(),api.completed());
    const ps=api.projects(),c=api.criteria(),all=api.rows(),pinned=new Set(api.model().pins.map(p=>p.ref)),matching=all.filter(t=>!c.states.length||c.states.includes(t.state)),rows=matching.filter(t=>!pinned.has(api.ref(t))),live=api.live();
    // #1592: partial names and the deduplicated, ordered rows come from the display model; the view only filters them.
    const names=ps.map(p=>p.name),partial=[...new Set([...ps.flatMap(p=>p.partial||[]),...view.S.display.all.partial.filter(n=>names.includes(n))])],missing=c.projects.filter(id=>!view.S.projects.some(p=>views.identity(p)===id));
    if(missing.length)partial.push('Selected projects unavailable');
    const counts={counts:view.S.display.states.map(({id,name})=>({state:id,name,n:partial.length?null:all.filter(t=>t.state===id).length})),open:partial.length?null:all.length};
    const listing=c.group==='combined'?rows.map(t=>app.taskRow(t,true)):api.unique(ps).map(p=>{
     const id=views.identity(p),group=rows.filter(t=>t.boardIdentity===id),sources=ps.filter(q=>views.identity(q)===id),reason=sources.flatMap(q=>[...(q.partial||[]),...(q.complete!==true||q.tasks_live===false?[q.error||'Project snapshot incomplete or stale']:[]),...(view.S.display.all.partial.includes(q.name)?['Project task data incomplete']:[])]).join('; ');
     return group.length?h('section',{class:'all-tasks-project',key:id},api.projectHeader(p,matching.filter(t=>t.boardIdentity===id).length,live.filter(t=>t.boardIdentity===id).length,reason),h('div',{class:'project-tasks',id:'project-tasks-'+encodeURIComponent(id),hidden:api.model().collapsed.includes(id)},group.map(t=>app.taskRow(t,false)))):null;
    });
    return h(Fragment,null,api.pins(),api.bar(),h('header',{class:'filter-header tqp'},h('div',{class:'state-filters',dangerouslySetInnerHTML:{__html:blocks.counters(counts.counts,'','Selected projects',partial,c.states,counts.open)}}),rows.filter(t=>t.state==='held').length>1&&h('span',{class:'held-shortcut'},app.startButton(rows))),partial.length>0&&h('p',{class:'meta',role:'status'},'Partial: refresh the affected project or open its Board.'),app.answerNotices(),rows.length?listing:h('div',{class:'body meta'},partial.length?'Tasks unknown':'No matching tasks',' ',h('button',{'data-a':'filter-reset'},'Reset')));
   },
   // MP14: Completed reads board history pages on demand through the tray (`taskq status --closed`); nothing is stored.
   since:function(period){const days={'7d':7,'30d':30}[period];return days&&Number.isFinite(view.S.now)?view.S.now-days*86400:null;},
   request:function(e){e.loading=true;e.reason=null;e.id=String(++requests);const id=e.id;
    e.timer=ui.defer(()=>api.receive({root:e.root,id,reason:'No history response within 65 seconds'}),65000);
    send({a:'completed',root:e.root,id,page:e.page+1,...(e.since==null?{}:{since:e.since})});},
   history:function(){const c=api.criteria(),key=JSON.stringify([c.projects,c.period]),prior=view.completed;
    if(api.current().id==='completed'&&prior?.key!==key){const since=api.since(c.period);prior?.entries.forEach(e=>ui.cancel(e.timer));
     view.completed={key,entries:api.unique(api.projects()).map(p=>{const old=prior?.entries.find(e=>e.identity===views.identity(p));return {root:p.root,name:p.name,kind:p.board_kind,identity:views.identity(p),page:0,more:true,tasks:old?.tasks||[],shown:old?.shown||[],replace:true,since};})};view.completed.entries.forEach(api.request);}
    const entries=view.completed?.entries||[];return {entries,rows:entries.flatMap(e=>e.tasks.map(t=>({...t,project:e.name,kind:e.kind,ref:e.identity+'#'+t.n})))};
   },
   receive:function(page){const e=view.completed?.entries.find(e=>e.id===page.id&&e.root===page.root);if(!e?.loading)return;ui.cancel(e.timer);e.loading=false;
    if(page.reason)e.reason=page.reason;
    else{if(e.replace){e.tasks=[];e.replace=false;}e.page=page.page||e.page+1;e.more=page.more===true;e.tasks.push(...(page.tasks||[]).filter(t=>!e.tasks.some(x=>x.n===t.n)));}present();},
   outcome:function(t){return t.outcome==='success'&&typeof t.merged==='string'&&t.merged?['git-merge','review','Merged']:outcomes[t.outcome]||outcomes.unknown;},
   completed:function(){const c=api.criteria(),{entries}=api.history(),pinned=new Set(api.model().pins.map(p=>p.ref));
    const shown=entries.flatMap(e=>{if(!e.replace)e.shown=e.tasks.filter(t=>views.matches(t,c.query)&&(!c.outcomes.length||c.outcomes.includes(t.outcome in outcomes?t.outcome:'unknown')));
     return e.shown.map(t=>({...t,project:e.name,kind:e.kind,ref:e.identity+'#'+t.n}));}).filter(t=>!pinned.has(t.ref)).sort((a,b)=>(b.closed??0)-(a.closed??0)||b.n-a.n);
    const loading=entries.some(e=>e.loading),more=entries.filter(e=>e.more&&!e.loading&&!e.reason);
    return h(Fragment,null,shown.map(t=>{const [mark,tone,label]=api.outcome(t);return h('div',{class:'task-row completed-row',key:t.ref,'data-completed':t.ref},h('span',{class:tone,title:label,'aria-label':label},icon(mark)),h('a',{class:'detail-title',href:t.url,'data-url':t.url},t.title),
      h('div',{class:'meta'},icon(t.kind),' ',t.project+' #'+t.n,' · ',label,' · ',Number.isFinite(t.closed)?new Date(t.closed*1000).toISOString().slice(0,10):'closed date unknown',t.result?' · '+t.result:'',t.pr&&h(Fragment,null,' · ',h('a',{href:t.pr,'data-url':t.pr},icon('git-pull-request'),'PR'))));}),
     entries.map(e=>h('p',{class:'meta',role:e.reason?'alert':'status',key:'source'+e.identity,'data-history-source':e.identity},e.name+': '+(e.loading?'Reading page '+(e.page+1)+'…':e.reason?'History unavailable · '+e.reason:'Page '+e.page+' loaded · '+e.tasks.length+' rows'+(e.more?' · more available':'')),
      e.replace&&e.shown.length>0?' · Showing previous rows':'',e.reason&&h('button',{'data-a':'completed-retry','data-root':e.root,'aria-label':'Retry '+e.name+' history'},'Retry'))),
     !loading&&!shown.length&&h('div',{class:'body meta'},entries.some(e=>e.reason)?'Completed history incomplete':entries.length?'No completed tasks match':'No projects selected'),
     more.length>0&&h('button',{class:'completed-more','data-a':'completed-more'},'Load more'));
   },
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
   // M3: questions share the menu's scroll; complete wording and choices stay in the task thread.
   // #1781: an ask row shows its amber question glyph (project-blocks) and any ask images (#1782); open details stay inline in the scroll flow.
   attention:function(t){if(t.state!=='ask')return null;const im=api.images(t.question),open=view.popup?.kind==='task'&&view.popup.key===t.key;return (im||open)?h(Fragment,null,im,open&&h('section',{id:'taskq-question-details',class:'question-task-details',tabindex:-1,'aria-label':'Task details for #'+t.n},api.detail())):null;},
   detail:function(){const t=api.live().find(t=>t.key===view.popup.key);if(!t)return h(Fragment,null,h('p',{role:'status'},'Task no longer available; no action submitted.'),h('button',{'data-a':'popup-close'},'Close'));
    const p=view.S.projects.find(p=>p.root===t.root),pinned=views.pinned(api.model(),api.ref(t));
    return h(Fragment,null,h('div',{class:'popover-heading'},h('strong',null,p.name+' #'+t.n),button(pinned?'pin-off':'pin',pinned?'Unpin task':'Pin task',{'data-a':'task-pin','aria-pressed':String(pinned)}),button('x','Close task details',{'data-a':'popup-close'})),h('a',{class:'detail-title',href:t.url,'data-url':t.url},t.title),
     h('div',{class:'body meta'},h('span',{class:blocks.marks[t.state]?.[1]||null},t.state_name||t.state),' · '+(t.age??'elapsed unknown')),t.wait&&h('p',{role:'status'},t.wait),t.failed&&h('p',{class:'alert'},t.failed),api.question(t),
     (t.blocked||[]).map(b=>h('p',null,h('a',{href:b.url,'data-url':b.url},b.ref),' · '+b.reason)),
     h('div',{class:'row-actions'},app.answerSlot(t,true),t.state==='held'&&app.startButton([t]),h('span',{dangerouslySetInnerHTML:{__html:require('./task-controls.js').render(t,{key:t.key,url:t.url,except:['start']})}})),
     h('details',{class:'task-more','data-view-key':'more:'+t.key},h('summary',null,icon('info'),'Task info'),h('dl',null,h('dt',null,'Runtime'),h('dd',null,t.runtime||'unknown'),h('dt',null,'Host'),h('dd',null,t.host||'unknown'),h('dt',null,'Session'),h('dd',null,t.session_url?h('a',{href:t.session_url,'data-url':t.session_url},icon('terminal'),'Open '+(t.runtime||'agent')+' session'):h('button',{disabled:true,title:t.session_reason||'Session unavailable'},t.session_reason||'Session unavailable')),
      h('dt',null,'Attempt'),h('dd',null,t.attempt||'unknown'),h('dt',null,'Revision'),h('dd',null,t.revision??'unknown'),h('dt',null,'Total age'),h('dd',null,t.total_age??'unknown'),h('dt',null,'Steps'),h('dd',null,Array.isArray(t.steps)?t.steps.filter(s=>s.complete===true).length+' completed':'unknown')),
      Array.isArray(t.steps)&&t.steps.length?h('ol',{class:'task-steps'},t.steps.map(s=>h('li',null,s.state,' · ',s.age??'duration unknown',s.complete?'':' · current'))):h('p',{class:'meta'},'Ordered stage history unavailable in this snapshot.')),
     app.answerNotices(p.issues));
   },
   // P-W4: the menu takes the View window's model whole; a switched view restores its own scroll and open task.
   adopt:function(value){const prior=view.dashboard?.selected;view.dashboard=views.normalize(value);if(prior!==view.dashboard.selected){view.restoreScroll=api.current().scroll;restore=true;}present();},
   popup:function(){const p=view.popup;if(!p||p.kind==='task'&&api.live().some(t=>t.key===p.key&&t.state==='ask'))return null;return popover.render({label:p.kind==='task'?'Task details':p.kind==='editor'?'View editor':'Saved views',children:p.kind==='task'?api.detail():p.kind==='editor'?api.editor():api.presets()});},
   place:function(){
    // A3: the view's open task returns on view switch and restart, never on a data refresh.
    if(restore&&view.S&&view.page==='home'){restore=false;const key=api.current().open,trigger=key&&menu.querySelector('[data-a="task-detail"][data-key="'+CSS.escape(key)+'"]');if(!view.popup&&trigger){api.open('task',trigger,key,false);return;}}
    if(view.popup){const selector=view.popup.kind==='task'?'[data-a="task-detail"][data-key="'+CSS.escape(view.popup.key)+'"]':'[data-a="'+(view.popup.kind==='views'?'view-list':'view-editor')+'"]',trigger=menu.querySelector(selector);if(trigger)view.popup.anchor=api.anchor(trigger);}for(const b of menu.querySelectorAll('[data-a="task-detail"]')){const inline=!!b.closest('[data-card]')?.querySelector('.question-shortcut');b.setAttribute('aria-expanded',String(view.popup?.kind==='task'&&view.popup.key===b.dataset.key));b.setAttribute('aria-controls',inline?'taskq-question-details':'taskq-popover');if(inline)b.removeAttribute('aria-haspopup');}if(view.popup)popover.place(menu.querySelector('#taskq-popover'),view.popup.anchor);},
   click:function(e){const b=e.target.closest('button');if(!b||b.disabled)return false;const d=b.dataset,m=api.model();if(d.a==='search-open'){view.searchOpen=!!api.criteria().query||!view.searchOpen;present();menu.querySelector(view.searchOpen?'[data-task-query]':'[data-a=search-open]')?.focus({preventScroll:true});return true;}if(d.a==='search-clear'){api.criteria().query='';view.searchOpen=false;present();menu.querySelector('[data-a=search-open]')?.focus({preventScroll:true});return true;}
    if(d.a==='project-collapse'){const closing=!m.collapsed.includes(d.project);m.collapsed=closing?[...m.collapsed,d.project]:m.collapsed.filter(id=>id!==d.project);if(closing&&view.popup?.kind==='task'&&api.live().some(t=>t.key===view.popup.key&&t.boardIdentity===d.project))api.close(false);else present();return true;}
    if(d.a==='task-detail'){if(view.popup?.kind==='task'&&view.popup.key===d.key)api.close();else api.open('task',b,d.key,true);return true;}
    if((d.a==='view-editor'||d.a==='view-list')&&view.viewsWindow){send({a:'views-open'});return true;}
    if(d.a==='view-editor'||d.a==='view-list'){if(view.popup?.kind===(d.a==='view-editor'?'editor':'views'))api.close();else api.open(d.a==='view-editor'?'editor':'views',b);return true;}
    if(d.a==='popup-close'){if(owned)send({a:'hide'});else api.close();return true;}
    // P-W1: pinning or unpinning closes the details; the pin is a global reference, not a detail latch.
    if(d.a==='task-pin'){const t=api.live().find(t=>t.key===view.popup?.key);if(t)views.toggle(m,{ref:api.ref(t),title:t.title,url:t.url});api.close();return true;}
    if(d.a==='task-unpin'){views.toggle(m,{ref:d.ref});present();return true;}
    if(d.a==='view-select'){if(!owned)api.current().scroll=menu.querySelector('.content')?.scrollTop||0;m.selected=d.id;view.popup=null;view.restoreScroll=api.current().scroll;restore=true;present();menu.querySelector('[data-a="view-list"]')?.focus();return true;}
    if(d.a==='view-pin'){const v=m.views.find(v=>v.id===d.id);if(v)v.pinned=!v.pinned;present();return true;}
    if(d.a==='view-delete'){if(views.remove(m,d.id)){if(m.selected==='all')view.restoreScroll=api.current().scroll;present();menu.querySelector((owned?'':'#taskq-popover ')+'[data-a="view-undo"]')?.focus();}return true;}
    if(d.a==='view-undo'){views.undo(m);present();menu.querySelector((owned?'':'#taskq-popover ')+'[data-a="view-select"][aria-pressed="true"]')?.focus();return true;}
    if(d.a==='view-save-as'){view.naming=true;present();menu.querySelector('[data-view-name]')?.focus();return true;}
    if(d.a==='view-cancel'){api.cancelNaming();return true;}
    if(d.a==='view-add'){if(views.add(m,api.current().draft,'view-'+Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join(''))){view.naming=false;view.popup=null;present();menu.querySelector('[data-a="view-select"][aria-pressed="true"], [data-a="view-list"]')?.focus();}return true;}
    if(d.a==='view-facet'){const value=d.value,key=d.facet,current=api.criteria()[key];if(!['states','projects','outcomes'].includes(key))return true;api.set({[key]:!value?[]:key==='projects'?[value]:current.includes(value)?current.filter(x=>x!==value):[...current,value]});return true;}
    if(d.a==='view-group'){api.set({group:d.value});return true;}
    if(d.a==='view-period'){api.set({period:d.value});return true;}
    if(d.a==='completed-retry'){const entry=view.completed?.entries.find(e=>e.root===d.root&&e.reason&&!e.loading);if(entry){api.request(entry);present();}return true;}
    if(d.a==='completed-more'){view.completed?.entries.filter(e=>e.more&&!e.loading&&!e.reason).forEach(api.request);present();return true;}
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
    ui.listen(document,'pointerdown',e=>{if(view.popup&&!e.target.closest('#taskq-popover,#taskq-question-details,[data-a="task-detail"],[data-a="view-editor"],[data-a="view-list"]'))api.close(false);});
    ui.listen(window,'resize',api.place);ui.listen(menu,'scroll',api.place,true);
   }
  });return api;
 }
});
