'use strict';
const pm=require('./pm-composer.js');
const {h}=require('preact'),ui=require('./ui-runtime.js'),icons=require('./ui-icons.js'),{execution}=ui;
// D6: the event kinds a watch may follow.
const EVENTS=['close','failed','ask','blocked','gone'];
module.exports=execution.module('conversations',{
 create:function({window,document,menu,draw,navigate,onSelect}){
  let snapshot={},hydrated=false,items=[],selected=null,list='recent',search='',selection={},scrolls={},listOnly=false,focusDraft=false,undo=null,notices={},failure=null,preview=null,confirmDelete=false;
  let owner={id:'main',detached:[]},queue=[],pending=null,timer=null,composing=false,saveTimer=null,navTimer=null;
  // D4/D7: the open dialogue view of one launched conversation; reads bypass the draft queue.
  const measured=new WeakMap();
  let view=null,reads={},pollTimer=null,opened=[],positions={},synced=false,unified=false;
  const api=execution.module('conversations.instance',{
   id:function(){return Array.from(window.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');},
   note:function(text,id=selected){notices[id||'list']=text;},
   current:function(){return items.find(c=>c.id===selected);},
   ids:function(){return items.map(c=>c.id);},
   opened:function(){return opened;},
   remember:function(id,root){const prior=opened.find(r=>r.id===id);if(prior)prior.root=root||prior.root;else opened.push({id,root:root||null});search='';api.nav();},
   // C2/C7: one selected identity for owner, task and event rows; only view identities persist.
   sync:function(rows,legacy){
    if(!synced){synced=true;if(!unified&&legacy)selected=legacy;}
    const row=items.find(c=>c.id===selected&&!c.deleted)||rows.find(r=>r.id===selected);
    if(!row||!!row.archived!==(list==='archive'))selected=null;
    selection[list]=selected;onSelect?.(selected);return selected;
   },
   capture:function(){const el=menu.querySelector('.conversation-reader');if(el?.parentElement.dataset.threadId)positions[el.parentElement.dataset.threadId]=el.scrollTop;},
   editable:function(c){return !!c&&(!owner.conversation||owner.conversation===c.id)&&!(owner.detached||[]).includes(c.id)&&!c.deleted;},
   title:function(c){return c.title||(c.text||'').split('\n').find(s=>s.trim())?.trim()||c.sent?.text?.split('\n').find(s=>s.trim())?.trim()||(c.request?'Native request':'New request');},
   visible:function(){return items.filter(c=>!c.deleted&&!!c.archived===(list==='archive')&&(!search||[api.title(c),c.root,c.id].join(' ').toLowerCase().includes(search.toLowerCase())));},
   launched:function(c){return c?.request?.state==='launched';},
   // D5: a live read wins over the snapshot; anything else, including no read yet, is unknown, never idle or done.
   state:function(c){
    const live=view?.id===c.id&&(view.status||view.statusReason),s=live?view.status?.state:c.dialogue?.state;
    return s==='working'||s==='idle'||/^open in /.test(s||'')?s.split(';')[0]:'unknown';
   },
   read:function(kind,extra={}){
    if(!view)return;const id=api.id(),conversation=view.id;
    reads[id]={conversation,kind,...extra};view.busy[kind]=id;
    ui.send({a:'conversation-dialogue',id,conversation,action:kind==='status'?'status':'transcript',...extra});
    ui.defer(()=>{if(reads[id]){api.settle({id,conversation,state:'unknown',reason:'Dialogue read receipt missing'});draw();}},30000);
   },
   settle:function(r){
    const op=reads[r.id];if(!op)return;delete reads[r.id];
    if(view?.id!==op.conversation)return;if(view.busy[op.kind]===r.id)view.busy[op.kind]=null;
    const v=r.value,el=menu.querySelector('.conversation-reader');
    if(op.kind==='status'){if(r.state==='accepted'){view.status=v;view.statusReason='';api.form();}else {view.status=null;view.statusReason=r.reason||'Status unknown';}return;}
    if(r.state!=='accepted'){view.reason=r.reason||'Transcript unknown';return;}
    view.reason='';
    if(op.kind==='initial'){view.items=v.items.map((item,i)=>({...item,n:i}));view.start=v.start;view.end=v.end;}
    else if(op.kind==='before'&&op.before===view.start){const first=view.items[0]?.n??0;view.items=[...v.items.map((item,i)=>({...item,n:first-v.items.length+i})),...view.items];view.start=v.start;if(el)view.anchor={height:el.scrollHeight,top:el.scrollTop};}
    else if(op.kind==='after'&&op.after===view.end&&v.end>view.end){const last=view.items.at(-1)?.n??-1;view.items.push(...v.items.map((item,i)=>({...item,n:last+1+i})));view.end=v.end;}
   },
   open:function(c){
    if(view?.id!==c.id){reads={};view={id:c.id,items:[],start:null,end:null,reason:'',status:null,statusReason:'',busy:{},stick:positions[c.id]===undefined,anchor:null,ticks:0,form:null};api.read('initial');api.read('status');}
    if(!pollTimer)pollTimer=ui.defer(api.poll,2000);
   },
   // D4: poll every 2 s while the view is open; status, which probes the native holder, every 10 s.
   poll:function(){
    pollTimer=null;const c=api.current();
    if(!view||c?.id!==view.id||!menu.querySelector('.dialogue-transcript'))return;
    if(!view.busy.initial&&!view.busy.after){if(view.end===null)api.read('initial');else api.read('after',{after:view.end});}
    if(++view.ticks%5===0&&!view.busy.status)api.read('status');
    pollTimer=ui.defer(api.poll,2000);
   },
   form:function(){
    if(view.form?.dirty)return;const w=view.status?.watch;
    view.form=w?.prompt?{prompt:w.prompt,every:String(w.every),events:[...(w.events||[])],dirty:false}:{prompt:'',every:'30',events:[],dirty:false};
   },
   when:function(ms){return ms?new window.Date(Number(ms)).toLocaleString():'none';},
   // D7: the Status control shows the watch, last and next turn, outcome and last assistant line; it sets and removes the watch.
   status:function(c,disabled){
    const v=view?.id===c.id?view:null,s=v?.status,w=s?.watch,f=v?.form||{prompt:'',every:'30',events:[]},watching=!!w?.prompt,every=Number(f.every);
    const watchText=!s?(v?.statusReason?'Status unknown: '+v.statusReason:'Reading status…'):w==='no watch'?'no watch':w?.unknown?'Watch unknown: '+w.unknown:watching?'watch every '+w.every+' min':'Watch unknown';
    const t=s?.turn,row=(label,value)=>[h('dt',null,label),h('dd',null,value)];
    return h('details',{class:'dialogue-status','data-view-key':'dialogue-status'},h('summary',null,'Status · '+watchText),
     h('div',{class:'dialogue-status-body'},
      h('dl',null,
       watching?[row('Prompt',w.prompt),row('Every',w.every+' minutes'),row('Events',(w.events||[]).join(', ')||'none'),row('Last turn',api.when(w.last_turn)),row('Next turn',api.when(w.next_due)),row('Last outcome',w.last_outcome||'none')]:row('Watch',watchText),
       row('Latest turn',t?t.state+' · '+(t.outcome||(t.state==='done'?'outcome unknown':'outcome pending'))+' · '+api.when(t.at):'none'),
       row('Last assistant line',!s?'unknown':s.reason?'unknown: '+s.reason:s.last_line??'none in the last chunk')),
      h('div',{class:'dialogue-watch'},
       h('label',null,'Watch prompt',h('textarea',{'data-w-field':'prompt',value:f.prompt,rows:2,maxLength:65536,disabled,placeholder:'Check the board as the project rules say and report'})),
       h('label',null,'Every (minutes, at least 5)',h('input',{type:'number',min:5,step:1,'data-w-field':'every',value:f.every,disabled})),
       h('fieldset',null,h('legend',null,'Events that make it due at once'),EVENTS.map(e=>h('label',{key:e},h('input',{type:'checkbox','data-w-field':'event',value:e,checked:f.events.includes(e),disabled}),e))),
       h('div',{class:'conversation-tools'},h('button',{'data-c':'watch',disabled:disabled||!!pending||!f.prompt.trim()||!Number.isInteger(every)||every<5},watching?'Save watch':'Set watch'),watching&&h('button',{'data-c':'unwatch',disabled:disabled||!!pending},'Remove watch'),h('button',{'data-c':'status-refresh',disabled:!v||!!v.busy.status},'Refresh status')))));
   },
   // D4: user and assistant text in order, one line per tool call, newest at the bottom; unknown is never shown as empty.
   transcript:function(c){
    const v=view?.id===c.id?view:null,label={user:'You',assistant:'Assistant'};
    return h('div',{class:'dialogue-transcript',role:'log','aria-label':'Transcript',tabindex:'0','data-id':c.id},
     v?.start>0&&h('button',{'data-c':'earlier',disabled:!!v.busy.before},v.busy.before?'Loading earlier…':'Load earlier'),
     v?.reason&&h('p',{role:'status',class:'dialogue-unknown'},'Transcript unknown: '+v.reason+(v.items.length?'. Showing what was read before.':'')),
     !v||v.end===null?!v?.reason&&h('p',{class:'meta'},'Reading transcript…'):v.items.length?v.items.map(item=>item.role==='tool'?h('p',{key:item.n,class:'dialogue-tool',title:item.text},'Tool · '+item.text):h('article',{key:item.n,class:'dialogue-item '+item.role},h('p',{class:'meta'},label[item.role]||item.role),h('p',{class:'conversation-text'},item.text))):h('p',{class:'meta'},v.start>0?'No messages in this chunk; Load earlier reads more.':'No messages in the transcript yet.'));
   },
   preview:function(){return preview&&h('section',{'aria-label':'Local file preview'},h('button',{'data-c':'preview-close'},'Close preview'),preview.file.mime.startsWith('image/')?h('img',{src:'data:'+preview.file.mime+';base64,'+preview.data,alt:preview.file.name}):h('pre',null,new window.TextDecoder().decode(Uint8Array.from(window.atob(preview.data),c=>c.charCodeAt(0)))));},
   local:function(c){return {title:c.title||'',root:c.root||'',text:c.text||'',caret:c.caret||[0,0],scroll:c.scroll||0,next:!!c.next};},
   nav:function(){ui.cancel(navTimer);navTimer=ui.defer(()=>ui.send({a:'conversation-navigation',navigation:{list,search,selected:api.current()?.id||null,listOnly,scrolls,threads:{selected,selection,opened,positions}}}),100);},
   select:function(id,focus=true){const prior=api.current();if(prior&&(prior.version||0)!==(prior.saved||0))api.save(prior);selected=id;onSelect?.(id);selection[list]=id;listOnly=false;focusDraft=focus;confirmDelete=false;preview=null;api.nav();draw();},
   // #1699 A2: open one owner conversation in its list without acknowledging anything
   reveal:function(id,focus=true){const c=items.find(c=>c.id===id);if(!c)return false;list=c.archived?'archive':'recent';search='';api.select(id,focus);return true;},
   // #1699 A7: a restored task or event thread opens in Recent
   show:function(name){if(list!==name){selection[list]=selected;list=name;selected=selection[list]||null;onSelect?.(selected);api.nav();}},
   fresh:function(copy){
    const prior=api.current();if(prior&&(prior.version||0)!==(prior.saved||0))api.save(prior);
    const c={id:api.id(),revision:0,title:'',root:copy?.root||snapshot.projects?.[0]?.root||'',text:copy?.text||'',caret:copy?.caret||[0,0],scroll:0,files:[],archived:false,version:1};
    items.push(c);list='recent';search='';selected=c.id;onSelect?.(c.id);selection.recent=c.id;listOnly=false;focusDraft=true;api.save(c);api.nav();draw();
   },
   save:function(c){
    if(!api.editable(c))return;
    if((c.saved??-1)===(c.version||0)||pending?.conversation===c.id&&pending.action==='save'&&pending.version===c.version)return;
    const existing=queue.find(o=>o.conversation===c.id&&o.action==='save');
    const op={conversation:c.id,action:'save',draft:api.local(c),version:c.version||0};
    if(existing)Object.assign(existing,op);else queue.push(op);
    api.next();
   },
   next:function(){
    if(pending||!queue.length||failure)return;
    const op=queue.shift(),c=items.find(c=>c.id===op.conversation);if(!api.editable(c)){api.next();return;}
    pending={...op,id:api.id(),revision:c.revision||0,version:c.version||0};
    ui.send({a:op.action==='window'?'conversation-window':'conversation-action',...pending});
    ui.cancel(timer);timer=ui.defer(()=>{if(pending){failure={...pending,reason:'Native receipt missing; refresh or retry saving. Submitted requests must use Check delivery.'};pending=null;api.note(failure.reason,failure.conversation);draw();}},30000);
   },
   action:function(action,extra={}){
    const c=api.current();if(!api.editable(c)||failure||c.archived&&!['restore','delete','preview'].includes(action))return;
    ui.cancel(saveTimer);api.save(c);
    if(action==='send'&&(c.sending||c.sent||c.request)||action==='reply'&&c.sending)return;
    if(action==='send'||action==='reply')c.sending=true;
    queue.push({conversation:c.id,action,...extra});api.next();draw();
   },
   receive:function(s){
    if(s.dialogueReceipt){api.settle(s.dialogueReceipt);return true;}
    if(s.workspaceOwner){owner=s.workspaceOwner;if(owner.conversation||owner.select){selected=owner.conversation||owner.select;onSelect?.(selected);listOnly=false;focusDraft=true;}draw();return true;}
    if(s.conversationNavigationError!==undefined){if(s.conversationNavigationError)api.note(s.conversationNavigationError);return true;}
    if(s.conversationReceipt){
     const r=s.conversationReceipt;if(r.id!==pending?.id)return true;
     const op=pending;pending=null;ui.cancel(timer);const c=items.find(c=>c.id===op.conversation),native=r.snapshot?.items?.find(c=>c.id===op.conversation);
     if(c&&native){c.revision=native.revision;c.request=native.request;c.sent=native.sent;c.dialogue=native.dialogue;if(['attach','remove'].includes(op.action))c.files=native.files;}
     if(r.state!=='accepted'&&['reply','watch','unwatch'].includes(op.action)){
      // D3: a refused turn or watch change is definite; its text shows and the draft stays.
      if(c)c.sending=false;api.note(op.action==='reply'?'Not sent: '+(r.reason||'delivery unknown')+'. Draft kept.':r.reason||'Watch change unknown',op.conversation);api.next();
     }else if(r.state!=='accepted'){
      failure={...op,reason:r.reason||'Native readback unknown; draft kept'};api.note(failure.reason,op.conversation);
      if(c)c.sending=false;
     }else {
      if(c&&op.action==='save')c.saved=op.version;
      if(c&&op.action==='send'){c.sending=false;if(c.text===c.sent?.text&&!c.next){c.text='';c.caret=[0,0];for(const f of c.sent.files)queue.push({conversation:c.id,action:'remove',file:f.id});c.files=c.files.filter(f=>!c.sent.files.some(sent=>sent.id===f.id));c.version=(c.version||0)+1;api.save(c);}api.note('Request saved. Native launch and replies are separate.',op.conversation);}
      if(op.action==='archive'&&c){c.archived=true;undo=c.id;const next=api.visible().find(row=>row.id!==c.id);selected=next?.id||null;selection[list]=selected;api.note('Archived',op.conversation);api.nav();}
      if(op.action==='restore'&&c){c.archived=false;list='recent';search='';api.select(c.id);api.note('Restored',op.conversation);undo=null;}
      if(op.action==='delete'){items=items.filter(c=>c.id!==op.conversation);selected=api.visible()[0]?.id||null;api.note('Permanently deleted',op.conversation);confirmDelete=false;undo=null;api.nav();}
      if(op.action==='fork'&&r.value?.forked){if(c&&c.version===op.version)Object.assign(c,native);for(const row of r.snapshot.items)if(!items.some(c=>c.id===row.id))items.push({...row,version:0,saved:0});api.select(r.value.forked);api.note('Draft and files moved to a separate request. Nothing sent.',op.conversation);}
      if(op.action==='preview'&&r.value){
       if(r.value.file.mime.startsWith('image/'))ui.effect('conversations.previewImage',()=>new Promise((resolve,reject)=>{const image=new window.Image();ui.listen(image,'load',()=>resolve(r.value));ui.listen(image,'error',()=>reject(Error('Image preview unavailable; file and draft kept')));image.src='data:'+r.value.file.mime+';base64,'+r.value.data;}),value=>{if(value.file){preview=value;api.note('Local preview',op.conversation);}else api.note(value.reason,op.conversation);draw();});
       else {preview=r.value;api.note('Local preview',op.conversation);}
      }
      if(c&&op.action==='reply'){c.sending=false;if(c.version===op.typed){c.text='';c.caret=[0,0];for(const f of c.files)queue.push({conversation:c.id,action:'remove',file:f.id});c.files=[];c.version=(c.version||0)+1;api.save(c);}api.note('Sent as one native turn. The reply appears in the transcript.',op.conversation);if(view?.id===c.id){view.stick=true;api.read('status');}}
      if(view?.id===op.conversation&&op.action==='watch'&&r.value){view.status={...view.status,watch:r.value};view.form=null;api.form();api.note('Watch saved. Its prompt is sent when due and the session is idle.',op.conversation);}
      if(view?.id===op.conversation&&op.action==='unwatch'){view.status={...view.status,watch:'no watch'};view.form=null;api.form();api.note('Watch removed',op.conversation);}
      if(op.action==='cancel')api.note('Cancelled before native launch. No runtime work was rolled back.',op.conversation);
      if(op.action==='reconcile')api.note('Delivery checked using the original request ID.',op.conversation);
      api.next();
     }
     draw();return true;
    }
    if(!s.projects)return false;
    snapshot=s;
    const incoming=s.conversations;
    if(incoming){
     for(const n of incoming.items||[]){const c=items.find(c=>c.id===n.id);if(c){c.request=n.request;c.sent=n.sent;c.dialogue=n.dialogue;if(!pending&&!queue.some(o=>o.conversation===c.id)&&(c.saved??0)===(c.version??0)){Object.assign(c,n);}}else items.push({...n,version:0,saved:n.revision===0?-1:0});}
     if(incoming.complete)for(const c of items){if(c.revision>0&&!incoming.items.some(n=>n.id===c.id)){c.deleted=true;if(c.text===c.sent?.text)c.text='';c.sent=null;c.request=null;c.files=[];queue=queue.filter(o=>o.conversation!==c.id);if(pending?.conversation===c.id){pending=null;ui.cancel(timer);}if(failure?.conversation===c.id)failure=null;}}
    }
    if(!hydrated){hydrated=true;const nav=incoming?.navigation;if(nav){list=nav.list==='archive'?'archive':'recent';search=nav.search||'';selection=nav.selection||{};scrolls=nav.scrolls||{};selected=nav.selected;listOnly=nav.listOnly===true;}if(nav?.threads&&typeof nav.threads==='object'){
      const t=nav.threads;unified=true;selected=typeof t.selected==='string'?t.selected:null;
      selection=Object.fromEntries(['recent','archive'].map(k=>[k,typeof t.selection?.[k]==='string'?t.selection[k]:null]));
      opened=Array.isArray(t.opened)?t.opened.filter(r=>r&&typeof r.id==='string'&&/^task:https?:\/\/.+\/[1-9][0-9]*$/.test(r.id)&&(r.root==null||typeof r.root==='string')).map(r=>({id:r.id,root:r.root||null})):[];
      positions=t.positions&&typeof t.positions==='object'?Object.fromEntries(Object.entries(t.positions).filter(([k,v])=>Number.isFinite(v)&&v>=0)):{};
     }else if(!selected)selected=api.visible()[0]?.id||null;}

    return false;
   },
   problem:function(c){
    if(!c)return 'Choose New to write a request.';
    if(c.deleting)return 'Deletion incomplete; retry Delete. History cannot be restored.';
    if(!api.editable(c))return c.deleted?'History permanently deleted; this draft cannot be sent.':'This item is editable in its detached window.';
    if(failure)return 'Native save/delivery unknown. Keep the draft and resolve the receipt.';
    if(snapshot.conversations?.complete!==true)return snapshot.conversations?.reason||'Native draft storage unavailable; refresh before sending.';
    if(api.launched(c)){
     if(c.archived)return 'Restore before sending.';
     if(!(c.text||'').trim())return 'Write a message for the session.';
     if(c.files?.some(f=>f.error))return 'Resolve attachment errors before sending.';
     if(c.sending)return 'Sending; wait for its receipt.';
     return '';
    }
    if(c.sent||c.request)return 'Replies open here after the native launch. New from draft starts a separate request.';
    if(c.archived)return 'Restore before sending.';
    if(!(c.text||'').trim())return 'Write a request before sending files.';
    const p=snapshot.projects?.find(p=>p.root===c.root);if(!p)return 'Choose a registered project.';if(!p.alive)return 'Project service unavailable.';
    if(!snapshot.ai?.runtime?.available)return snapshot.ai?.runtime?.reason||'Owner runtime unavailable; no fallback.';
    if(c.files?.some(f=>f.error))return 'Resolve attachment errors before sending.';
    if(c.sending||pending?.action==='send')return 'Saving request; wait for its receipt.';
    return '';
   },
   files:function(files){
    const c=api.current();if(!api.editable(c)||c.archived||!files.length)return;
    for(const file of files){
     const mime=({txt:'text/plain',md:'text/markdown',png:'image/png',jpg:'image/jpeg',jpeg:'image/jpeg',webp:'image/webp'})[file.name.split('.').at(-1).toLowerCase()];
     if(!mime||!file.size||file.size>5*1024*1024||c.files.length+queue.filter(o=>o.conversation===c.id&&o.action==='attach').length>=4){api.note(file.name+': unsupported or over limit. Use 4 files, 5 MiB each; PDF unavailable (0 pages).');draw();continue;}
     const cid=c.id;
     ui.effect('conversations.readFile',()=>new Promise((resolve,reject)=>{const reader=new window.FileReader();ui.listen(reader,'load',()=>{if(mime.startsWith('image/')){const image=new window.Image();ui.listen(image,'load',()=>image.naturalWidth<=8000&&image.naturalHeight<=8000?resolve(reader.result):reject(Error('Image exceeds 8000 pixels per side; draft kept')));ui.listen(image,'error',()=>reject(Error('Image unreadable; source and draft kept')));image.src=reader.result;}else resolve(reader.result);});ui.listen(reader,'error',()=>reject(Error('File unreadable; source kept')));reader.readAsDataURL(file);}),result=>{
      if(typeof result!=='string'){api.note(file.name+': '+(result.reason||'Read failed'));draw();return;}
      const current=items.find(c=>c.id===cid);if(!api.editable(current)){api.note('File read finished after ownership changed; select it again in the owner window.');draw();return;}
      api.save(current);queue.push({conversation:cid,action:'attach',file:{id:api.id(),name:file.name,mime,bytes:file.size,hash:'0'.repeat(64)},data:result.slice(result.indexOf(',')+1)});api.next();draw();
     });
    }
   },
   // #1699 A1/A4: one thread list: owner conversations plus the task and event threads the workspace passes in
   body:function(extra={}){
    const thread=extra.selected,c=thread?null:api.current(),disabled=!api.editable(c)||c.deleting||c.archived,bad=api.problem(c),button=icons.button,dialogue=api.launched(c);
    if(c?.deleted)return h('section',{class:'conversations'},h('p',{role:'status'},'History permanently deleted. Sent content cannot be restored. Independent saved drafts remain in Recent.'),c.text&&h('label',null,'Unsent local text',h('textarea',{readOnly:true,value:c.text})),owner.conversation?h('button',{'data-c':'return'},'Return to main'):h('button',{'data-c':'new'},'New request'));
    const find=text=>!search||text.toLowerCase().includes(search.toLowerCase()),threads=(extra.rows||[]).filter(r=>!!r.archived===(list==='archive')&&find([r.title,r.meta].join(' '))),own=api.visible();
    const rows=[...threads.map(r=>h('button',{key:r.id,class:'conversation-row','data-a':'thread','data-thread':r.id,'aria-current':r.id===thread?'page':null},h('span',null,r.title),h('small',null,r.meta,r.marker&&h('b',{class:'thread-marker'},' · '+r.marker)))),
     ...own.map(row=>h('button',{key:row.id,class:'conversation-row','data-c':'select','data-id':row.id,'aria-current':!thread&&row.id===selected?'page':null},h('span',null,api.title(row)),h('small',null,(snapshot.projects?.find(p=>p.root===row.root)?.name||'Project unavailable')+' · '+(row.request?.kind||(row.sent?'Delivery unknown':'Draft'))+(api.launched(row)?' · '+api.state(row):'')+((owner.detached||[]).includes(row.id)?' · Detached':''),extra.marks?.[row.id]&&h('b',{class:'thread-marker'},' · '+extra.marks[row.id]))))];
    // A6: the session runtime is the one the request recorded at launch, not the current owner default
    const runtime=c?.request?.runtime||c?.sent?.runtime;
    const tools=c&&!c.archived?h('div',{class:'conversation-tools'},h('label',{class:'attach-label'},'Attach',h('input',{type:'file',multiple:true,accept:'.txt,.md,.png,.jpg,.jpeg,.webp','data-c-field':'files',disabled})),dialogue&&h('button',{'data-c':'reply',disabled:!!bad},'Send'),h('button',{'data-c':c.sent||c.request?'from-draft':'send',disabled:(c.sent||c.request)?disabled||!!owner.conversation||!c.text.trim():!!bad},c.sent||c.request?'New from draft':'Send')):null;
    return h('section',{class:'conversations'+(listOnly?' list-only':'')+(owner.conversation?' detached':''),'aria-label':'Conversations'},
     h('div',{class:'conversation-toolbar'},h('button',{'data-c':'list',class:'conversation-list-toggle'},listOnly?'Back':'List'),h('button',{'data-c':'new',disabled:!!owner.conversation},icons.icon('plus'),' New'),icons.button('panels-top-left','Work sessions',{'data-route':'sessions'}),h('details',{'data-view-key':'conversation-history'},h('summary',null,'History'),h('div',{class:'conversation-history'},(snapshot.ai?.history||[]).map((text,i)=>h('button',{'data-c':'history','data-index':i,disabled},text)),h('button',{'data-c':'clear-history'},'Clear command history'),h('p',{class:'meta'},'History fills only. Permanent deletion is available in Archive.')))),
     h('div',{class:'conversation-grid'},
      h('aside',{class:'conversation-list','aria-label':'Threads'},h('div',{class:'conversation-tools'},h('button',{'data-c':'recent','aria-pressed':list==='recent'},'Recent'),h('button',{'data-c':'archive-list','aria-pressed':list==='archive'},'Archive')),h('label',{class:'conversation-search'},h('span',{class:'sr-only'},'Search threads'),h('input',{type:'search','data-c-field':'search',value:search,placeholder:'Search',maxLength:512})),search&&h('button',{'data-c':'clear-search'},'Clear search'),h('div',{class:'conversation-rows'},rows.length?rows:h('p',null,search?'No matches':'No '+(list==='archive'?'archived threads':'threads yet')))),
      thread?h('section',{class:'conversation-detail thread-detail','data-thread-id':thread,'aria-label':'Selected thread'},extra.detail?.header,h('div',{class:'conversation-reader'},extra.detail?.reading),extra.detail?.tools&&h('div',{class:'conversation-send'},extra.detail.tools)):
      h('section',{class:'conversation-detail','data-thread-id':c?.id||'','aria-label':'Selected request'},c?[h('div',{class:'conversation-tools thread-head'},h('input',{'data-c-field':'title','aria-label':'Request name',placeholder:api.title(c),value:c.title,disabled,maxLength:120}),owner.conversation?h('button',{'data-c':'return'},'Return to main'):h('button',{'data-c':(owner.detached||[]).includes(c.id)?'raise':'detach',disabled:!!pending},(owner.detached||[]).includes(c.id)?'Raise window':'Detach'),h('button',{'data-c':c.archived?'restore':'archive',disabled:!api.editable(c)||!!pending},c.archived?'Restore':'Archive')),
       h('div',{class:'conversation-reader'},
       // A5: launch diagnostics and Status collapse above the transcript, which takes the reading area
       dialogue?[h('div',{class:'dialogue-bar'},h('p',{class:'dialogue-state','data-state':api.state(c).split(' ')[0]},'State: '+api.state(c)),h('details',{class:'dialogue-launch','data-view-key':'dialogue-launch'},h('summary',null,'Details'),pm.handoff(c.request,true)),c.archived?h('div',null,h('p',{class:'meta'},'Archived in TaskQ only; the native conversation stays. Restore before reply.'),h('button',{'data-c':'delete-review',disabled:!api.editable(c)||!!pending},'Delete permanently'),confirmDelete&&h('div',{role:'alert'},h('p',null,'Permanently remove this TaskQ record and its owned files? No undo. Native chats and sessions stay.'),h('button',{'data-c':'delete'},'Delete permanently now'),h('button',{'data-c':'delete-cancel'},'Keep record'))):api.status(c,disabled),api.preview()),api.transcript(c)]:
       h('div',{class:'conversation-record',tabindex:'0','aria-label':'Request record'},h('details',{'data-view-key':'request-identity'},h('summary',null,'Request identity'),h('p',{class:'meta'},c.id)),c.sent&&!c.request&&h('p',{role:'status'},'Delivery unknown. Check the original request ID; nothing will be resent.'),pm.handoff(c.request,true),c.sent&&h('article',null,h('h2',null,'Sent request'),h('p',{class:'meta'},(snapshot.projects?.find(p=>p.root===c.sent.root)?.name||'Original project unavailable')+' · '+c.sent.runtime),h('p',{class:'conversation-text'},c.sent.text),h('ul',null,c.sent.files.map(f=>h('li',{key:f.id},f.name+' · '+f.bytes+' bytes · SHA256 '+f.hash)))),(c.sent||c.request)&&h('p',{class:'meta'},'Replies open here after the native launch.'),c.request?.state==='pending'&&h('button',{'data-c':'cancel',disabled},'Cancel queued launch'),c.sent&&h('button',{'data-c':'reconcile',disabled:disabled||!!pending},'Check delivery'),c.archived&&h('button',{'data-c':'delete-review',disabled:!api.editable(c)||!!pending},c.deleting?'Retry Delete':'Delete permanently'),confirmDelete&&h('div',{role:'alert'},h('p',null,'Permanently remove this TaskQ record and its owned files? No undo. Source files, native chats, sessions and board tasks stay.'),h('button',{'data-c':'delete'},'Delete permanently now'),h('button',{'data-c':'delete-cancel'},'Keep record')),api.preview()),
       extra.notice,
       !c.archived&&pm.composer({text:c.text,disabled,id:'conversation-draft',placeholder:dialogue?'Reply to the '+(runtime||'native')+' session':c.sent?'Draft a separate next request':'Ask AI…',before:[h('label',null,'Project',h('select',{'data-c-field':'root',value:c.root,disabled},h('option',{value:''},'Choose a project'),!snapshot.projects?.some(p=>p.root===c.root)&&c.root&&h('option',{value:c.root},'Unavailable project'),(snapshot.projects||[]).map(p=>h('option',{value:p.root},p.name)))),h('p',{class:'meta'},(dialogue?(runtime?'Session '+runtime:'Session runtime not recorded'):snapshot.ai?.runtime?.name||'Runtime unknown')+' · '+(snapshot.host||'Host unknown'))],files:
        h('div',{class:'conversation-files'},(c.files||[]).map(f=>h('div',{class:'conversation-file',key:f.id},h('button',{'data-c':'preview','data-file':f.id,disabled},f.name+' · '+f.bytes+' B'),button('x','Remove '+f.name,{'data-c':'remove','data-file':f.id,disabled})))),
        reason:bad||(dialogue?'Send goes to this '+(runtime||'native')+' session from this window as one turn. New from draft starts a separate request.':'Share this request and selected files with the visible native session.'),after:h('details',{'data-view-key':'file-limits'},h('summary',null,'File limits'),h('p',{class:'meta'},'UTF-8 text/Markdown, PNG/JPEG/WebP. 4 files, 5 MiB each, 20 MiB per draft, 100 MiB owned storage. PDF unavailable (0 pages). Sources stay untouched.'))})),tools&&h('div',{class:'conversation-send'},tools)
      ]:h('p',null,'Choose a thread or New. Launched requests continue here as native turns.'))),
     h('footer',{class:'conversation-status',role:'status','aria-live':'polite'},notices[selected||'list']||'',pending?.conversation===selected&&' · Saving…',undo&&h('button',{'data-c':'undo'},'Undo archive'),failure?.conversation===selected&&h('button',{'data-c':'retry-save'},failure.action==='send'||failure.action==='reconcile'?'Check original delivery':['reply','watch','unwatch'].includes(failure.action)?'Check status':'Retry native save'),extra.footer));
   },
   sessions:function(){return [h('p',{class:'meta'},'Recorded workers and reviewers. Opening a target sends no input.'),(snapshot.projects||[]).map(p=>h('section',{key:p.root},h('h2',null,p.name),(!p.alive||p.complete===false)&&h('p',{role:'status'},'Session state unknown: project snapshot stale or incomplete.'),(p.tasks||[]).filter(t=>t.session).map(t=>h('article',{key:t.n},h('a',{href:p.issues+'/'+t.n},'#'+t.n+' '+t.title),h('p',{class:'meta'},(t.role||'Attempt')+' · '+(t.runtime||t.session)+' · '+(t.host||'Host unknown')+' · '+t.state),t.session_url?h('a',{href:t.session_url,'data-url':t.session_url},'Open recorded session'):h('p',{class:'meta'},t.session_reason||'Verified session target unavailable.'))))),h('p',{class:'meta'},'Remote/headless dialogue uses the native runtime or existing CLI. No local session or gateway is created.')];},
   click:function(b){
    const action=b.dataset.c;if(!action)return false;const c=api.current();
    if(action==='new'&&!owner.conversation)api.fresh();
    else if(action==='select'){if((owner.detached||[]).includes(b.dataset.id))ui.send({a:'conversation-window',action:'raise',conversation:b.dataset.id});else api.select(b.dataset.id);}
    else if(action==='list'){listOnly=!listOnly;api.nav();draw();}
    else if(action==='recent'||action==='archive-list'){api.flush();api.show(action==='recent'?'recent':'archive');listOnly=true;api.nav();draw();}
    else if(action==='clear-search'){search='';api.nav();draw();}
    else if(action==='history'&&api.editable(c)){if(c.sent||c.sending)c.next=true;c.text=snapshot.ai.history[Number(b.dataset.index)];c.caret=[0,0];c.version=(c.version||0)+1;api.save(c);focusDraft=true;draw();}
    else if(action==='clear-history'){ui.send({a:'ai-clear'});snapshot.ai.history=[];draw();}
    else if(action==='undo'){selected=undo;api.action('restore');}
    else if(action==='delete-review'){confirmDelete=true;draw();}
    else if(action==='delete-cancel'){confirmDelete=false;draw();}
    else if(action==='preview-close'){preview=null;draw();}
    else if(action==='from-draft'&&api.editable(c))api.action('fork',{fork_id:api.id()});
    else if(action==='raise'&&c)ui.send({a:'conversation-window',action:'raise',conversation:c.id});
    else if(action==='return'&&c?.deleted)ui.send({a:'conversation-window',windowAction:'return',conversation:c.id});
    else if(action==='detach'||action==='return'){if(c){api.save(c);queue.push({conversation:c.id,action:'window',windowAction:action});api.next();}}
    else if(action==='reply-open'){if(!api.reveal(b.dataset.id)){api.note('Conversation unavailable in this window; refresh.');draw();}}
    else if(action==='reply'&&!api.problem(c))api.action('reply',{typed:c.version||0});
    else if(action==='watch'&&view?.form){api.action('watch',{prompt:view.form.prompt,every:Number(view.form.every),events:EVENTS.filter(e=>view.form.events.includes(e))});}
    else if(action==='unwatch')api.action('unwatch');
    else if(action==='status-refresh'&&view&&!view.busy.status){api.read('status');draw();}
    else if(action==='earlier'&&view&&!view.busy.before&&view.start>0){api.read('before',{before:view.start});draw();}
    else if(action==='retry-save'){
     const failed=failure;failure=null;
     // a reply with no receipt is never resent: the transcript and status tell whether it ran
     if(failed&&['reply','watch','unwatch'].includes(failed.action)){const f=items.find(c=>c.id===failed.conversation);if(f)f.sending=false;api.note('Delivery unknown; draft kept. Check the transcript and status before sending again.');if(view?.id===failed.conversation)api.read('status');api.next();draw();}
     else if(failed){if(['send','reconcile'].includes(failed.action)){queue=queue.filter(o=>o.action!=='send');queue.unshift({conversation:failed.conversation,action:'reconcile'});}else queue.unshift({...failed,id:undefined});api.next();draw();}
    }
    else if(['archive','restore','send','cancel','reconcile','preview','remove','delete'].includes(action))api.action(action,{...(b.dataset.file?{file:b.dataset.file}:{}),...(action==='send'?{runtime:snapshot.ai?.runtime?.id}:{})});
    return true;
   },
   mount:function(){
    ui.listen(menu,'input',e=>{const watch=e.target.dataset.wField;if(view?.form&&(watch==='prompt'||watch==='every')){view.form[watch]=e.target.value;view.form.dirty=true;draw();return;}const field=e.target.dataset.cField;if(field==='search'){search=e.target.value;api.nav();draw();return;}if(!['text','title'].includes(field))return;const c=api.current();if(!api.editable(c))return;if(field==='text'&&(c.sent||c.sending))c.next=true;c[field]=e.target.value;if(field==='text')c.caret=[e.target.selectionStart,e.target.selectionEnd];c.version=(c.version||0)+1;ui.cancel(saveTimer);saveTimer=ui.defer(()=>api.save(c),100);draw();});
    ui.listen(menu,'change',e=>{if(e.target.dataset.wField==='event'&&view?.form){view.form.events=view.form.events.filter(k=>k!==e.target.value).concat(e.target.checked?[e.target.value]:[]);view.form.dirty=true;draw();return;}const c=api.current();if(e.target.dataset.cField==='files'){api.files([...e.target.files]);e.target.value='';}if(e.target.dataset.cField==='root'&&api.editable(c)){if(c.sent||c.sending)c.next=true;c.root=e.target.value;c.version=(c.version||0)+1;api.save(c);draw();}});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{if(e.target.dataset.cField!=='text')return;const c=api.current();if(api.editable(c)&&JSON.stringify(c.caret)!==JSON.stringify([e.target.selectionStart,e.target.selectionEnd])){c.caret=[e.target.selectionStart,e.target.selectionEnd];c.version=(c.version||0)+1;api.save(c);}});
    ui.listen(menu,'scroll',e=>{
     if(e.target.classList?.contains('conversation-rows')){scrolls[list]=e.target.scrollTop;api.nav();return;}
     if(!e.target.classList?.contains('conversation-reader'))return;
     positions[selected]=e.target.scrollTop;api.nav();
     if(view?.id===selected&&e.target.querySelector('.dialogue-transcript')){view.stick=e.target.scrollHeight-e.target.scrollTop-e.target.clientHeight<24;if(e.target.scrollTop<32&&view.start>0&&!view.busy.before&&!view.anchor){api.read('before',{before:view.start});draw();}}
    },true);
    ui.listen(menu,'toggle',e=>{if(e.target.classList?.contains('dialogue-status')&&view){view.stick=false;const detail=menu.querySelector('.conversation-reader');if(detail)positions[selected]=detail.scrollTop;}},true);
    ui.listen(window,'resize',()=>draw());
    ui.listen(menu,'dragover',e=>{if(e.target.closest('.conversations'))e.preventDefault();});
    ui.listen(menu,'drop',e=>{if(!e.target.closest('.conversations'))return;e.preventDefault();api.files([...e.dataTransfer.files]);});
    ui.listen(menu,'compositionstart',()=>{composing=true;});ui.listen(menu,'compositionend',()=>{composing=false;});
    ui.listen(document,'keydown',e=>{if(e.target.dataset?.cField==='text'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();const c=api.current();if(api.problem(c))return;if(api.launched(c))api.action('reply',{typed:c.version||0});else api.action('send',{runtime:snapshot.ai?.runtime?.id});}});
   },
   grow:function(){
    for(const input of menu.querySelectorAll('.conversation-composer textarea')){
     const style=window.getComputedStyle(input),width=input.getBoundingClientRect().width,key=JSON.stringify([input.value,width,style.font,style.lineHeight]);
     if(measured.get(input)===key)continue;
     const mirror=input.cloneNode();mirror.removeAttribute('id');mirror.removeAttribute('data-c-field');mirror.disabled=true;mirror.tabIndex=-1;mirror.setAttribute('aria-hidden','true');mirror.value=input.value;
     Object.assign(mirror.style,{position:'absolute',visibility:'hidden',pointerEvents:'none',width:width+'px',height:'0',minHeight:'0',maxHeight:'none'});
     input.after(mirror);const height=mirror.scrollHeight+parseFloat(style.borderTopWidth)+parseFloat(style.borderBottomWidth);mirror.remove();
     input.style.height=Math.max(parseFloat(style.minHeight)||0,height)+'px';measured.set(input,key);
    }
    const reader=menu.querySelector('.conversation-reader'),editor=reader?.querySelector(':scope>.conversation-composer,:scope>.answer-composer');
    if(reader)reader.classList.toggle('long-draft',!!editor&&editor.getBoundingClientRect().height>reader.clientHeight*.45);
   },
   afterDraw:function(){
    api.grow();
    const log=menu.querySelector('.dialogue-transcript'),detail=menu.querySelector('.conversation-reader'),open=api.current();
    if(detail)detail.scrollTop=positions[selected]??open?.scroll??0;
    if(log&&open){api.open(open);if(view.anchor){detail.scrollTop=detail.scrollHeight-view.anchor.height+view.anchor.top;view.anchor=null;}else if(view.stick)detail.scrollTop=detail.scrollHeight;positions[selected]=detail.scrollTop;}
    const c=api.current(),input=menu.querySelector('[data-c-field=text]'),record=menu.querySelector('.conversation-record');const rows=menu.querySelector('.conversation-rows');if(rows)rows.scrollTop=scrolls[list]||0;if(record&&c)record.scrollTop=c.scroll||0;if(focusDraft&&input&&!input.disabled){focusDraft=false;const row=menu.querySelector('[data-c=select][data-id="'+c.id+'"]');if(row?.checkVisibility()){row.scrollIntoView({block:'nearest'});scrolls[list]=rows.scrollTop;api.nav();}input.focus({preventScroll:true});input.setSelectionRange(...(c.caret||[0,0]));}},
   flush:function(){ui.cancel(saveTimer);const c=api.current();if(c)api.save(c);api.nav();},
  });return api;
 }
});
