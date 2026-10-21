'use strict';
const ui=require('./ui-runtime.js'),shell=require('./app-shell.js'),{signal}=require('@preact/signals'),{execution}=ui;
const {create}=execution.module('workspace',{
 create:function(window=globalThis){
  const document=window.document,menu=document.querySelector('.menu'),renderer=shell.create(menu),snapshot=signal(null);
  const view={page:'messages',taskFilter:null,working:null,aiRoot:null,disclosures:{},restoreScroll:0},trail=[];
  const routes={messages:{title:'Messages'},sessions:{title:'Work sessions',parent:'messages'},agents:{title:'Agent settings',parent:'messages'}};
  const state=require('./tray-view-state.js').create(view,{menu,document,routes,trail,root:'messages',defer:ui.defer,cancel:ui.cancel,send:m=>ui.send({...m,a:'workspace-set'})});
  const api=execution.module('workspace.instance',{
   draw:function(){
    const s=snapshot.value;if(!s)return;
    const title=routes[view.page]?.title||'Unavailable',scroll=view.restoreScroll??menu.querySelector('.content')?.scrollTop??0,active=document.activeElement;
    document.documentElement.style.colorScheme=s.theme||'light dark';menu.style.setProperty('--size',(s.font||16)+'px');
    const actions='<details class="shell-overflow" data-view-key="navigation-actions"><summary title="Workspace navigation" aria-label="Workspace navigation">⋯</summary><nav class="overflow-actions" aria-label="Workspace"><button data-route="messages">Messages</button><button data-route="sessions">Work sessions</button><button data-route="agents">Agent settings</button></nav></details>';
    const body=view.page==='messages'?'<p>The workspace shell is ready. Messages and task answers are not available in this version. Open the task’s Board from the tray to answer it.</p><div class="options"><button data-route="sessions">Work sessions</button><button data-route="agents">Agent settings</button></div>':view.page==='sessions'?'<p>Work sessions are not available here. Use the tray’s Agents view and the recorded native session links.</p>':'<p>Agent settings are not available here. Use Settings → Agents in the tray. Opening this screen changes no runtime or permissions.</p>';
    renderer.draw(shell.bar({title,back:view.page!=='messages',actions,windows:'<button data-a="hide" title="Close workspace" aria-label="Close workspace">×</button>'})+'<main class="content"><div class="content-body">'+(view.unavailable?'<p role="status">'+shell.escape(view.unavailable)+'</p>':'')+body+(view.error?'<p role="alert">'+shell.escape(view.error)+'</p>':'')+'</div></main>','/'+view.page);
    state.restore(view.disclosures);menu.querySelector('.content').scrollTop=scroll;view.restoreScroll=null;
    if(active?.isConnected)active.focus({preventScroll:true});state.flush();
   },
   navigate:function(page){trail.push(state.capture());if(trail.length>32)trail.shift();view.page=page;view.unavailable=null;view.disclosures={};view.restoreScroll=0;api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');menu.querySelector('h1').focus();},
   back:function(){const prior=trail.pop()||{page:'messages',open:{},scroll:0};state.apply(prior);api.draw();menu.querySelector('.shell-overflow')?.removeAttribute('open');state.focus(prior.focus);},
   receive:function(s){
    if(s.diagnose){ui.execution.diagnose(s.diagnose.selector,Math.max(0,Math.min(60000,s.diagnose.until_ms-Date.now())),s.diagnose.limit);return;}
    if(s.viewStateError!==undefined){view.error=s.viewStateError;api.draw();return;}
    const first=snapshot.value===null;snapshot.value=s;view.S={...s,projects:[]};state.hydrate({...view.S,navigation:s.workspace_navigation});api.draw();if(first)state.focus(view.restoreFocus);
   },
   mount:function(){
    ui.connect();window.taskq={receive:api.receive};
    ui.listen(menu,'click',e=>{const b=e.target.closest('button');if(!b||b.disabled)return;if(b.dataset.route)api.navigate(b.dataset.route);else if(b.dataset.a==='back')api.back();else if(b.dataset.a==='hide'){state.flush();ui.send({a:'hide'});}});
    ui.listen(menu,'scroll',()=>state.schedule(),true);
    ui.listen(menu,'toggle',()=>state.schedule(),true);
    ui.listen(document,'keydown',e=>{if(e.key==='Escape'){const overflow=menu.querySelector('.shell-overflow[open]');if(overflow){overflow.open=false;overflow.querySelector('summary').focus();return;}state.flush();ui.send({a:'hide'});}if(e.altKey&&e.key==='ArrowLeft'&&view.page!=='messages'){e.preventDefault();api.back();}});
    ui.send({a:'ready'});
   }
  });
  return api;
 }
});
module.exports={create};
