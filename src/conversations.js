'use strict';
const pm=require('./pm-composer.js');
const {h}=require('preact'),ui=require('./ui-runtime.js'),icons=require('./ui-icons.js'),{execution}=ui;
// D6: the event kinds a watch may follow.
const EVENTS=['close','failed','ask','blocked','gone'];
// #1742 Q-A5: a transcript view mounts at most WINDOW items and re-reads evicted ranges from the native source;
// at most FILL pages in a row without a view item are read without a user action;
// reading anchors persist for the SAVED most recently read threads within SAVED_BYTES of the 4096-byte navigation.
const WINDOW=600,SAVED=24,SAVED_BYTES=1536,FILL=8;
module.exports=execution.module('conversations',{
 create:function({window,document,menu,draw,navigate,onSelect}){
  let snapshot={},hydrated=false,items=[],selected=null,list='recent',search='',selection={},scrolls={},listOnly=false,focusDraft=false,focusSearch=false,undo=null,notices={},failure=null,preview=null,confirmDelete=false;
  let owner={id:'main',detached:[]},queue=[],pending=null,timer=null,composing=false,saveTimer=null,navTimer=null;
  // D4/D7: the open dialogue view of one launched conversation; reads bypass the draft queue.
  const measured=new WeakMap();
  let view=null,reads={},pollTimer=null,opened=[],positions={},synced=false,unified=false,observed=null,resize=null;
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
   capture:function(){const el=menu.querySelector('.conversation-reader');if(el?.parentElement.dataset.threadId&&!el.querySelector('.dialogue-transcript'))api.place(el.parentElement.dataset.threadId,el.scrollTop);},
   // a pixel offset for a thread reader, a record anchor for a transcript; the oldest beyond SAVED are dropped
   place:function(id,value){
    if(id!==undefined){delete positions[id];if(value!==undefined)positions[id]=value;}
    for(const k of Object.keys(positions)){if(Object.keys(positions).length<=SAVED&&JSON.stringify(positions).length<=SAVED_BYTES)break;delete positions[k];}
   },
   anchored:function(v){return !!v&&typeof v==='object'&&Number.isSafeInteger(v.at)&&v.at>=0&&Number.isSafeInteger(v.i)&&v.i>=0&&Number.isFinite(v.y);},
   editable:function(c){return !!c&&(!owner.conversation||owner.conversation===c.id)&&!(owner.detached||[]).includes(c.id)&&!c.deleted;},
   // The draft names only an unsent request; a sent or native thread keeps its title while the owner types the next message.
   title:function(c){const line=s=>(s||'').split('\n').find(s=>s.trim())?.trim();return c.title||(c.sent||c.request?line(c.sent?.text)||'Native request':line(c.text)||'New request');},
   visible:function(){return items.filter(c=>!c.deleted&&!!c.archived===(list==='archive')&&(!search||[api.title(c),c.root,c.id].join(' ').toLowerCase().includes(search.toLowerCase())));},
   launched:function(c){return c?.request?.state==='launched';},
   // D5: a live read wins over the snapshot; anything else, including no read yet, is unknown, never idle or done.
   state:function(c){
    const live=view?.id===c.id&&(view.status||view.statusReason),s=live?view.status?.state:c.dialogue?.state;
    return s==='working'||s==='idle'||/^open in /.test(s||'')?s.split(';')[0]:'unknown';
   },
   // #1773 A3/A4: the list and the head name the delivery or session state in words; bare unknown never shows
   rowStatus:function(c){
    if(api.launched(c)){const s=api.state(c);return {working:'Working',idle:'Ready'}[s]||(s==='unknown'?'Status unknown':s[0].toUpperCase()+s.slice(1));}
    if(c.request)return {pending:'Waiting to start',launching:'Starting chat',failed:'Could not start',interrupted:'Start interrupted'}[c.request.state]||'Delivery unknown';
    return c.sent?'Delivery unknown':'Draft';
   },
   activity:function(c){const s=api.state(c);return {working:'PM is replying',idle:'PM is ready',unknown:'PM status unknown'}[s]||s[0].toUpperCase()+s.slice(1);},
   read:function(kind,extra={}){
    if(!view)return;const id=api.id(),conversation=view.id;
    reads[id]={conversation,kind,gen:view.gen,...extra};view.busy[kind]=id;
    ui.send({a:'conversation-dialogue',id,conversation,action:kind==='status'?'status':'transcript',...extra});
    ui.defer(()=>{if(reads[id]){api.settle({id,conversation,state:'unknown',reason:'Dialogue read receipt missing'});draw();}},30000);
   },
   settle:function(r){
    const op=reads[r.id];if(!op)return;delete reads[r.id];
    if(view?.id!==op.conversation)return;if(view.busy[op.kind]===r.id)view.busy[op.kind]=null;
    const v=r.value,el=menu.querySelector('.conversation-reader');
    if(op.kind==='status'){if(r.state==='accepted'){view.status=v;view.statusReason='';api.form();}else {view.status=null;view.statusReason=r.reason||'Status unknown';}return;}
    if(r.state!=='accepted'){view.reason=r.reason||'Transcript unknown';return;}
    // Q-A3/A5: a reply for a replaced window, or for a cursor the window no longer ends at, is obsolete
    if(op.gen!==view.gen)return;
    view.reason='';view.size=v.size;
    if(el&&!view.stick&&view.items.length&&menu.querySelector('.dialogue-transcript')?.dataset.id===view.id)view.anchor=api.mark(el)||view.anchor;
    const have=new Set(view.items.map(api.key)),fresh=v.items.filter(item=>!have.has(api.key(item)));
    if(op.kind==='initial'){
     // a saved anchor reloads the range that starts at its record; a record that is gone is said, then Latest opens
     if(op.after!==undefined&&!v.items.some(item=>api.key(item)===view.anchor?.key)){view.lost='Saved reading position is no longer in the transcript; showing the latest messages.';view.from=undefined;view.anchor=null;view.stick=true;api.place(view.id);api.read('initial');return;}
     view.items=v.items;view.start=v.start;view.end=v.end;view.from=undefined;
    }
    else if(op.kind==='before'&&op.before===view.start){view.items=[...fresh,...view.items];view.start=v.start;view.quiet.before=fresh.length?0:view.quiet.before+1;if(fresh.length)view.announce='Earlier messages loaded';}
    else if(op.kind==='after'&&op.after===view.end){view.items.push(...fresh);view.end=v.end;view.quiet.after=fresh.length?0:view.quiet.after+1;if(fresh.length){view.announce=fresh.length+(fresh.length===1?' new message':' new messages');if(!view.stick)view.unseen=true;}}
   },
   key:function(item){return item.at+':'+item.i;},
   // Q-A3: the first transcript record visible in the reading viewport and its offset from the viewport top
   mark:function(el){
    const nodes=el.querySelectorAll('.dialogue-transcript>[data-key]');if(!nodes.length)return null;
    const top=el.getBoundingClientRect().top;let lo=0,hi=nodes.length-1;
    while(lo<hi){const mid=(lo+hi)>>1;if(nodes[mid].getBoundingClientRect().bottom<=top)lo=mid+1;else hi=mid;}
    return {key:nodes[lo].dataset.key,y:nodes[lo].getBoundingClientRect().top-top};
   },
   // Q-A3: the one owner of transcript scroll correction: follow the end, or keep the anchored record at its offset
   hold:function(el){
    if(view.stick){el.scrollTop=el.scrollHeight;return;}
    const a=view.anchor,node=a&&el.querySelector('.dialogue-transcript>[data-key="'+a.key+'"]');
    if(node)el.scrollTop+=node.getBoundingClientRect().top-el.getBoundingClientRect().top-a.y;
   },
   // Q-A3: font, width and content size changes keep the anchor too
   observe:function(log){
    if(observed===log)return;resize?.disconnect();observed=log;
    resize=new window.ResizeObserver(()=>{const el=observed?.isConnected&&observed.closest('.conversation-reader');if(el&&view?.id===observed.dataset.id)api.hold(el);});resize.observe(log);
   },
   // Q-A4: read earlier or later records while they are within one viewport of the visible area; short and
   // ignored-only pages leave the edge near, so the next draw reads again until the viewport fills or the cursor ends
   edge:function(el){
    const log=el.querySelector('.dialogue-transcript');if(!log||view.end===null||view.reason||view.busy.initial)return;
    const box=el.getBoundingClientRect(),r=log.getBoundingClientRect(),margin=el.clientHeight;
    if(view.start>0&&!view.busy.before&&view.quiet.before<FILL&&r.top>box.top-margin)api.read('before',{before:view.start});
    if(view.end<view.size&&!view.busy.after&&view.quiet.after<FILL&&r.bottom<box.bottom+margin)api.read('after',{after:view.end});
   },
   // Q-A5: evict whole records beyond two viewports, the farther side first; records that hold a selection stay
   // mounted while it is in use, so copy keeps its text
   trim:function(el){
    const items=view.items;if(items.length<=WINDOW)return false;
    const log=el.querySelector('.dialogue-transcript'),nodes=log.querySelectorAll(':scope>[data-key]');if(nodes.length!==items.length)return false;
    const box=el.getBoundingClientRect(),over=el.clientHeight*2,excess=items.length-WINDOW,sel=window.getSelection(),held=[];
    if(sel&&!sel.isCollapsed)for(const n of [sel.anchorNode,sel.focusNode]){const node=n?.parentElement?.closest('.dialogue-transcript>[data-key]');if(node&&log.contains(node))held.push([...nodes].indexOf(node));}
    let top=0,bottom=items.length;
    while(top<items.length&&nodes[top].getBoundingClientRect().bottom<box.top-over)top++;
    while(bottom>top&&nodes[bottom-1].getBoundingClientRect().top>box.bottom+over)bottom--;
    if(held.length){top=Math.min(top,...held);bottom=Math.max(bottom,...held.map(k=>k+1));}
    const far=items.length-bottom,b=top>=far?Math.min(far,excess-Math.min(top,excess)):Math.min(far,excess);
    let a=Math.min(top,excess-b);while(a>0&&items[a].i>0)a--;
    let cut=items.length-b;while(cut<items.length&&items[cut].i>0)cut++;
    if(!a&&cut===items.length)return false;
    if(cut<items.length)view.end=items[cut].at;if(a)view.start=items[a].at;
    view.items=items.slice(a,cut);return true;
   },
   // Q-A4: detached from the end, or new output arrived below the reader
   latest:function(){return !!view&&view.end!==null&&!view.reason&&!view.stick;},
   jump:function(){return api.latest()&&h('button',{'data-c':'latest',class:'dialogue-latest'},view.unseen?'New messages · Jump to latest':'Jump to latest');},
   // only this action and Send resume following; a window that does not hold the end is replaced by the newest page
   follow:function(){view.unseen=false;view.stick=true;view.lost='';api.place(view.id);if(view.end!==null&&view.end<view.size){view.gen++;view.busy.before=view.busy.after=null;api.read('initial');}},
   open:function(c){
    if(view?.id!==c.id){
     reads={};const saved=api.anchored(positions[c.id])?positions[c.id]:null;
     view={id:c.id,items:[],start:null,end:null,size:null,gen:0,from:saved?.at,reason:'',lost:'',announce:'',unseen:false,quiet:{before:0,after:0},status:null,statusReason:'',busy:{},stick:!saved,anchor:saved&&{key:saved.at+':'+saved.i,y:saved.y},ticks:0,form:null};
     api.read('initial',saved?{after:saved.at}:{});api.read('status');
    }
    if(!pollTimer)pollTimer=ui.defer(api.poll,2000);
   },
   // D4: poll every 2 s while the view is open; status, which probes the native holder, every 10 s.
   poll:function(){
    pollTimer=null;const c=api.current();
    if(!view||c?.id!==view.id||!menu.querySelector('.dialogue-transcript'))return;
    if(!view.busy.initial&&!view.busy.after){if(view.end===null)api.read('initial',view.from!==undefined?{after:view.from}:{});else if(view.end>=view.size)api.read('after',{after:view.end});}
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
    return h('div',{class:'dialogue-status'},h('p',{class:'meta'},'Status · '+watchText),
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
    const v=view?.id===c.id?view:null,label={user:'You',assistant:'Project manager'};
    // Q-A9: page loads are not announced in full; a short status says what changed
    return [h('p',{class:'sr-only',role:'status'},v?.announce||''),h('div',{class:'dialogue-transcript',role:'log','aria-live':'off','aria-label':'Transcript',tabindex:'0','data-id':c.id},
     v?.start>0&&h('button',{'data-c':'earlier',disabled:!!v.busy.before},v.busy.before?'Loading earlier…':'Load earlier'),
     v?.lost&&h('p',{role:'status',class:'dialogue-unknown'},v.lost),
     v?.reason&&h('p',{role:'status',class:'dialogue-unknown'},'Transcript unknown: '+v.reason+(v.items.length?'. Showing what was read before.':'')),
     !v||v.end===null?!v?.reason&&h('p',{class:'meta'},'Reading transcript…'):v.items.length?v.items.map(item=>{const key=api.key(item);return item.role==='tool'?h('p',{key,'data-key':key,class:'dialogue-tool',title:item.text},'Tool · '+item.text):item.role==='brief'?h('details',{key,'data-key':key,class:'dialogue-brief'},h('summary',{class:'meta'},'Setup brief'),h('p',{class:'conversation-text'},item.text)):h('article',{key,'data-key':key,class:'dialogue-item '+item.role},h('p',{class:'meta'},label[item.role]||item.role),h('p',{class:'conversation-text'},item.text));}):h('p',{class:'meta'},v.start>0?'No messages in this chunk; Load earlier reads more.':'No messages in the transcript yet.'),
     v?.end<v?.size&&v.quiet.after>=FILL&&h('button',{'data-c':'later',disabled:!!v.busy.after},v.busy.after?'Loading later…':'Load later'),
     c.archived&&api.jump())];
   },
   preview:function(){return preview&&h('section',{'aria-label':'Local file preview'},h('button',{'data-c':'preview-close'},'Close preview'),preview.file.mime.startsWith('image/')?h('img',{src:'data:'+preview.file.mime+';base64,'+preview.data,alt:preview.file.name}):h('pre',null,new window.TextDecoder().decode(Uint8Array.from(window.atob(preview.data),c=>c.charCodeAt(0)))));},
   local:function(c){return {title:c.title||'',root:c.root||'',text:c.text||'',caret:c.caret||[0,0],scroll:c.scroll||0,next:!!c.next};},
   // #1773 A4: the host refuses fractional scroll offsets and navigation over 4096 bytes; whole pixels and the newest opened threads fit
   nav:function(){ui.cancel(navTimer);navTimer=ui.defer(()=>{const whole=Object.fromEntries(Object.entries(scrolls).map(([k,v])=>[k,Math.max(0,Math.round(v)||0)]));let navigation;do navigation={list,search,selected:api.current()?.id||null,listOnly,scrolls:whole,threads:{selected,selection,opened,positions}};while(JSON.stringify(navigation).length>4096&&opened.shift());ui.send({a:'conversation-navigation',navigation});},100);},
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
    if(op.action!=='save')api.note('',op.conversation);
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
      if(op.action==='archive'&&c){c.archived=true;undo=c.id;const next=api.visible().find(row=>row.id!==c.id);selected=next?.id||null;selection[list]=selected;api.note('Archived');api.nav();}
      if(op.action==='restore'&&c){c.archived=false;list='recent';search='';api.select(c.id);api.note('Restored',op.conversation);undo=null;}
      if(op.action==='delete'){items=items.filter(c=>c.id!==op.conversation);selected=api.visible()[0]?.id||null;api.note('Permanently deleted');confirmDelete=false;undo=null;api.nav();}
      if(op.action==='fork'&&r.value?.forked){if(c&&c.version===op.version)Object.assign(c,native);for(const row of r.snapshot.items)if(!items.some(c=>c.id===row.id))items.push({...row,version:0,saved:0});api.select(r.value.forked);api.note('Draft and files moved to a separate request. Nothing sent.');}
      if(op.action==='preview'&&r.value){
       if(r.value.file.mime.startsWith('image/'))ui.effect('conversations.previewImage',()=>new Promise((resolve,reject)=>{const image=new window.Image();ui.listen(image,'load',()=>resolve(r.value));ui.listen(image,'error',()=>reject(Error('Image preview unavailable; file and draft kept')));image.src='data:'+r.value.file.mime+';base64,'+r.value.data;}),value=>{if(value.file){preview=value;api.note('Local preview',op.conversation);}else api.note(value.reason,op.conversation);draw();});
       else {preview=r.value;api.note('Local preview',op.conversation);}
      }
      if(c&&op.action==='reply'){c.sending=false;if(c.version===op.typed){c.text='';c.caret=[0,0];for(const f of c.files)queue.push({conversation:c.id,action:'remove',file:f.id});c.files=[];c.version=(c.version||0)+1;api.save(c);}api.note('Sent as one native turn. The reply appears in the transcript.',op.conversation);if(view?.id===c.id){api.follow();api.read('status');}}
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
      positions=t.positions&&typeof t.positions==='object'?Object.fromEntries(Object.entries(t.positions).filter(([k,v])=>Number.isFinite(v)&&v>=0||api.anchored(v))):{};api.place();
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
    if(c.sent||c.request)return 'Reply unavailable; inspect launch details.';
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
     ...own.map(row=>h('button',{key:row.id,class:'conversation-row','data-c':'select','data-id':row.id,'aria-current':!thread&&row.id===selected?'page':null},h('span',null,api.title(row)),h('small',null,(snapshot.projects?.find(p=>p.root===row.root)?.name||'Project unavailable')+' · '+api.rowStatus(row)+((owner.detached||[]).includes(row.id)?' · Detached':''),extra.marks?.[row.id]&&h('b',{class:'thread-marker'},' · '+extra.marks[row.id]))))];
    // A6: the session runtime is the one the request recorded at launch, not the current owner default
    const runtime=c?.request?.runtime||c?.sent?.runtime;
    // #1773 A3: the composer bottom row holds the attach, new-from-draft and history icons and the send arrow
    const tools=c&&!c.archived?h('div',{class:'conversation-tools composer-bottom'},!disabled&&h('label',{class:'attach-label compact-action',title:'Attach files'},icons.icon('paperclip'),h('span',{class:'sr-only'},'Attach'),h('input',{type:'file',multiple:true,accept:'.txt,.md,.png,.jpg,.jpeg,.webp','data-c-field':'files','aria-label':'Attach files'})),
     (c.sent||c.request)&&button('file-plus-2','New from draft',{'data-c':'from-draft',class:'compact-action',disabled:disabled||!!owner.conversation||!c.text.trim()}),
     h('details',{'data-view-key':'conversation-history',class:'command-history-menu'},h('summary',{class:'compact-action',title:'History','aria-label':'History'},icons.icon('history')),h('div',{class:'conversation-history'},(snapshot.ai?.history||[]).map((text,i)=>h('button',{'data-c':'history','data-index':i,disabled},text)),h('button',{'data-c':'clear-history'},'Clear command history'),h('p',{class:'meta'},'History fills only. Permanent deletion is available in Archive.'))),
     h('button',{'data-c':dialogue?'reply':'send',class:'primary-action compact-action',title:'Send','aria-label':'Send',disabled:!!bad},icons.icon('arrow-up'))):null;
    const win=owner.conversation?button('minimize-2','Return to main',{'data-c':'return',class:'compact-action'}):(owner.detached||[]).includes(c?.id)?button('maximize-2','Raise window',{'data-c':'raise',class:'compact-action',disabled:!!pending}):button('external-link','Detach',{'data-c':'detach',class:'compact-action',disabled:!!pending});
    return h('section',{class:'conversations'+(listOnly?' list-only':'')+(owner.conversation?' detached':''),'aria-label':'Conversations'},
     h('div',{class:'conversation-toolbar chat-entry-bar'},button(listOnly?'arrow-left':'rows-3',listOnly?'Back':'List',{'data-c':'list',class:'conversation-list-toggle compact-action'}),h('button',{'data-c':'new',class:'new-chat-action',disabled:!!owner.conversation},icons.icon('plus'),'New chat'),button('search','Search threads',{'data-c':'search-open',class:'compact-action'}),button('panels-top-left','Work sessions',{'data-route':'sessions',class:'compact-action'})),
     h('div',{class:'conversation-grid'},
      h('aside',{class:'conversation-list','aria-label':'Threads'},h('div',{class:'conversation-tools list-tabs'},h('button',{'data-c':'recent','aria-pressed':list==='recent'},icons.icon('inbox'),'Recent'),h('button',{'data-c':'archive-list','aria-pressed':list==='archive'},icons.icon('download'),'Archive')),h('div',{class:'conversation-search'},h('label',null,icons.icon('search'),h('span',{class:'sr-only'},'Search threads'),h('input',{type:'search','data-c-field':'search',value:search,placeholder:'Search',maxLength:512})),search&&button('x','Clear search',{'data-c':'clear-search',class:'compact-action'})),h('div',{class:'conversation-rows'},rows.length?rows:h('p',null,search?'No matches':'No '+(list==='archive'?'archived threads':'threads yet')))),
      thread?h('section',{class:'conversation-detail thread-detail','data-thread-id':thread,'aria-label':'Selected thread'},extra.detail?.header,h('div',{class:'conversation-reader'},extra.detail?.reading),extra.detail?.tools&&h('div',{class:'conversation-send'},extra.detail.tools)):
      h('section',{class:'conversation-detail','data-thread-id':c?.id||'','aria-label':'Selected request'},c?[h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title owner-title',title:api.title(c)},api.title(c)),h('span',{class:'options'},win,button(c.archived?'undo-2':'download',c.archived?'Restore':'Archive',{'data-c':c.archived?'restore':'archive',class:'compact-action',disabled:!api.editable(c)||!!pending}))),
       dialogue&&h('p',{class:'dialogue-activity'},h('span',{class:'meta dialogue-state','data-state':api.state(c).split(' ')[0],'aria-label':'Session state: '+api.state(c)},api.activity(c))),
       h('div',{class:'conversation-reader'},
       // UX07: one disclosure owns secondary controls; the transcript and composer stay clear by default.
       h('details',{class:'conversation-info dialogue-launch','data-view-key':'conversation-info:'+c.id},h('summary',null,icons.icon('info'),'Conversation info'),
        h('label',null,'Request name',h('input',{'data-c-field':'title','aria-label':'Request name',placeholder:api.title(c),value:c.title,disabled,maxLength:120})),
        h('div',{class:'conversation-tools'},c.archived?h('button',{'data-c':'delete-review',disabled:!api.editable(c)||!!pending},c.deleting?'Retry Delete':'Delete permanently'):null),
        h('label',null,'Project',h('select',{'data-c-field':'root',value:c.root,disabled},h('option',{value:''},'Choose a project'),!snapshot.projects?.some(p=>p.root===c.root)&&c.root&&h('option',{value:c.root},'Unavailable project'),(snapshot.projects||[]).map(p=>h('option',{value:p.root},p.name)))),
        h('p',{class:'meta'},(dialogue?'Session '+(runtime||'runtime not recorded'):snapshot.ai?.runtime?.name||'Runtime unknown')+' · '+(snapshot.host||'Host unknown')),
        h('p',{class:'meta'},'Request '+c.id),pm.handoff(c.request,true),
        dialogue&&!c.archived&&h('div',{class:'dialogue-bar'},api.status(c,disabled)),
        h('p',{class:'meta'},dialogue?'Send replies to this session. New from draft starts a separate request.':'Send shares this request and selected files with the visible native session.'),
        h('p',{class:'meta'},'UTF-8 text/Markdown, PNG/JPEG/WebP. 4 files, 5 MiB each, 20 MiB per draft, 100 MiB owned storage. PDF unavailable (0 pages). Sources stay untouched.'),
        disabled&&h('p',{class:'meta'},'Attachments cannot be added while this record is read-only.')),
       c.archived&&h('div',null,h('p',{class:'meta'},'Archived in TaskQ only; the native conversation stays. Restore before reply.'),confirmDelete&&h('div',{role:'alert'},h('p',null,'Permanently remove this TaskQ record and its owned files? No undo. Source files, native chats, sessions and board tasks stay.'),h('button',{'data-c':'delete'},'Delete permanently now'),h('button',{'data-c':'delete-cancel'},'Keep record'))),
       api.preview(),dialogue?api.transcript(c):h('div',{class:'conversation-record',tabindex:'0','aria-label':'Request record'},c.sent&&!c.request&&h('p',{role:'status'},'Delivery unknown. Check the original request ID; nothing will be resent.'),c.request&&c.request.state!=='launched'&&h('p',{role:'status'},c.request.reason||c.request.next||'Native launch: '+c.request.state),c.sent&&h('article',null,h('h2',null,'Sent request'),h('p',{class:'meta'},(snapshot.projects?.find(p=>p.root===c.sent.root)?.name||'Original project unavailable')+' · '+c.sent.runtime),h('p',{class:'conversation-text'},c.sent.text),h('ul',null,c.sent.files.map(f=>h('li',{key:f.id},f.name+' · '+f.bytes+' bytes · SHA256 '+f.hash)))),c.request?.state==='pending'&&h('button',{'data-c':'cancel',disabled},'Cancel queued launch'),c.sent&&h('button',{'data-c':'reconcile',disabled:disabled||!!pending},'Check delivery')),
       extra.notice,
       !c.archived&&pm.composer({text:c.text,disabled,id:'conversation-draft',placeholder:dialogue?'Reply to the '+(runtime||'native')+' session':c.sent?'Draft a separate next request':'Ask AI…',before:[dialogue&&view?.id===c.id&&api.jump(),h('p',{class:'meta composer-destination',title:dialogue?'Send replies to the recorded '+(runtime||'native')+' session. Change the separate request destination in Details.':'Change the project in Conversation info.'},icons.icon('folder'),(snapshot.projects?.find(p=>p.root===(dialogue?c.sent?.root||c.request?.root||c.root:c.root))?.name||'Project unavailable')+' · '+(dialogue?runtime||'Runtime unknown':snapshot.ai?.runtime?.name||'Runtime unknown')+' · '+(snapshot.host||'Host unknown')),
        // #1773 A3/A5: files sit above the input and the tools row closes the input box; the shared DashQ composer stays unchanged
               h('div',{class:'conversation-files'},(c.files||[]).map(f=>h('div',{class:'conversation-file',key:f.id},h('button',{'data-c':'preview','data-file':f.id,disabled},f.name+' · '+f.bytes+' B'),button('x','Remove '+f.name,{'data-c':'remove','data-file':f.id,disabled}))))],
        tools,reason:bad==='Write a message for the session.'?'':bad}))
      ]:h('p',null,'Choose a thread or New. Launched requests continue here as native turns.'))),
     h('footer',{class:'conversation-status',role:'status','aria-live':'polite'},notices[selected||'list']||'',pending?.conversation===selected&&' · Saving…',undo&&h('button',{'data-c':'undo'},'Undo archive'),failure&&failure.conversation!==selected&&h('span',null,' · Saving blocked by ',h('button',{'data-c':'reply-open','data-id':failure.conversation},api.title(items.find(c=>c.id===failure.conversation)||{title:failure.conversation})),'. Open it to resolve the receipt.'),failure?.conversation===selected&&h('button',{'data-c':'retry-save'},failure.action==='send'||failure.action==='reconcile'?'Check original delivery':['reply','watch','unwatch'].includes(failure.action)?'Check status':'Retry native save'),extra.footer));
   },
   sessions:function(){return [h('p',{class:'meta'},'Recorded workers and reviewers. Opening a target sends no input.'),(snapshot.projects||[]).map(p=>h('section',{key:p.root},h('h2',null,p.name),(!p.alive||p.complete===false)&&h('p',{role:'status'},'Session state unknown: project snapshot stale or incomplete.'),(p.tasks||[]).filter(t=>t.session).map(t=>h('article',{key:t.n},h('a',{href:p.issues+'/'+t.n},'#'+t.n+' '+t.title),h('p',{class:'meta'},(t.role||'Attempt')+' · '+(t.runtime||t.session)+' · '+(t.host||'Host unknown')+' · '+t.state),t.session_url?h('a',{href:t.session_url,'data-url':t.session_url},'Open recorded session'):h('p',{class:'meta'},t.session_reason||'Verified session target unavailable.'))))),h('p',{class:'meta'},'Remote/headless dialogue uses the native runtime or existing CLI. No local session or gateway is created.')];},
   click:function(b){
    const action=b.dataset.c;if(!action)return false;const c=api.current();
    if(action==='new'&&!owner.conversation)api.fresh();
    else if(action==='select'){if((owner.detached||[]).includes(b.dataset.id))ui.send({a:'conversation-window',action:'raise',conversation:b.dataset.id});else api.select(b.dataset.id);}
    else if(action==='search-open'){listOnly=true;focusSearch=true;api.nav();draw();}
    else if(action==='list'){listOnly=!listOnly;api.nav();draw();}
    else if(action==='recent'||action==='archive-list'){api.flush();api.show(action==='recent'?'recent':'archive');listOnly=true;api.nav();draw();}
    else if(action==='clear-search'){search='';api.nav();draw();}
    else if(action==='history'&&api.editable(c)){b.closest('details')?.removeAttribute('open');if(c.sent||c.sending)c.next=true;c.text=snapshot.ai.history[Number(b.dataset.index)];c.caret=[0,0];c.version=(c.version||0)+1;api.save(c);focusDraft=true;draw();}
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
    else if(action==='earlier'&&view&&!view.busy.before&&view.start>0){view.quiet.before=0;api.read('before',{before:view.start});draw();}
    else if(action==='later'&&view&&!view.busy.after&&view.end<view.size){view.quiet.after=0;api.read('after',{after:view.end});draw();}
    else if(action==='latest'&&view){api.follow();draw();}
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
     if(!e.target.querySelector('.dialogue-transcript')){api.place(selected,e.target.scrollTop);api.nav();return;}
     if(view?.id!==selected||view.end===null||!view.items.length)return;
     // Q-A3/A4: a reader at the end of a window that holds the file end follows; anywhere else keeps its record anchor
     // #1747: while Jump to latest replaces the window, a scroll event of the old window keeps following
     const shown=api.latest();if(!view.busy.initial)view.stick=view.end>=view.size&&e.target.scrollHeight-e.target.scrollTop-e.target.clientHeight<24;if(view.stick)view.unseen=false;
     view.anchor=api.mark(e.target);const a=view.anchor&&view.items.find(item=>api.key(item)===view.anchor.key);
     api.place(selected,view.stick||!a?undefined:{at:a.at,i:a.i,y:Math.round(view.anchor.y)});api.nav();
     api.edge(e.target);if(shown!==api.latest()||view.busy.before||view.busy.after)draw();
    },true);
    ui.listen(menu,'toggle',e=>{if(e.target.classList?.contains('conversation-info')&&view){view.stick=false;const detail=menu.querySelector('.conversation-reader');if(detail)view.anchor=api.mark(detail);}},true);
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
    if(detail&&!log){const p=positions[selected];detail.scrollTop=(typeof p==='number'?p:null)??open?.scroll??0;}
    if(log&&open){api.open(open);api.observe(log);api.hold(detail);}
    const c=api.current(),input=menu.querySelector('[data-c-field=text]'),record=menu.querySelector('.conversation-record');const rows=menu.querySelector('.conversation-rows');if(focusSearch){focusSearch=false;menu.querySelector('[data-c-field=search]')?.focus();}if(rows)rows.scrollTop=scrolls[list]||0;if(record&&c)record.scrollTop=c.scroll||0;if(focusDraft&&input&&!input.disabled){focusDraft=false;const row=menu.querySelector('[data-c=select][data-id="'+c.id+'"]');if(row?.checkVisibility()){row.scrollIntoView({block:'nearest'});scrolls[list]=rows.scrollTop;api.nav();}input.focus({preventScroll:true});input.setSelectionRange(...(c.caret||[0,0]));}
    if(log&&open){if(api.trim(detail))draw();else api.edge(detail);}},
   flush:function(){ui.cancel(saveTimer);const c=api.current();if(c)api.save(c);api.nav();},
  });return api;
 }
});
