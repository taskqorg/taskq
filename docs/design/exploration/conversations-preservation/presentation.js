// Review presentation only: retain props, handlers, identities and child controls from the source renderer.
'use strict';
const {h,cloneElement}=require('preact'),icons=require('./ui-icons.js');
const flat=x=>Array.isArray(x)?x.flatMap(flat):x==null||x===false?[]:[x];
const text=v=>Array.isArray(v)?v.map(text).join(' '):v?.props?text(v.props.children):typeof v==='string'||typeof v==='number'?String(v):'';
const strip=v=>Array.isArray(v)?v.map(strip):v?.props?v.type==='details'?h('section',{class:'inspector-section'},h('h3',null,v.props.class==='dialogue-launch'?'Session':text(flat(v.props.children)[0])),flat(v.props.children).slice(1).map(strip)):cloneElement(v,{},strip(v.props.children)):v;
const inspector=(label,content,key)=>h('details',{class:'context-inspector','data-view-key':key},h('summary',{title:label,'aria-label':label},icons.icon('info'),h('span',null,label)),h('div',{class:'inspector-body'},strip(content)));
function tidy(v,inside=false){if(Array.isArray(v))return v.map(x=>tidy(x,inside));if(!v?.props)return v;const children=tidy(v.props.children,inside||v.type==='details');if(v.type==='details'&&inside){const parts=flat(children);return h('section',{...v.props,class:(v.props.class||'')+' secondary-facts',open:undefined},h('h3',null,parts[0]?.props?.children),parts.slice(1));}return cloneElement(v,{},children);}
const actions={
 'c:new':['plus','New conversation'],'c:list':['panel-left-open','Conversation list'],
 'c:archive':['archive','Archive this conversation'],'c:restore':['archive-restore','Restore this conversation'],
 'c:detach':['monitor-up','Detach conversation into its own window'],'c:return':['corner-down-left','Return conversation to main window'],
 'c:raise':['monitor-up','Raise the detached window'],'c:reply':['arrow-up','Send to this session'],
 'c:send':['arrow-up','Send request'],'c:from-draft':['file-plus-2','New conversation from this draft'],
 'c:status-refresh':['refresh-cw','Refresh session status'],'c:preview-close':['x','Close file preview'],
 'c:clear-search':['x','Clear thread search'],'c:cancel':['x','Cancel queued launch'],
 'a:view-save':['save','Save view conditions'],'a:view-save-as':['copy','Save as a new view'],'a:view-reset':['rotate-ccw','Reset to saved view conditions'],
 'a:answer-send':['arrow-up','Send this answer'],'a:answer-requeue':['rotate-ccw','Requeue with this reason'],
 'a:message-read':['check-check','Mark this event read'],'a:message-archive':['archive','Archive this event'],
 'a:message-restore':['archive-restore','Restore this event'],'a:answer-dismiss':['x','Dismiss this receipt']
};
function polish(v,thread,draft){
 if(Array.isArray(v))return v.map(x=>polish(x,thread,draft));if(!v?.props)return v;
 let p={...v.props};thread=p['data-thread-id']||thread;draft=draft||p.class?.split(' ').includes('new-chat');let children=polish(p.children,thread,draft),key=p['data-c']?'c:'+p['data-c']:p['data-a']?'a:'+p['data-a']:null;
 if(v.type==='button'&&actions[key]){const [glyph,label]=actions[key];p={...p,title:label,'aria-label':label,class:(p.class||'')+' compact-action'+(['c:send','c:reply','a:answer-send'].includes(key)?' primary-action':'')};children=icons.icon(glyph);}
 if(v.type==='button'&&p['data-c']==='list'){const back=text(v.props.children)==='Back';p={...p,title:back?'Return to the selected chat':'Choose an existing chat','aria-label':back?'Back to chat':'Chats',class:'list-chat-action compact-labelled'};children=[icons.icon(back?'arrow-left':'panel-left-open'),h('span',null,back?'Back to chat':'Chats')];}
 if(v.type==='button'&&p['data-c']==='new'){p={...p,title:'New chat','aria-label':'New chat',class:'new-chat-action compact-labelled'};children=[icons.icon('plus'),h('span',null,'New chat')];}
 if(v.type==='button'&&p['data-c']==='send'&&draft){p={...p,title:'Send the first message and start this chat','aria-label':'Start chat',class:'start-chat-action primary-action'};children=[icons.icon('arrow-up'),h('span',null,'Start chat')];}
 if(v.type==='button'&&p['data-route']==='sessions'){p={...p,title:'See agents working on tasks'};children=[icons.icon('activity'),'Task activity'];}
 if(v.type==='button'&&p['data-route']==='conversations'&&text(children)==='Conversations')children=[icons.icon('messages-square'),'Chats'];
 if(v.type==='button'&&p['data-a']==='popup-close'&&text(children)==='Done'){p={...p,title:'Done','aria-label':'Done',class:'compact-action'};children=icons.icon('check');}
 if(v.type==='button'&&p['data-a']==='workspace'&&p['data-page']==='answer'){p={...p,title:'Open question #'+p['data-task'],'aria-label':'Open question #'+p['data-task'],class:'compact-action'};children=icons.icon('messages-square');}
 if(v.type==='button'&&p['data-a']==='start'){const name=text(children),scope=/^Start \d+ held$/.test(name)?name+' tasks in this view':p.title||name;p={...p,class:(p.class||'')+' compact-labelled',title:scope,'aria-label':scope};children=[icons.icon('play'),h('span',null,name.replace(/^Start /,''))];}
 if(v.type==='a'&&/^Board(?: ↗)?$/.test(text(children))){const board=require('./review-board-links.js').board(p['data-url']||p.href);p={...p,title:'Open task on '+board.name,'aria-label':'Open task on '+board.name,class:(p.class||'')+' compact-action board-link'};children=icons.icon(board.kind);}
 if(v.type==='summary'&&text(children).trim()==='History'){p={...p,title:'Command history · fills the draft only','aria-label':'Command history',class:'compact-action'};children=icons.icon('history');}
 if(v.type==='label'&&p.class==='attach-label'&&!p['aria-disabled']){p={...p,title:'Attach files','aria-label':'Attach files',class:p.class+' compact-action'};children=[icons.icon('paperclip'),...flat(children).filter(x=>x?.type==='input').map(x=>cloneElement(x,{'aria-label':'Attach files',title:'Attach files'}))];}
 if(v.type==='input'&&p['data-c-field']==='title')p={...p,title:'Rename conversation','aria-label':'Conversation name'};
 if(v.type==='button'&&['recent','archive-list'].includes(p['data-c'])){p={...p,class:(p.class||'')+' compact-labelled'};children=[icons.icon(p['data-c']==='recent'?'inbox':'archive'),h('span',null,p['data-c']==='recent'?'Recent':'Archive')];}
 if(v.type==='div'&&p.class==='conversation-record'&&p['data-start-state']&&p['data-start-state']!=='draft'){const parts=flat(children),state=p['data-start-state'],handoff=parts.find(x=>x?.props?.class==='pm-handoff'),message=text(handoff&&flat(handoff.props.children)[0]).replace(/^[^:]+:\s*/,''),titles={pending:'Waiting to start',launching:'Starting chat',failed:'Could not start the chat',interrupted:'Chat start interrupted',unknown:'Delivery unknown'},visible=parts.filter(x=>x?.type==='button'||x?.props?.role==='alert'||x?.props?.['aria-label']==='Local file preview');children=[h('div',{class:'new-chat-guide'},h('h2',null,titles[state]||'Chat status unknown'),h('p',{role:'status'},message||'Check the original delivery before starting another chat.')),visible,h('details',{class:'first-message-details'},h('summary',null,'First message and details'),parts.filter(x=>!visible.includes(x)))];}
 if(draft&&v.type==='div'&&p.class==='conversation-record'){const parts=flat(children);children=[h('div',{class:'new-chat-guide'},h('h2',null,'New chat'),h('p',null,'Discuss a project with your project manager.'),h('p',{class:'meta'},'Choose the project below, then send your first message. Until you send, this is only a saved draft.')), ...parts.map(x=>x?.props?.['data-view-key']==='request-identity'?cloneElement(x,{},[h('summary',null,'Technical details'),...flat(x.props.children).slice(1)]):x)];}
 if(draft&&v.type==='details'&&p.class?.includes('composer-destination')){const fields=flat(children).find(x=>x?.props?.class==='composer-options overflow-actions');return h('section',{class:'new-chat-destination'},fields&&cloneElement(fields,{class:'new-chat-fields'},fields.props.children));}
 if(!draft&&v.type==='textarea'&&p['data-c-field']==='text'&&String(p.placeholder).startsWith('Reply to'))p={...p,placeholder:'Message…','aria-label':'Message in this chat'};
 if(draft&&v.type==='textarea'&&p['data-c-field']==='text')p={...p,placeholder:'What would you like to discuss?','aria-label':'First message to your project manager'};
 if(v.type==='section'&&p.class==='conversation-detail new-chat'){const parts=flat(children),reader=parts.find(x=>x?.props?.class==='candidate-reading'),composer=parts.find(x=>x?.props?.class==='conversation-composer');let identity;
  const take=n=>{if(Array.isArray(n))return n.map(take);if(n?.props?.['data-view-key']==='request-identity'){identity=n;return null;}return n?.props?cloneElement(n,{},take(n.props.children)):n;},clean=take(reader);
  const put=n=>Array.isArray(n)?n.map(put):n?.props?.class==='composer-options overflow-actions'?cloneElement(n,{},[n.props.children,identity]):n?.props?cloneElement(n,{},put(n.props.children)):n;
  children=[...parts.filter(x=>x!==reader&&x!==composer),clean,put(composer)];}
 if(v.type==='section'&&p.class==='conversation-detail'){
  const parts=flat(children),reader=parts.find(x=>x?.props?.class==='candidate-reading'),read=reader&&flat(reader.props.children),bar=read?.find(x=>x?.props?.class==='dialogue-bar');
  if(bar){const partsOfBar=flat(bar.props.children),state=partsOfBar.find(x=>x?.props?.class==='dialogue-state'),fixed=cloneElement(bar,{},state),context=partsOfBar.filter(x=>['dialogue-launch','dialogue-status'].includes(x?.props?.class)),other=partsOfBar.filter(x=>x!==state&&!context.includes(x)),next=cloneElement(reader,{},[inspector('Conversation info',context,'conversation-info:'+thread),...other,...read.filter(x=>x!==bar)]);children=parts.flatMap(x=>x===reader?[fixed,next]:[x]);}
 }
 if(v.type==='footer'&&p.class==='conversation-status'&&text(children).includes('Sent as one native turn.'))children='Message delivered · PM replies here.';
 if(v.type==='div'&&p.class==='dialogue-watch')return h('section',{class:'watch-editor'},h('h3',null,'Watch'),cloneElement(v,p,children));
 if(v.type==='details'&&p.class==='task-more'){const parts=flat(children);children=[h('summary',null,icons.icon('info'),'Task info'),...parts.slice(1)];}
 if(v.type==='details'&&p.class?.includes('composer-help')){const parts=flat(children);children=[parts[0],strip(parts.slice(1))];}
 return cloneElement(v,p,children);
}
function arrange(v){
 if(Array.isArray(v))return v.map(arrange);if(!v?.props)return v;
 let children=arrange(v.props.children);
 if(v.type==='section'&&v.props.class?.split(' ').includes('conversation-detail')){
  let parts=flat(children),reader=parts.find(x=>x?.props?.class==='candidate-reading'),outer=parts.find(x=>['conversation-composer','answer-composer'].includes(x?.props?.class)),composer=outer?.props.class==='answer-composer'?flat(outer.props.children).find(x=>x?.props?.class==='conversation-composer'):outer;
  if(reader&&composer){const cp=flat(composer.props.children),bottom=cp.filter(x=>x?.props?.class==='composer-bottom'||x?.props?.class?.includes('composer-reason')),editor=cloneElement(composer,{class:'conversation-composer composer-editor',key:'editor:'+reader.props['data-thread-id']},cp.filter(x=>!bottom.includes(x))),tools=cloneElement(composer,{class:'conversation-composer composer-actions'},bottom),actions=outer===composer?tools:cloneElement(outer,{},tools);let record=reader.props.children;
   if(v.props.class.includes('thread-detail')){const ids=[];const take=n=>Array.isArray(n)?n.map(take):n?.props?.class==='ids'?(ids.push(n),null):n?.props?cloneElement(n,{},take(n.props.children)):n;record=take(record);const place=n=>Array.isArray(n)?n.map(place):n?.props?.class?.includes('thread-head')?cloneElement(n,{},[n.props.children,inspector('Task info',ids,'task-info')]):n?.props?cloneElement(n,{},place(n.props.children)):n;record=[inspector('Task info',ids,'task-info'),record];}
   const flow=cloneElement(reader,{},[h('div',{class:'thread-record'},record),editor]),jump=h('button',{'data-c':'jump-latest',class:'jump-latest',hidden:true,title:'Go to latest and follow','aria-label':'Go to latest and follow'},icons.icon('arrow-down'),h('span',null,'Latest'));
   children=parts.flatMap(x=>x===reader?[flow,jump]:x===outer?[actions]:[x]);
  }
 }
 return cloneElement(v,{},children);
}
function present(nodes){
 const find=(v,test)=>{if(Array.isArray(v)){for(const child of v){const result=find(child,test);if(result)return result;}}else if(v?.props){if(test(v))return v;return find(v.props.children,test);}return null;};
 const toolbar=find(nodes,v=>v.props.class==='conversation-toolbar'),header=find(nodes,v=>v.props.class?.split(' ').includes('head'));
 if(!toolbar||!header)return tidy(polish(nodes));
 const actions=find(header,v=>v.props.class==='nav-actions'),nav=find(header,v=>v.type==='nav'&&v.props['aria-label']==='Workspace'),newButton=find(toolbar,v=>v.props['data-c']==='new'),listButton=find(toolbar,v=>v.props['data-c']==='list'),searchButton=find(toolbar,v=>v.props['data-c']==='search-open'),history=find(toolbar,v=>v.props['data-view-key']==='conversation-history');
 const replacements=new Map([[toolbar,null]]);
 if(actions)replacements.set(actions,cloneElement(actions,{},actions.props.children));
 const conversations=find(nodes,v=>v.props.class?.split(' ').includes('conversations'));if(conversations)replacements.set(conversations,cloneElement(conversations,{},[h('div',{class:'chat-entry-bar'},newButton,listButton,searchButton),conversations.props.children]));
 if(history){const composer=find(nodes,v=>v.props.class==='conversation-composer'),tools=composer&&find(composer,v=>v.props.class==='conversation-tools');if(tools){const children=flat(history.props.children),summary=h('summary',{title:'Command history · fills the draft only','aria-label':'Command history',class:'compact-action'},icons.icon('history')),details=cloneElement(history,{class:'command-history-menu'},[summary,...children.slice(1)]);replacements.set(tools,cloneElement(tools,{},[tools.props.children,details]));}}
 const grid=find(nodes,v=>v.props.class==='conversation-grid');if(grid)replacements.set(grid,grid.props.children);
 const swap=v=>{if(Array.isArray(v))return v.map(swap);if(replacements.has(v)){v=replacements.get(v);if(Array.isArray(v))return v.map(swap);}return v?.props?cloneElement(v,{},swap(v.props.children)):v;};
 return tidy(arrange(polish(swap(nodes))));
}
module.exports={polish,present};
