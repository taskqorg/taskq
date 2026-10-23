'use strict';
const {h,Fragment}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const views=require('./task-views.js'),blocks=require('./project-blocks.js'),popover=require('./anchored-popover.js'),{icon,button}=require('./ui-icons.js');
module.exports=execution.module('taskDashboard',{
 create:function(view,{menu,present,send,app,window=globalThis,document=window.document}){
  const crypto=window.crypto||globalThis.crypto,CSS=window.CSS||globalThis.CSS;
  const api=execution.module('taskDashboard.instance',{
   model:function(){return view.dashboard||(view.dashboard=views.normalize(null,{taskFilter:view.taskFilter}));},
   current:function(){return views.current(api.model());},
   criteria:function(){return api.current().working;},
   set:function(patch){Object.assign(api.criteria(),patch);present();},
   projects:function(){return views.scope(view.S.projects,api.criteria());},
   rows:function(){return app.unique(api.projects().flatMap(app.tasks)).filter(t=>t.state!=='closed'&&views.matches(t,api.criteria().query)).sort(views.compare);},
   anchor:function(target){const r=target.getBoundingClientRect();return {left:r.left,top:r.top,bottom:r.bottom};},
   open:function(kind,target,key=null,pinned=true){view.popup={kind,key,pinned,anchor:api.anchor(target)};present();if(pinned)menu.querySelector('#taskq-popover')?.focus({preventScroll:true});},
   close:function(focus=true){const p=view.popup;if(!p)return;view.popup=null;present();if(focus){const target=p.kind==='task'?menu.querySelector('[data-a="task-detail"][data-key="'+CSS.escape(p.key)+'"]'):menu.querySelector('[data-a="'+(p.kind==='views'?'view-list':'view-editor')+'"]');view.popupClosing=true;try{target?.focus({preventScroll:true});}finally{view.popupClosing=false;}}},
   bar:function(){const m=api.model(),v=api.current(),c=v.working;
    return h(Fragment,null,h('div',{class:'viewbar'},h('div',{class:'view-picks'},h('button',{'data-a':'view-list','aria-haspopup':'dialog','aria-expanded':String(view.popup?.kind==='views')},v.name,views.dirty(v)?' *':''),m.views.filter(x=>x.pinned&&x.id!==m.selected).map(x=>h('button',{'data-a':'view-select','data-id':x.id,key:x.id},x.name))),button('sliders-horizontal','Edit view conditions',{'data-a':'view-editor','aria-haspopup':'dialog','aria-expanded':String(view.popup?.kind==='editor')})),
     h('div',{class:'view-criteria',role:'status'},c.projects.length?api.projects().map(p=>p.name).join(' + ')||'Selected projects unavailable':'All projects',' · ',c.states.length?view.S.display.states.filter(x=>c.states.includes(x.id)).map(x=>x.name).join(' + '):'All states',views.dirty(v)?' · Unsaved':''),
     h('label',{class:'task-search'},icon('search'),h('input',{type:'search','data-task-query':'',maxLength:1024,'aria-label':'Search tasks',placeholder:'Search tasks',value:c.query}),h('span',{class:'meta'},api.rows().filter(t=>!c.states.length||c.states.includes(t.state)).length)));
   },
   list:function(){const ps=api.projects(),c=api.criteria(),all=api.rows(),rows=all.filter(t=>!c.states.length||c.states.includes(t.state));
    // #1592: partial names and the deduplicated, ordered rows come from the display model; the view only filters them.
    const names=ps.map(p=>p.name),partial=[...new Set([...ps.flatMap(p=>p.partial||[]),...view.S.display.all.partial.filter(n=>names.includes(n))])],missing=c.projects.filter(id=>!view.S.projects.some(p=>views.identity(p)===id));
    if(missing.length)partial.push('Selected projects unavailable');
    const counts={counts:view.S.display.states.map(({id,name})=>({state:id,name,n:partial.length?null:all.filter(t=>t.state===id).length})),open:partial.length?null:all.length};
    const listing=c.group==='combined'?rows.map(t=>app.taskRow(t,true)):ps.filter((p,i)=>ps.findIndex(q=>views.identity(q)===views.identity(p))===i).map(p=>{
     const group=rows.filter(t=>t.boardIdentity===views.identity(p));return group.length?h('section',{class:'all-tasks-project',key:p.issues},h('div',{class:'project-group'},h('button',{'data-route':p.root},icon('folder'),p.name),h('a',{href:p.issues,'data-url':p.issues,title:'Open '+p.name+' Board','aria-label':'Open '+p.name+' Board'},icon(p.board_kind))),group.map(t=>app.taskRow(t,false))):null;
    });
    return h(Fragment,null,api.bar(),h('header',{class:'filter-header tqp',dangerouslySetInnerHTML:{__html:blocks.counters(counts.counts,'','Selected projects',partial,c.states,counts.open)}}),partial.length>0&&h('p',{class:'meta',role:'status'},'Partial: refresh the affected project or open its Board.'),rows.filter(t=>t.state==='held').length>1&&h('div',{class:'row'},app.startButton(rows)),app.answerNotices(),rows.length?listing:h('div',{class:'body meta'},partial.length?'Tasks unknown':'No matching tasks',' ',h('button',{'data-a':'filter-reset'},'Reset')));
   },
   editor:function(){const c=api.criteria(),v=api.current();return h(Fragment,null,h('div',{class:'popover-heading'},h('strong',null,'View'),button('x','Close view editor',{'data-a':'popup-close'})),
    h('fieldset',null,h('legend',null,'Projects'),h('div',{class:'facet-list'},h('button',{'data-a':'view-facet','data-facet':'projects','data-value':'','aria-pressed':String(!c.projects.length)},'All projects'),view.S.projects.filter((p,i)=>view.S.projects.findIndex(q=>views.identity(q)===views.identity(p))===i).map(p=>h('button',{'data-a':'view-facet','data-facet':'projects','data-value':views.identity(p),disabled:!views.identity(p),'aria-pressed':String(c.projects.includes(views.identity(p)))},icon('folder'),p.name)))),
    h('fieldset',null,h('legend',null,'States'),h('div',{class:'facet-list'},h('button',{'data-a':'all-tasks','aria-pressed':String(!c.states.length)},'All states'),view.S.display.states.map(({id:s,name:n})=>h('button',{'data-a':'view-facet','data-facet':'states','data-value':s,'aria-pressed':String(c.states.includes(s))},icon(blocks.marks[s][0]),n)))),
    h('fieldset',null,h('legend',null,'Display'),h('div',{class:'options'},[['combined','All together'],['project','By project']].map(([value,label])=>h('button',{'data-a':'view-group','data-value':value,'aria-pressed':String(c.group===value)},label)))),
    h('p',{class:'meta'},'State order · priority · issue number · repository'),h('div',{class:'options'},h('button',{'data-a':'view-save',disabled:!views.dirty(v)},icon('save'),'Save'),h('button',{'data-a':'view-reset'},icon('rotate-ccw'),'Reset'),h('button',{'data-a':'popup-close'},'Done')));
   },
   presets:function(){const m=api.model();return h(Fragment,null,h('div',{class:'popover-heading'},h('strong',null,'Saved views'),button('x','Close saved views',{'data-a':'popup-close'})),m.views.map(v=>h('div',{class:'saved-view',key:v.id},h('button',{'data-a':'view-select','data-id':v.id,'aria-pressed':String(v.id===m.selected)},v.name,views.dirty(v)?' *':''),button(v.pinned?'pin':'pin-off',(v.pinned?'Unpin ':'Pin ')+v.name,{'data-a':'view-pin','data-id':v.id,'aria-pressed':String(v.pinned)}))),h('label',null,'New view',h('input',{'data-view-name':'',value:view.newViewName||'',maxLength:80,placeholder:'View name'})),h('button',{'data-a':'view-add',disabled:!view.newViewName?.trim()||m.views.length>=64},icon('plus'),'Save as new view'),view.viewError&&h('p',{role:'alert'},view.viewError));},
   detail:function(){const t=app.unique(view.S.projects.flatMap(app.tasks)).find(t=>t.key===view.popup.key);if(!t)return h(Fragment,null,h('p',{role:'status'},'Task no longer available; no action submitted.'),h('button',{'data-a':'popup-close'},'Close'));
    const p=view.S.projects.find(p=>p.root===t.root);
    return h(Fragment,null,h('div',{class:'popover-heading'},h('strong',null,p.name+' #'+t.n),button(view.popup.pinned?'pin-off':'pin','Keep task details open',{'data-a':'popup-pin','aria-pressed':String(view.popup.pinned)}),button('x','Close task details',{'data-a':'popup-close'})),h('a',{class:'detail-title',href:t.url,'data-url':t.url},t.title),
     h('div',{class:'body meta'},t.state_name+' · '+(t.age??'elapsed unknown'),' · ',t.runtime||'runtime unknown',' · ',t.host||'host unknown'),t.wait&&h('p',{role:'status'},t.wait),
     h('div',{class:'body meta'},'Total age: ',t.total_age??'unknown'),
     h('details',{'data-view-key':'steps:'+t.key},h('summary',null,'Steps · ',Array.isArray(t.steps)?t.steps.filter(s=>s.complete===true).length+' completed':'unknown'),Array.isArray(t.steps)&&t.steps.length?h('ol',{class:'task-steps'},t.steps.map(s=>h('li',null,s.state,' · ',s.age??'duration unknown',s.complete?'':' · current'))):h('p',{class:'meta'},'Ordered stage history unavailable in this snapshot.')),
     h('div',{class:'body meta'},'Attempt: '+(t.attempt||'unknown')+' · revision '+(t.revision??'unknown')),t.failed&&h('p',{class:'alert'},t.failed),t.card&&h('p',{class:'task-card-text'},t.card),
     (t.blocked||[]).map(b=>h('p',null,h('a',{href:b.url,'data-url':b.url},b.ref),' · '+b.reason)),
     h('div',{class:'row-actions'},app.answerSlot(t),t.state==='held'&&app.startButton([t]),h('span',{dangerouslySetInnerHTML:{__html:require('./task-controls.js').render(t,{key:t.key,url:t.url,except:['start']})}})),
     t.session_url&&h('a',{href:t.session_url,'data-url':t.session_url},icon('terminal'),'Open '+(t.runtime||'agent')+' session'),!t.session_url&&h('p',{class:'meta'},t.session_reason||'Session unavailable'),app.answerNotices(p.issues));
   },
   popup:function(){const p=view.popup;if(!p)return null;return popover.render({label:p.kind==='task'?'Task details':p.kind==='editor'?'View editor':'Saved views',children:p.kind==='task'?api.detail():p.kind==='editor'?api.editor():api.presets()});},
   place:function(){if(view.popup){const selector=view.popup.kind==='task'?'[data-a="task-detail"][data-key="'+CSS.escape(view.popup.key)+'"]':'[data-a="'+(view.popup.kind==='views'?'view-list':'view-editor')+'"]',trigger=menu.querySelector(selector);if(trigger)view.popup.anchor=api.anchor(trigger);}for(const b of menu.querySelectorAll('[data-a="task-detail"]'))b.setAttribute('aria-expanded',String(view.popup?.kind==='task'&&view.popup.key===b.dataset.key));if(view.popup)popover.place(menu.querySelector('#taskq-popover'),view.popup.anchor);},
   click:function(e){const b=e.target.closest('button');if(!b||b.disabled)return false;const d=b.dataset,m=api.model();
    if(d.a==='task-detail'){api.open('task',b,d.key,true);return true;}
    if(d.a==='view-editor'||d.a==='view-list'){if(view.popup?.kind===(d.a==='view-editor'?'editor':'views'))api.close();else api.open(d.a==='view-editor'?'editor':'views',b);return true;}
    if(d.a==='popup-close'){api.close();return true;}
    if(d.a==='popup-pin'){view.popup.pinned=!view.popup.pinned;present();return true;}
    if(d.a==='view-select'){api.current().scroll=menu.querySelector('.content')?.scrollTop||0;m.selected=d.id;view.popup=null;view.restoreScroll=api.current().scroll;present();menu.querySelector('[data-a="view-list"]')?.focus();return true;}
    if(d.a==='view-pin'){const v=m.views.find(v=>v.id===d.id);if(v)v.pinned=!v.pinned;present();return true;}
    if(d.a==='view-add'){if(views.add(m,view.newViewName||'','view-'+Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join(''))){view.newViewName='';view.popup=null;present();}return true;}
    if(d.a==='view-facet'){const value=d.value,key=d.facet,current=api.criteria()[key];if(!['states','projects'].includes(key))return true;api.set({[key]:!value?[]:current.includes(value)?current.filter(x=>x!==value):[...current,value]});return true;}
    if(d.a==='view-group'){api.set({group:d.value});return true;}
    if(d.a==='view-save'){views.save(m);present();return true;}
    if(d.a==='view-reset'||d.a==='filter-reset'){views.reset(m);present();return true;}
    if(d.a==='all-tasks'){api.set({states:[]});return true;}
    return false;
   },
   mount:function(){
    ui.listen(menu,'input',e=>{if(e.target.hasAttribute('data-task-query')){api.criteria().query=e.target.value;present();}if(e.target.hasAttribute('data-view-name')){view.newViewName=e.target.value;present();}});
    ui.listen(menu,'focusin',e=>{const b=e.target.closest('[data-a="task-detail"]');if(b&&!view.popup&&!view.popupClosing)api.open('task',b,b.dataset.key,false);});
    ui.listen(menu,'pointerover',e=>{const b=e.target.closest('[data-a="task-detail"]');if(b&&!b.contains(e.relatedTarget)&&!view.popup)api.open('task',b,b.dataset.key,false);});
    ui.listen(menu,'pointerout',e=>{if(view.popup?.kind!=='task'||view.popup.pinned)return;const related=e.relatedTarget;if(related?.closest?.('#taskq-popover,[data-a="task-detail"]'))return;ui.defer(()=>{if(view.popup?.kind==='task'&&!view.popup.pinned&&!menu.querySelector('#taskq-popover')?.matches(':hover')&&!menu.querySelector('[data-a="task-detail"]:hover')&&!menu.querySelector('#taskq-popover')?.contains(document.activeElement))api.close(false);},240);});
    ui.listen(document,'pointerdown',e=>{if(view.popup&&!e.target.closest('#taskq-popover,[data-a="task-detail"],[data-a="view-editor"],[data-a="view-list"]'))api.close(false);});
    ui.listen(window,'resize',api.place);ui.listen(menu,'scroll',api.place,true);
   }
  });return api;
 }
});
