'use strict';
const pm=require('./pm-composer.js'),blocks=require('./project-blocks.js'),time=require('./relative-time.js');
const views=require('./task-views.js'),news=require('./notification-panel.js');
const {h}=require('preact'),ui=require('./ui-runtime.js'),icons=require('./ui-icons.js'),{execution}=ui;
// D6: the event kinds a watch may follow.
const EVENTS=['close','failed','ask','blocked','gone'];
// #1742 Q-A5: a transcript view mounts at most WINDOW items and re-reads evicted ranges from the native source;
// at most FILL pages in a row without a view item are read without a user action;
// reading anchors persist for the SAVED most recently read threads within SAVED_BYTES of the 4096-byte navigation.
const WINDOW=600,SAVED=24,SAVED_BYTES=1536,FILL=8,PAGE=64,ROWS=192,BYTES=32*1024*1024;
const MODES=[['all','All','messages-square'],['new','New','inbox'],['needs','Needs me','message-circle-question'],['reply','Needs reply','message-circle-question'],['decision','Decisions','circle-check'],['activity','In progress','activity'],['finished','Finished','check-check']];
// #1918 A6: tools whose argument names the files they read or change; a command naming a test runner is a test
const FILE_TOOLS=/^(Read|Edit|MultiEdit|Write|NotebookEdit|apply_patch|read_file|write_file|view)$/,TEST=/\b(test|tests|pytest|unittest|selftest|check\.py)\b/i;
module.exports=execution.module('conversations',{
 create:function({window,document,menu,draw,navigate,onSelect,checkpoint=()=>{}}){
  let snapshot={},catalogEpoch=-1,hydrated=false,navigated=false,items=[],selected=null,list='recent',search='',selection={},scrolls={},listOnly=false,focusDraft=false,focusSearch=false,searchOpen=false,undo=null,notices={},failure=null,preview=null,confirmDelete=false;
  let sidebar={pins:[],collapsed:[],archived:{}},sidebarPending=null,revealed=null,activityRead=null,activityTimer=null,activityTimes={},activityChecks={};
  let focuses={},readerIdentity=null,layoutFrame=null,correcting=false,scrollIntent=null,scrollIntentTimer=null,dragScroll=false,sidebarEpoch=0,sidebarProjection=null,stableOrder=new Map(),stableGroups=new Map(),modeCounts={},pageSelected=null,listObserver=null,listObservedRoot=null,listAnchor=null;
  let resultFilter='all',sidebarItems=[],resultPages=new Map(),scopeReturn=null,completed=null,readerResize=null;
  const closedViews=new Map(),historyViews=new Map(),textPages=new Map(),recordBytes=new WeakMap(),taskBodies=new Map(),taskReads=new Map();let taskSelected=null;
  let recoveryQueue=[],recoveryDrafts=[];
  let owner={id:'main',detached:[]},queue=[],pending=null,timer=null,composing=false,saveTimer=null,navTimer=null;
  // D4/D7: the open dialogue view of one launched conversation; reads bypass the draft queue.
  // Q-A5: owner/task/result reserve 2 slots/2 MiB each; closed/question reserve 1/1 MiB each, total 8/8 MiB.
  let bodyReads=new Map(),bodyCache=new Map(),bodyNotice='';
  let view=null,reads={},pollTimer=null,opened=[],positions={},synced=false,unified=false,observed=null,resize=null;
  // #1831: one attempt-fenced live tail, owned only by the visible task thread.
  let live=null,workerPaused=false,linked=null,linkedReads={};
  const api=execution.module('conversations.instance',{
   id:function(){return Array.from(window.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');},
   note:function(text,id=selected){notices[id||'list']=text;},
   task:function(p,t){
    if(!t){taskSelected=null;return t;}if(!t.body_lazy)return t;const key=p.root+'#'+t.n+'@'+t.revision+':'+t.body_source_revision;taskSelected=key;
    const cached=taskBodies.get(key);if(cached){taskBodies.delete(key);taskBodies.set(key,cached);return {...t,...cached.row,body_lazy:false};}
    if(!taskReads.has(key)){const op={id:api.id(),root:p.root,n:t.n,revision:t.revision,source:t.body_source_revision};taskReads.set(key,op);while(taskReads.size>8){const [old,v]=taskReads.entries().next().value;ui.cancel(v.timer);taskReads.delete(old);}op.timer=ui.defer(()=>{if(taskReads.get(key)===op){op.reason='Task body readback missing. Refresh before reading or answering.';draw();}},30000);ui.send({a:'conversation-dialogue',action:'task-body',id:op.id,conversation:'task:'+p.issues+'/'+t.n,root:p.root,revision:t.revision,source_revision:t.body_source_revision});}
    return {...t,body_reason:taskReads.get(key)?.reason||'Reading the current task card…'};
   },
   taskTrim:function(){let bytes=0;for(const [key,v]of [...taskBodies].reverse()){bytes+=v.bytes;if(key!==taskSelected&&(taskBodies.size>2||bytes>2*1024*1024))taskBodies.delete(key);}},
   taskOverflow:function(){const v=taskBodies.get(taskSelected);return v?.bytes>2*1024*1024?h('p',{role:'status',class:'meta'},'Selected task source: '+(v.bytes/1048576).toFixed(1)+' MiB pinned while reading; inactive task bodies are evicted.'):null;},
   taskText:function(t,text,source,root){
    if(!text)return null;const key='task-'+Array.from(new TextEncoder().encode(root),b=>b.toString(16).padStart(2,'0')).join('')+'-'+t.n+'-'+t.revision+'-'+source,match=new RegExp('^report:'+key+'-(\\d+):').exec(positions[selected]?.key||'');
    const start=Math.min(text.length-1,match?Number(match[1]):textPages.get(key)||0);const endAt=at=>{let end=Math.min(text.length,at+32768),count=0;for(const m of text.slice(at,end).matchAll(/\n\s*\n/g))if(++count===63){end=at+m.index+m[0].length;break;}if(end<text.length&&/[\uD800-\uDBFF]/.test(text[end-1]))end++;return end;},end=endAt(start);
    const turn=next=>{textPages.delete(key);textPages.set(key,next);while(textPages.size>24)textPages.delete(textPages.keys().next().value);api.place(selected,{key:'report:'+key+'-'+next+':0',y:0});api.nav();draw();};
    return [require('./reports.js').body(text.slice(start,end),key+'-'+start),(start>0||end<text.length)&&h('div',{class:'conversation-tools'},h('span',{class:'meta'},'Text '+(start+1)+'–'+end+' / '+text.length),start>0&&h('button',{onClick:()=>{let before=0;for(let at=0;at<start;){before=at;at=endAt(at);}turn(before);}},'Previous text'),end<text.length&&h('button',{onClick:()=>turn(end)},'Next text'))];
   },
   linked:function(c,p,results=[]){
    if(!c||!p||!c.sent&&!c.request)return [];
    if(linked?.id!==c.id){linked={id:c.id,rows:[],known:new Set(),checked:new Set(),busy:false,page:1,more:false,reason:'',start:0,current:null};api.linkedRead(c.id);}
    const rows=p.tasks.filter(t=>t.conversation?.id===c.id),current=new Set(rows.map(t=>t.n));
    for(const n of linked.known)if(!current.has(n)&&!linked.checked.has(n)&&!linked.rows.some(t=>t.n===n)&&!Object.values(linkedReads).some(r=>r.conversation===c.id&&r.task===n))api.linkedRead(c.id,n);
    const tasks=[...rows,...linked.rows.filter(t=>!current.has(t.n))],known=new Set(tasks.map(t=>t.n)),all=[...tasks,...results.filter(r=>r.conversation===c.id&&r.root===p.root&&!known.has(r.report)).map(r=>({n:r.report,title:r.title,report:r}))];linked.total=all.length;linked.start=Math.min(linked.start,Math.max(0,all.length-1));const page=all.slice(linked.start,linked.start+PAGE);
    for(const t of page){linked.known.delete(t.n);linked.known.add(t.n);}while(linked.known.size>ROWS)linked.known.delete(linked.known.values().next().value);while(linked.checked.size>ROWS)linked.checked.delete(linked.checked.values().next().value);
    if(!page.some(t=>t.n===linked.current))linked.current=(page.find(t=>t.state==='ask')||page[0])?.n;
    return page;
   },
   linkedCurrent:function(){return linked?.current;},
   linkedRead:function(conversation,task,page=1){
    if(!linked||linked.id!==conversation||!task&&linked.busy)return;
    if(task)linked.checked.add(task);
    const id=api.id();linkedReads[id]={conversation,task,page};if(!task)linked.busy=true;
    ui.send({a:'conversation-dialogue',action:'tasks',id,conversation,...(task?{task}:{page})});
    ui.defer(()=>{if(linkedReads[id])api.receive({dialogueReceipt:{id,conversation,action:'tasks',state:'unknown',reason:'Task readback missing; nothing was replayed'}});},30000);
   },
   linkedTools:function(){return linked&&[linked.reason&&h('p',{role:'status'},'Task history unknown: '+linked.reason),h('p',{class:'meta'},'Linked tasks · '+(linked.start+1)+'–'+Math.min(linked.total,linked.start+PAGE)+' / '+linked.total+' loaded'+(linked.more?' · older board pages available':'')),h('div',{class:'conversation-tools'},linked.start>0&&h('button',{'data-c':'linked-page','data-start':Math.max(0,linked.start-PAGE)},'Previous tasks'),linked.start+PAGE<linked.total&&h('button',{'data-c':'linked-page','data-start':linked.start+PAGE},'Next tasks'),h('button',{'data-c':'linked-refresh',disabled:linked.busy},'Refresh task history'),linked.page>1&&h('button',{'data-c':'linked-newer',disabled:linked.busy},'Newer results'),linked.more&&h('button',{'data-c':'linked-older',disabled:linked.busy},'Older results'))];},
   current:function(){return items.find(c=>c.id===selected);},
   bodyRead:function(c){if(!c?.lazy||c.bodyReason||bodyReads.has(c.id))return;const id=api.id(),op={id,revision:c.revision};bodyReads.set(c.id,op);while(bodyReads.size>8){const [key,old]=bodyReads.entries().next().value;ui.cancel(old.timer);bodyReads.delete(key);}ui.send({a:'conversation-dialogue',action:'body',conversation:c.id,revision:c.revision,id});op.timer=ui.defer(()=>{if(bodyReads.get(c.id)===op){bodyReads.delete(c.id);c.bodyReason='Saved draft read unknown; refresh to retry. Nothing was sent.';api.note(c.bodyReason,c.id);draw();}},30000);},
   bodyTrim:function(){
    let bytes=0;for(const [id]of [...bodyCache].reverse()){const c=items.find(c=>c.id===id);if(!c){bodyCache.delete(id);continue;}bytes+=new TextEncoder().encode(JSON.stringify(api.local(c))+JSON.stringify(c.sent||'')).length;if(id===selected||c.sending||c.reply||(c.saved??0)!==(c.version??0)||pending?.conversation===id||queue.some(o=>o.conversation===id))continue;if(bodyCache.size>2||bytes>2*1024*1024){bodyCache.delete(id);c.preview_title=api.title(c);c.lazy=true;delete c.text;delete c.files;if(c.sent)c.sent={root:c.sent.root,runtime:c.sent.runtime,revision:c.sent.revision};}}bodyNotice=bodyCache.size>2?'Drafts remain in memory while edits or delivery receipts are pending ('+bodyCache.size+' sources).':'';
   },
   ids:function(){return items.map(c=>c.id);},
   opened:function(){return opened;},
   sidebarState:function(){return sidebar;},
   remember:function(id,root,keepSearch=false){const prior=opened.find(r=>r.id===id);if(prior)prior.root=root||prior.root;else opened.push({id,root:root||null});if(!keepSearch){search='';searchOpen=false;}api.nav();},
   // C2/C7: one selected identity for owner, task and event rows; only view identities persist.
   sync:function(rows,legacy){
    if(!synced){synced=true;if(!unified&&legacy)selected=legacy;}
    const row=items.find(c=>c.id===selected&&!c.deleted)||rows.find(r=>r.id===selected);
    if(row&&scopeReturn)list=row.archived?'archive':'recent';if(!row&&!recoveryDrafts.some(c=>c.id===selected)&&!items.some(c=>c.id===selected&&c.deleted)){if(selected)api.note('The selected source or saved position is unavailable. Refresh or choose another thread.','list');selected=null;}else if(row&&!!row.archived!==(list==='archive'))selected=null;
    selection[list]=selected;onSelect?.(selected);return selected;
   },
   capture:function(){const el=menu.querySelector('.conversation-reader'),id=el?.parentElement.dataset.threadId;if(id&&!el.querySelector('.dialogue-transcript')&&positions[id]===undefined){const a=api.mark(el);api.place(id,a?.board?api.boardPosition(a):Math.round(el.scrollTop));}},
   // Stable board/native record offsets share the bounded navigation; plain readers retain pixel positions.
   boardPosition:function(a){return {key:a.key,y:Math.round(a.y),...(a.before!==undefined?{before:a.before}:{}),...(a.g?{g:a.g}:{})};},
   place:function(id,value){
    if(id!==undefined){delete positions[id];if(value!==undefined)positions[id]=value;}
    for(const k of Object.keys(positions)){if(Object.keys(positions).length<=SAVED&&JSON.stringify(positions).length<=SAVED_BYTES)break;delete positions[k];}
   },
   anchored:function(v){return !!v&&typeof v==='object'&&Number.isFinite(v.y)&&(/^(?:(task|brief|question|answer|result):[0-9]+(?::[0-9]+)?|report:[a-z0-9:-]+)$/.test(v.key)||Number.isSafeInteger(v.at)&&v.at>=0&&Number.isSafeInteger(v.i)&&v.i>=0&&(v.part===undefined||Number.isSafeInteger(v.part)&&v.part>=0&&v.part<=16*1024*1024));},
   editable:function(c){return !!c&&!c.lazy&&(!owner.conversation||owner.conversation===c.id)&&!(owner.detached||[]).includes(c.id)&&!c.deleted;},
   // The draft names only an unsent request; a sent or native thread keeps its title while the owner types the next message.
   title:function(c){const line=s=>(s||'').split('\n').find(s=>s.trim())?.trim();return c.title||(c.lazy?c.preview_title:null)||(c.sent||c.request?line(c.sent?.text)||c.preview_title||'Native request':line(c.text)||'New request');},
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
    reads[id]={conversation,kind,gen:view.gen,expected:kind==='initial'?view.anchor?.key:undefined,...extra};view.busy[kind]=id;
    reads[id].timer=ui.defer(()=>{if(reads[id]){api.settle({id,conversation,state:'unknown',reason:'Dialogue read receipt missing'});draw();}},30000);
    ui.send({a:'conversation-dialogue',id,conversation,action:kind==='status'?'status':'transcript',...extra});
   },
   pageValid:function(v){return !!v&&Array.isArray(v.items)&&v.items.every(i=>i&&typeof i.role==='string'&&typeof i.text==='string');},
   settle:function(r){
    const op=reads[r.id];if(!op||r.conversation!==op.conversation)return;ui.cancel(op.timer);delete reads[r.id];
    if(view?.id!==op.conversation)return;if(view.busy[op.kind]===r.id)view.busy[op.kind]=null;
    if(op.gen!==view.gen)return;
    const v=r.value,el=menu.querySelector('.conversation-reader');
    if(op.kind==='status'){sidebarProjection=null;if(r.state==='accepted'){view.status=v;view.statusReason='';const c=api.current();if(c&&Object.hasOwn(v,'updated')){c.updated=v.updated;activityTimes[c.id]={updated:v.updated,reason:v.updated_reason};}api.form();}else {view.status=null;view.statusReason=r.reason||'Status unknown';activityTimes[view.id]={updated:null,reason:view.statusReason};}return;}
    if(op.kind==='before'&&op.before!==view.start||op.kind==='after'&&op.after!==view.end||op.kind==='tail'&&op.after!==view.size)return;
    if(r.state==='accepted'&&!api.pageValid(v)){view.reason='Transcript response invalid; source unknown. Refresh before reading.';return;}
    if(r.state!=='accepted'){view.reason=r.reason||'Transcript unknown';if(/shrank|replaced/.test(view.reason)){view.lost=view.reason;view.stick=false;}return;}
    // Q-A3/A5: a reply for a replaced window, or for a cursor the window no longer ends at, is obsolete
    if(op.gen!==view.gen)return;
    if(v.generation&&view.source&&v.generation!==view.source&&(view.items.length||view.end>0||view.anchor)){
     view.gen++;view.items=[];view.start=view.end=null;if(!view.anchor?.board)view.anchor=null;view.stick=false;view.lost='Transcript source changed. Choose Latest to open the current source.';view.reason=view.lost;api.place(view.id);return;
    }
    const priorSize=view.size;view.source=v.generation||view.source;view.reason='';view.size=v.size;if(op.kind==='tail'){if(v.size>priorSize&&!view.stick)view.unseen=true;return;}
    // The user scroll owns the anchor; a late read must not sample an intermediate reflow.
    const have=new Set(view.items.map(api.key)),fresh=v.items.filter(item=>!have.has(api.key(item)));
    if(op.kind==='initial'){
     // a saved anchor reloads the range that starts at its record; a record that is gone is said, then Latest opens
     if(op.after!==undefined&&!v.items.some(item=>api.key(item)===op.expected)){view.lost='Saved reading position is no longer in the transcript. Choose Latest to open the current source.';view.reason=view.lost;view.from=undefined;view.anchor=null;view.stick=false;api.place(view.id);return;}
     view.items=v.items;view.start=v.start;view.end=v.end;view.from=undefined;
    }
    else if(op.kind==='before'&&op.before===view.start){view.items=[...fresh,...view.items];view.start=v.start;view.quiet.before=fresh.length?0:view.quiet.before+1;if(fresh.length)view.announce='Earlier messages loaded';}
    else if(op.kind==='after'&&op.after===view.end){view.items.push(...fresh);view.end=v.end;view.quiet.after=fresh.length?0:view.quiet.after+1;if(fresh.length){view.announce=fresh.length+(fresh.length===1?' new message':' new messages');if(!view.stick)view.unseen=true;}}
   },
   key:function(item){return item.at+':'+item.i;},
   bytes:function(item){if(!recordBytes.has(item))recordBytes.set(item,new TextEncoder().encode(item.text||'').length);return recordBytes.get(item);},
   // Q-A3: the first visible native or board record and its offset from the viewport top
   mark:function(el){
    const top=el.getBoundingClientRect().top;
    // Nested disclosures have overlapping rectangles: use the deepest visible paragraph/record, never binary search.
    const nodes=[...el.querySelectorAll('.dialogue-transcript [data-key],[data-reading-key],[data-native-part]')].filter(n=>n.checkVisibility()&&![...n.querySelectorAll('[data-reading-key],[data-key],[data-native-part]')].some(c=>c.checkVisibility()));
    let first=null,last=null;for(const node of nodes){const r=node.getBoundingClientRect();if(r.height<=0||r.bottom<=top)continue;if(!first)first=node;if(r.top<=top)last=node;else if(last)break;}
    const node=last||first;if(!node)return null;const page=node.closest('[data-read-before]');return {key:node.dataset.readingKey||node.dataset.key||node.closest('[data-key]').dataset.key,board:!!node.dataset.readingKey,...(page?{before:Number(page.dataset.readBefore),g:page.dataset.sourceGeneration}:{}),...(node.dataset.nativePart!==undefined?{part:Number(node.dataset.nativePart)}:{}),y:node.getBoundingClientRect().top-top};
   },
   hold:function(el,state=view){
    if(!state)return;
    const a=state.anchor,record=a&&[...el.querySelectorAll('[data-reading-key],[data-key]')].filter(n=>n.checkVisibility()&&(a.board?n.dataset.readingKey:n.dataset.key)===a.key).at(-1),node=a?.part!==undefined?[...(record?.querySelectorAll('[data-native-part="'+a.part+'"]')||[])].filter(n=>n.checkVisibility()).at(-1)||record:record;
    const delta=state.stick?el.scrollHeight-el.clientHeight-el.scrollTop:node?node.getBoundingClientRect().top-el.getBoundingClientRect().top-a.y:0;
    if(Math.abs(delta)>.5){correcting=true;el.scrollTop+=delta;window.requestAnimationFrame(()=>{correcting=false;});}return state.stick||!!node;
   },
   layout:function(now=false){if(layoutFrame!==null){if(!now)return;window.cancelAnimationFrame(layoutFrame);layoutFrame=null;}const correct=()=>{layoutFrame=null;const el=menu.querySelector('.conversation-reader');if(!el)return;const id=el.parentElement.dataset.threadId,p=positions[id],s=live?.thread===selected?live:view?.id===selected?view:null;if(s){const held=api.hold(el,s);if(!s.stick&&held){s.anchor=api.mark(el)||s.anchor;api.storeAnchor(id,s);}}else if(p?.key){if(!api.hold(el,{anchor:{key:p.key,board:true,y:p.y}}))return;const fresh=api.mark(el);if(fresh?.board){const next=api.boardPosition(fresh);if(JSON.stringify(next)!==JSON.stringify(p)){api.place(id,next);api.nav();}}}};if(now)correct();else layoutFrame=window.requestAnimationFrame(correct);},
   observe:function(reader){
    resize?.disconnect();observed=reader;if(!reader)return;
    resize??=new window.ResizeObserver(()=>api.layout());resize.observe(reader);for(const node of reader.children)resize.observe(node);
   },

   storeAnchor:function(id,v){const a=v.anchor&&v.items.find(i=>api.key(i)===v.anchor.key),next=v.stick?undefined:v.anchor?.board?{key:v.anchor.key,y:Math.round(v.anchor.y)}:!a?undefined:{at:a.at,i:a.i,y:Math.round(v.anchor.y),...(v.source?{g:v.source}:{}),...(v.anchor.part!==undefined?{part:v.anchor.part}:{})};if(JSON.stringify(positions[id])!==JSON.stringify(next)){api.place(id,next);api.nav();}},
   // Q-A4: read earlier or later records while they are within one viewport of the visible area; short and
   // ignored-only pages leave the edge near, so the next draw reads again until the viewport fills or the cursor ends
   edge:function(el,v=view){
    const log=el.querySelector('.dialogue-transcript');if(!log||v.end===null||v.reason||v.busy.initial)return;
    const box=el.getBoundingClientRect(),r=log.getBoundingClientRect(),margin=el.clientHeight;
    if(v.start>0&&!v.busy.before&&v.quiet.before<FILL&&r.top>box.top-margin)(v===live?api.workerRead:api.read)('before',{before:v.start});
    if(v.end<v.size&&!v.busy.after&&v.quiet.after<FILL&&r.bottom<box.bottom+margin)(v===live?api.workerRead:api.read)('after',{after:v.end});
   },
   // Q-A5: evict whole records beyond two viewports, the farther side first; records that hold a selection stay
   // mounted while it is in use, so copy keeps its text
   trim:function(el,v=view){
    const items=v.items,bytes=items.reduce((n,i)=>n+api.bytes(i),0);if(items.length<=WINDOW&&bytes<=BYTES){v.limitReason='';return false;}v.limitReason=items.length>WINDOW||bytes>BYTES?'Resident history: '+items.length+' records / '+(bytes/1048576).toFixed(1)+' MiB; the visible passage or selected text is pinned until it can be evicted safely.':'';
    const log=el.querySelector('.dialogue-transcript'),byKey=new Map([...log.querySelectorAll('[data-key]')].map(n=>[n.dataset.key,n])),nodes=items.map(item=>{const node=byKey.get(api.key(item));return node?.closest('.dialogue-steps:not([open])')||node;});if(nodes.some(n=>!n))return false;
    const box=el.getBoundingClientRect(),over=el.clientHeight*2,excess=Math.max(0,items.length-WINDOW),sel=window.getSelection(),held=[];
    if(sel&&!sel.isCollapsed)for(const n of [sel.anchorNode,sel.focusNode]){const node=n?.parentElement?.closest('[data-key]'),index=node&&log.contains(node)?items.findIndex(i=>api.key(i)===node.dataset.key):-1;if(index>=0)held.push(index);}
    let top=0,bottom=items.length;
    while(top<items.length&&nodes[top].getBoundingClientRect().bottom<box.top-over)top++;
    while(bottom>top&&nodes[bottom-1].getBoundingClientRect().top>box.bottom+over)bottom--;
    if(held.length){top=Math.min(top,...held);bottom=Math.max(bottom,...held.map(k=>k+1));}
    let wanted=excess;if(bytes>BYTES){let remaining=bytes;for(let i=0;i<items.length&&remaining>BYTES;i++){remaining-=api.bytes(items[i]);wanted=Math.max(wanted,i+1);}}
    const far=items.length-bottom,b=top>=far?Math.min(far,wanted-Math.min(top,wanted)):Math.min(far,wanted);
    let a=Math.min(top,wanted-b);while(a>0&&items[a].i>0)a--;
    let cut=items.length-b;while(cut<items.length&&items[cut].i>0)cut++;
    if(!a&&cut===items.length)return false;
    if(cut<items.length)v.end=items[cut].at;if(a)v.start=items[a].at;
    v.items=items.slice(a,cut);return true;
   },
   // Q-A4: detached from the end, or new output arrived below the reader
   latest:function(){return !!view&&!view.stick&&(view.end!==null||!!view.lost);},
   jump:function(){return api.latest()&&h('button',{'data-c':'latest',class:'dialogue-latest'},view.unseen?'New messages · Jump to latest':'Jump to latest');},
   // only Latest resumes following; a window that does not hold the end is replaced by the newest page
   scrollIntent:function(kind){scrollIntent=kind;ui.cancel(scrollIntentTimer);scrollIntentTimer=ui.defer(()=>{scrollIntent=null;},250);},
   pause:function(){const el=menu.querySelector('.conversation-reader'),s=live?.thread===selected?live:view?.id===selected?view:null;if(s&&s.stick){s.stick=false;s.anchor=el&&api.mark(el);draw();}},
   follow:function(){const reset=!!view.reason;view.unseen=false;view.stick=true;view.lost='';view.reason='';api.place(view.id);if(reset||view.end===null||view.end<view.size||view.items.length===0){view.gen++;view.busy={};if(reset){view.items=[];view.start=view.end=null;view.source=null;view.anchor=null;}api.read('initial');}},
   open:function(c){
    if(view?.id!==c.id){
     reads={};const saved=api.anchored(positions[c.id])?positions[c.id]:null;
     view={id:c.id,items:[],start:null,end:null,size:null,gen:0,from:saved?.at,source:saved?.g,reason:'',lost:'',announce:'',unseen:false,quiet:{before:0,after:0},status:null,statusReason:'',busy:{},stick:!saved,anchor:saved&&{key:saved.key||saved.at+':'+saved.i,board:!!saved.key,y:saved.y,...(saved.part!==undefined?{part:saved.part}:{})},ticks:0,form:null};
     api.read('initial',saved?.at!==undefined?{after:saved.at}:{});api.read('status');
    }
    if(!pollTimer)pollTimer=ui.defer(api.poll,2000);
   },
   // D4: poll every 2 s while the view is open; status, which probes the native holder, every 10 s.
   poll:function(){
    pollTimer=null;const c=api.current();
    if(!view||c?.id!==view.id||!menu.querySelector('.dialogue-transcript'))return;
    if(!view.lost&&!view.busy.initial&&!view.busy.after&&!view.busy.tail){if(view.end===null)api.read('initial',view.from!==undefined?{after:view.from}:{});else if(view.end>=view.size)api.read('after',{after:view.end});else api.read('tail',{after:view.size});}
    if(++view.ticks%5===0&&!view.busy.status)api.read('status');
    pollTimer=ui.defer(api.poll,2000);
   },
   form:function(){
    if(view.form?.dirty)return;const w=view.status?.watch;
    view.form=w?.prompt?{prompt:w.prompt,every:String(w.every),events:[...(w.events||[])],dirty:false}:{prompt:'',every:'30',events:[],dirty:false};
   },
   // D7: the Status control shows the watch, last and next turn, outcome and last assistant line; it sets and removes the watch.
   status:function(c,disabled){
    const v=view?.id===c.id?view:null,s=v?.status,w=s?.watch,f=v?.form||{prompt:'',every:'30',events:[]},watching=!!w?.prompt,every=Number(f.every);
    const watchText=!s?(v?.statusReason?'Status unknown: '+v.statusReason:'Reading status…'):w==='no watch'?'no watch':w?.unknown?'Watch unknown: '+w.unknown:watching?'watch every '+w.every+' min':'Watch unknown';
    const t=s?.turn,row=(label,value)=>[h('dt',null,label),h('dd',null,value)];
    return h('div',{class:'dialogue-status'},h('p',{class:'meta'},'Status · '+watchText),
     h('div',{class:'dialogue-status-body'},
      h('dl',null,
       watching?[row('Prompt',w.prompt),row('Every',w.every+' minutes'),row('Events',(w.events||[]).join(', ')||'none'),row('Last watch dispatch',time.stamp(w.last_turn,'Watch dispatched')),row('Next scheduled turn',time.stamp(w.next_due,'Next scheduled turn')),row('Last outcome',w.last_outcome||'none')]:row('Watch',watchText),
       row('Latest turn',t?[t.state+' · '+(t.outcome||(t.state==='done'?'outcome unknown':'outcome pending'))+' · ',time.stamp(t.at,'Turn submitted')]:time.stamp(null,'Turn submitted',s?'No turn timestamp in the status record':v?.statusReason||'Turn status unavailable')),
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
     v?.start>0&&h('button',{'data-c':'earlier',disabled:!!v.busy.before||!!v.lost},v.busy.before?'Loading earlier…':'Load earlier'),
     v?.limitReason&&h('p',{role:'status',class:'meta'},v.limitReason),v?.lost&&h('p',{role:'status',class:'dialogue-unknown'},v.lost),
     v?.reason&&v.reason!==v.lost&&h('p',{role:'status',class:'dialogue-unknown'},'Transcript unknown: '+v.reason+(v.items.length?'. Showing what was read before.':'')),
     !v||v.end===null?!v?.reason&&h('p',{class:'meta'},'Reading transcript…'):v.items.length?api.steps(v.items,api.state(c)==='working',item=>{const key=api.key(item);return item.role==='tool'?h('p',{key,'data-key':key,class:'dialogue-tool',title:api.short(item.text)},h('span',{class:'tool-label'},'Tool · '+api.short(item.text)),time.stamp(item.updated,'Tool recorded')):item.role==='brief'?h('details',{key,'data-key':key,class:'dialogue-brief'},h('summary',{class:'meta'},'Setup brief',time.stamp(item.updated,'Setup brief recorded')),api.nativeItem(item,null)):api.nativeItem(item,label[item.role]||item.role);}):h('p',{class:'meta'},v.start>0?'No messages in this chunk; Load earlier reads more.':'No messages in the transcript yet.'),
     v?.end<v?.size&&v.quiet.after>=FILL&&h('button',{'data-c':'later',disabled:!!v.busy.after},v.busy.after?'Loading later…':'Load later'),
     c.archived&&api.jump())];
   },
   // #1918 A6: consecutive tool calls fold into one collapsible line (docs/design/conversations-usability-review.md#tool-collapse-1918);
   // a message ends the group. `current`: the worker runs, so the last group names its latest step live.
   steps:function(items,current,render){
    const out=[],tool=item=>{const key=api.key(item);return h('p',{key,'data-key':key,class:'dialogue-tool',title:api.short(item.text)},h('span',{class:'tool-label'},'Tool · '+api.short(item.text)),time.stamp(item.updated,'Tool recorded'));};
    for(let i=0;i<items.length;){
     if(items[i].role!=='tool'){out.push(render(items[i]));i++;continue;}
     let j=i;while(j<items.length&&items[j].role==='tool')j++;
     const run=items.slice(i,j),key=api.key(run[0]),now=current&&j===items.length;
     out.push(run.length===1?tool(run[0]):h('details',{key,'data-key':key,class:'dialogue-steps'},
      h('summary',{class:'dialogue-tool'},h('span',{class:'tool-label'},api.stepSummary(run)+(now?' · now: '+api.short(run.at(-1).text):'')),time.stamp(run.at(-1).updated,'Latest tool recorded',undefined,false)),run.map(tool)));
     i=j;
    }
    return out;
   },
   // "N steps · files a.rs, b.js · 2 commands · tests cargo test": each list deduplicated and cut to three names
   stepSummary:function(run){
    const files=new Set(),tests=new Set();let commands=0;
    for(const item of run){
     const text=String(item.text),name=item.tool_name||text.split(' ')[0],arg=text.slice(name.length).trim();
     if(FILE_TOOLS.test(name))for(const f of arg.split(/\s+/).filter(Boolean))files.add(f.split(/[\\/]/).pop());
     else if(TEST.test(arg))tests.add(api.short(arg).slice(0,40));
     else commands++;
    }
    const list=(label,s)=>s.size?label+' '+[...s].slice(0,3).join(', ')+(s.size>3?' +'+(s.size-3):''):'';
    return [run.length+' steps',list('files',files),commands?commands+(commands===1?' command':' commands'):'',list('tests',tests)].filter(Boolean).join(' · ');
   },
   workerStop:function(){if(live){ui.cancel(live.timer);ui.cancel(live.timeout);}live=null;},
   workerClose:function(){workerPaused=true;api.workerStop();},
   // The browser names only a registered project and attempt. The native adapter selects the session (A2); a worker on
   // another host answers through the host link (#1832), and a host that does not answer reads unknown, never empty.
   worker:function(thread,t,p){
    if(workerPaused||!p?.alive||p.complete===false||!(t?.local_session||t?.host)||t.state!=='doing'||t.role!=='worker'||!t.attempt){api.workerStop();return null;}
    if(live?.attempt!==t.attempt||live.thread!==thread||live.root!==p.root){api.workerStop();const saved=api.anchored(positions[thread])?positions[thread]:null;live={thread,root:p.root,attempt:t.attempt,items:[],start:null,end:null,size:null,source:saved?.g,from:saved?.at,gen:0,quiet:{before:0,after:0},reason:'',busy:{},request:null,timer:null,timeout:null,ended:false,stick:!saved,anchor:saved&&{key:saved.key||saved.at+':'+saved.i,board:!!saved.key,y:saved.y,...(saved.part!==undefined?{part:saved.part}:{})}};}
    if(!live.ended&&!live.lost&&!live.request&&!live.timer)live.timer=ui.defer(()=>api.workerRead(live.end===null?'initial':'after'),live.end===null&&!live.reason?0:1000);
    const label={assistant:'Worker',owner:'You · via the board'};
    return h('section',{class:'worker-live','aria-label':'Worker document'},h('p',{class:'meta document-author'},'Live session · '+t.runtime+(t.local_session?'':' on '+t.host)),
     !live.stick&&h('button',{'data-c':'worker-latest',class:'dialogue-latest'},'Jump to latest'),
     h('div',{class:'dialogue-transcript',role:'log','aria-live':'off','aria-label':'Live worker session','data-id':thread},
      live.start>0&&h('button',{'data-c':'worker-earlier',disabled:!!live.request},'Load earlier'),live.end<live.size&&live.quiet.after>=FILL&&h('button',{'data-c':'worker-later',disabled:!!live.request},'Load later'),
      live.limitReason&&h('p',{role:'status',class:'meta'},live.limitReason),live.ended&&h('p',{role:'status',class:'meta'},'Worker attempt ended'),
      live.reason&&h('p',{role:'status',class:'dialogue-unknown'},'Live session unknown: '+live.reason),
      live.end===null?!live.reason&&h('p',{class:'meta'},'Reading session…'):api.steps(live.items,!live.ended,item=>{const key=api.key(item);return item.role==='brief'?h('p',{key,'data-key':key,class:'meta'},'TaskQ brief for this turn',time.stamp(item.updated,'Turn brief recorded')):api.nativeItem(item,item.kind==='answer'?'Your answer · via the board':label[item.role]||item.role);})));
   },
   workerRead:function(kind='after',extra={}){
    const l=live;if(!l||l.request||l.lost)return;ui.cancel(l.timer);l.timer=null;
    if(workerPaused||l.ended||!menu.querySelector('.worker-live')?.checkVisibility()){api.workerStop();return;}
    if(kind==='after'&&l.end<l.size&&!Object.keys(extra).length)return;
    const cursor=Object.keys(extra).length?extra:kind==='initial'?(l.from!==undefined?{after:l.from}:{}):kind==='before'?{before:l.start}:{after:l.end};
    const id=api.id();l.request={id,kind,gen:l.gen,expected:kind==='initial'?l.anchor?.key:undefined,...cursor};l.busy[kind]=id;
    ui.send({a:'conversation-dialogue',id,conversation:l.thread,root:l.root,attempt:l.attempt,action:'transcript',...cursor});
    l.timeout=ui.defer(()=>{if(live===l&&l.request?.id===id){l.request=null;l.busy[kind]=null;l.reason='Live read receipt missing';draw();}},30000);
   },
   workerSettle:function(r){
    const l=live,op=l?.request;if(!op||op.id!==r.id||r.conversation!==l.thread)return false;l.request=null;l.busy[op.kind]=null;ui.cancel(l.timeout);
    if(op.gen!==l.gen||op.kind==='before'&&op.before!==l.start||op.kind==='after'&&op.after!==l.end)return true;
    // Retain the user-owned anchor through receipt/layout races.
    const was=l.reason,first=l.end===null;let fresh=[];
    if(r.state==='accepted'&&r.value?.ended===true){l.ended=true;l.reason='';return true;}
    if(r.state==='accepted'&&!api.pageValid(r.value)){l.reason='Worker transcript response invalid; source unknown. Refresh before reading.';return true;}
    if(r.state!=='accepted')l.reason=r.reason||'Transcript unknown';
    else if(r.value.generation&&l.source&&r.value.generation!==l.source&&(l.items.length||l.end>0||l.anchor)){l.gen++;l.items=[];l.start=l.end=null;l.anchor=null;api.place(l.thread);l.reason='Transcript source changed. Choose Latest to open the current source.';l.lost=true;l.stick=false;}
    else {
     const v=r.value;l.source=v.generation||l.source;l.reason='';l.size=v.size;
     const have=new Set(l.items.map(api.key));fresh=v.items.filter(i=>!have.has(api.key(i)));
     if(op.kind==='initial'&&op.after!==undefined&&!v.items.some(i=>api.key(i)===op.expected)){l.reason='Saved worker reading position is unavailable. Choose Latest to open the current source.';l.lost=true;l.stick=false;return true;}
     if(op.kind==='initial'){l.items=v.items;l.start=v.start;l.end=v.end;l.from=undefined;}
     else if(op.kind==='before'&&op.before===l.start){l.items=[...fresh,...l.items];l.start=v.start;l.quiet.before=fresh.length?0:l.quiet.before+1;}
     else if(op.kind==='after'&&op.after===l.end){l.items.push(...fresh);l.end=v.end;l.quiet.after=fresh.length?0:l.quiet.after+1;}
    }
    if(!first&&!fresh.length&&was===l.reason&&!l.timer)l.timer=ui.defer(()=>api.workerRead('after'),1000);
    return true;
   },
   // #1775: a tool line keeps the file name of an absolute path, never the local directories
   short:function(text){return String(text).replace(/(?<![^\s'"`=(])(?:~\/|\/)(?:[^\s/'"`]+\/)+([^\s/'"`]*)/g,'…/$1');},
   preview:function(){return preview&&h('section',{'aria-label':'Local file preview'},h('button',{'data-c':'preview-close'},'Close preview'),preview.file.mime.startsWith('image/')?h('img',{src:'data:'+preview.file.mime+';base64,'+preview.data,alt:preview.file.name}):h('pre',null,new window.TextDecoder().decode(Uint8Array.from(window.atob(preview.data),c=>c.charCodeAt(0)))));},
   nativeItem:function(item,label=item.role,closed=false,scope=''){
    const key=api.key(item),text=item.text||'',source=closed?'native:'+item.at+':'+item.i:null;let content=text.length<=32768?require('./reports.js').body(text,source,0):null;
    if(text.length>32768){const resident=live||view,cache=(closed?scope:resident?.id||resident?.thread)+'@'+(closed?closedViews.get(scope)?.page?.generation||'':resident?.source||'')+':'+key,stored=closed&&new RegExp('^report:native:'+item.at+':'+item.i+':part:(\\d+)$').exec(positions[closedViews.get(scope)?.thread]?.key||''),anchor=!closed&&resident?.anchor?.key===key?resident.anchor:null,part=anchor?.part??(stored?Number(stored[1]):textPages.get(cache))??0,start=Math.floor(Math.min(part,text.length-1)/32768)*32768,end=Math.min(text.length,start+32768);

     const turn=next=>{textPages.delete(cache);textPages.set(cache,next);while(textPages.size>24)textPages.delete(textPages.keys().next().value);if(closed){const page=closedViews.get(scope);api.place(page.thread,{key:'report:native:'+item.at+':'+item.i+':part:'+next,y:0,before:page.page.end,g:page.page.generation});api.nav();}else if(resident){resident.stick=false;resident.anchor={key,part:next,y:0};api.storeAnchor(resident.id||resident.thread,resident);}draw();};
     content=[require('./reports.js').body(text.slice(start,end),source,start),h('span',{class:'conversation-tools'},h('span',{class:'meta'},'Message text · '+(start/32768+1)+' / '+Math.ceil(text.length/32768)),start>0&&h('button',{onClick:()=>turn(start-32768)},'Previous text'),end<text.length&&h('button',{onClick:()=>turn(end)},'Next text'))];
    }
    return h('article',{key,'data-key':key,...(closed?{'data-reading-key':'report:native:'+item.at+':'+item.i,'data-read-before':closedViews.get(scope)?.page?.end,'data-source-generation':closedViews.get(scope)?.page?.generation}:{}),class:'dialogue-item '+item.role},label&&h('p',{class:'meta'},label,time.stamp(item.updated,label+' message')),h('div',{class:'conversation-text conversation-prose'},content));
   },
   history:function(p,t,render){
    const total=t.history_count??t.history?.length??0,tail=t.history_tail===true?t.history||[]:t.history_tail||[],sourceRevision=t.history_source_revision??t.revision,sourceTotal=total-tail.length;if(!total)return null;
    const key=p.root+'#'+t.n+'@'+t.revision+':'+sourceRevision;let state=historyViews.get(key);if(!state){state={start:0,revision:sourceRevision};historyViews.set(key,state);}historyViews.delete(key);historyViews.set(key,state);while(historyViews.size>24){const key=historyViews.keys().next().value;ui.cancel(historyViews.get(key).timer);historyViews.delete(key);}
    const read=(start,previous=false)=>{if(state.loading)return;state.cursor=start;state.previous=previous;state.start=start;state.reason='';state.limitReason='';if(!t.history_lazy){state.start=previous?Math.max(0,start-64):start;state.rows=(t.history||[]).slice(state.start,state.start+64);state.next=state.start+state.rows.length;return;}state.id=api.id();state.loading=true;delete state.rows;ui.send({a:'conversation-dialogue',action:'history',id:state.id,conversation:'task:'+p.issues+'/'+t.n,root:p.root,revision:sourceRevision,...(previous?{before:start}:{start})});state.timer=ui.defer(()=>{if(state.loading){state.loading=false;state.reason='Recorded questions readback missing; retry without sending.';draw();}},30000);};
    if(sourceTotal&&!state.rows&&!state.loading&&!state.reason)read(state.start);
    return [h('p',{class:'meta'},'Recorded questions · '+total+' · Read only'),tail.length>0&&[h('p',{class:'meta'},'Latest confirmed answer · source refresh pending'),render({...t,history:tail,history_start:sourceTotal},p.root)],state.loading&&h('p',{role:'status'},'Reading recorded questions…'),state.limitReason&&h('p',{role:'status',class:'meta'},state.limitReason),state.reason&&h('p',{role:'status'},state.reason),state.reason&&h('button',{onClick:()=>{read(state.start);draw();}},'Retry history read'),state.rows&&render({...t,history:state.rows,history_start:state.start},p.root),h('div',{class:'conversation-tools'},state.start>0&&h('button',{disabled:state.loading,onClick:()=>{read(state.start,true);draw();}},'Previous questions'),state.next<sourceTotal&&h('button',{disabled:state.loading,onClick:()=>{read(state.next);draw();}},'Next questions'))];
   },
   closed:function(r){const key=r.root+'#'+r.n+'@'+r.sha,thread=api.readingId(r),p=positions[thread];let t=closedViews.get(key);if(!t&&/^report:native:/.test(p?.key||'')&&Number.isSafeInteger(p.before)){t={scheduled:true,loading:true};closedViews.set(key,t);ui.defer(()=>{t.loading=false;api.closedRead(r.root,r.n,r.sha,p.before,true,false,thread);},0);}return h('details',{'data-view-key':'native:'+key},h('summary',null,'Task activity · native transcript'),h('p',{class:'meta'},'Read-only native transcript. Opening sends no input.'),!t&&h('button',{'data-c':'closed-open','data-root':r.root,'data-task':r.n,'data-sha':r.sha},'Open transcript'),t&&[t.loading&&h('p',{role:'status'},'Reading transcript…'),t.limitReason&&h('p',{role:'status',class:'meta'},t.limitReason),t.reason&&h('p',{role:'status'},'Transcript unavailable: '+t.reason),t.reason&&h('button',{'data-c':'closed-open','data-root':r.root,'data-task':r.n,'data-sha':r.sha,disabled:t.loading},'Latest transcript'),t.page&&[t.page.start>0&&h('button',{'data-c':'closed-earlier','data-root':r.root,'data-task':r.n,'data-sha':r.sha,disabled:t.loading},'Load earlier'),h('button',{'data-c':'closed-open','data-root':r.root,'data-task':r.n,'data-sha':r.sha,disabled:t.loading},'Latest messages'),t.page.items.map((item,i)=>api.nativeItem({...item,at:item.at??t.page.start,i:item.i??i},{user:'You',assistant:'Agent'}[item.role]||item.role,true,key)),!t.page.items.length&&h('p',{class:'meta'},'No messages in this chunk.')]]);},
   closedRead:function(root,task,sha,before,restore=false,latest=false,thread='result:'+root+'/'+task+'@'+sha){const key=root+'#'+task+'@'+sha,prior=closedViews.get(key);if(prior?.loading)return;const t={...prior,root,task,sha,before,restore,latest,thread,id:api.id(),loading:true,reason:''};closedViews.delete(key);closedViews.set(key,t);while(closedViews.size>1){const old=closedViews.keys().next().value;ui.cancel(closedViews.get(old).timer);closedViews.delete(old);}ui.send({a:'completed',root,task,id:t.id,transcript:true,...(before==null?{}:{before})});t.timer=ui.defer(()=>api.closedReceive({root,id:t.id,reason:'No transcript response within 65 seconds'}),65000);draw();},
   closedReceive:function(r){const t=[...closedViews.values()].find(t=>t.id===r.id&&t.root===r.root);if(!t?.loading)return;ui.cancel(t.timer);t.loading=false;t.reason=r.reason||r.transcript?.reason||'';if(r.transcript?.items){const p=positions[t.thread],parts=/^report:native:(\d+):(\d+)/.exec(p?.key||'');if(t.restore&&(p?.g&&r.transcript.generation!==p.g||parts&&!r.transcript.items.some(i=>i.at===Number(parts[1])&&i.i===Number(parts[2])))){t.reason='Saved native source position unavailable. Choose Latest to open the current source.';delete t.page;}else {t.page=r.transcript;const bytes=t.page.items.reduce((n,i)=>n+api.bytes(i),0);t.limitReason=bytes>1024*1024?'Selected transcript: '+(bytes/1048576).toFixed(1)+' MiB pinned while reading; inactive pages are evicted.':'';if((t.latest||!t.restore&&t.before!==undefined)&&t.page.items.length){const last=t.latest?t.page.items.at(-1):t.page.items[0];api.place(t.thread,{key:'report:native:'+last.at+':'+last.i,y:0,before:t.page.end,g:t.page.generation});}}}let bytes=0;for(const [key,v]of [...closedViews].reverse()){bytes+=(v.page?.items||[]).reduce((n,i)=>n+api.bytes(i),0);if(bytes>1024*1024&&v!==t&&!v.loading)closedViews.delete(key);}draw();},
   runtime:function(c){const base=snapshot.ai?.runtime;return (base?.choices||[base]).find(r=>r?.id===(c?.runtime||base?.id))||{id:c?.runtime,available:false,reason:'Selected runtime unavailable; no fallback.'};},
   local:function(c){return {title:c.title||'',root:c.root||'',...(c.runtime?{runtime:c.runtime}:{}),text:c.text||'',caret:c.caret||[0,0],scroll:c.scroll||0,next:!!c.next,...(c.replyAck?{reply_ack:c.replyAck}:{})};},
   // #1773 A4: the host refuses fractional scroll offsets and navigation over 4096 bytes; whole pixels and the newest opened threads fit
   nav:function(){ui.cancel(navTimer);navTimer=ui.defer(()=>{const whole=Object.fromEntries(Object.entries(scrolls).map(([k,v])=>[k,Math.max(0,Math.round(v)||0)]));let navigation;
    for(;;){navigation={list,search,filter:resultFilter,selected:api.current()?.id||null,listOnly,scrolls:whole,threads:{selected,selection,opened,positions,focuses,completed,scopeReturn}};if(JSON.stringify(navigation).length<=4096)break;if(opened.length){opened.shift();continue;}if(Object.keys(focuses).length>1){delete focuses[Object.keys(focuses)[0]];continue;}const key=Object.keys(positions)[0];if(key){delete positions[key];continue;}api.note('Navigation exceeds saved-state capacity; current view kept in this window.');return;}
    ui.send({a:'conversation-navigation',navigation});checkpoint();},100);},
   order:function(){return sidebarItems;},
   readingId:function(report){return report?'result:'+report.root+'/'+report.n+'@'+report.sha:selected;},
   navigation:function(report){return {index:sidebarItems.findIndex(r=>r.id===selected),length:sidebarItems.length,position:positions[api.readingId(report)]};},
   reading:function(key,report){const id=api.readingId(report),prior=positions[id];api.place(id,{key,y:0,...(/^report:native:/.test(key)&&prior?.before!==undefined?{before:prior.before,g:prior.g}:{})});api.nav();},
   move:function(step){const rows=api.order(),at=rows.findIndex(r=>r.id===selected),r=rows[at+step];if(r)api.activate(r);},
   activate:function(r){if(r.report)onSelect?.(r.id);api.select(r.id,!!items.find(c=>c.id===r.id));},
   enter:function(id,archived){if(!scopeReturn)scopeReturn={selected,list,filter:resultFilter,search,completed};const row=items.find(c=>c.id===id)||sidebarItems.find(r=>r.id===id),target=(archived??row?.archived??Object.hasOwn(sidebar.archived||{},id))?'archive':'recent';if(list!==target){selection[list]=selected;list=target;}api.select(id,false);},
   filterName:function(){return resultFilter;},
   hasContext:function(){return !!selected||!!search||resultFilter!=='all';},
   archived:function(id){if(selected===id){scopeReturn=null;revealed=null;selected=null;selection[list]=null;onSelect?.(null);api.nav();}},
   context:function(){return {selected,list,filter:resultFilter,search,completed};},
   restoreContext:function(v){list=v.list;resultFilter=v.filter;search=v.search;completed=v.completed;scopeReturn=null;selected=v.selected;onSelect?.(selected);api.nav();},
   filter:function(value,criteria){navigated=true;api.flush();stableOrder.clear();stableGroups.clear();sidebarProjection=null;resultFilter=value;list='recent';selected=null;onSelect?.(null);if(criteria)completed=views.criteria(criteria);else completed=null;scopeReturn=null;revealed=null;api.nav();},
   returnScope:function(){if(!scopeReturn)return;const prior=scopeReturn;scopeReturn=null;list=prior.list;resultFilter=prior.filter;search=prior.search;completed=prior.completed;api.select(prior.selected,false);},
   select:function(id,focus=true){navigated=true;revealed=id;const prior=api.current();if(prior&&(prior.version||0)!==(prior.saved||0))api.save(prior);selected=id;if(bodyCache.has(id)){bodyCache.delete(id);bodyCache.set(id,true);}onSelect?.(id);selection[list]=id;listOnly=false;focusDraft=focus;confirmDelete=false;preview=null;api.nav();draw();},
   // #1699 A2: open one owner conversation in its list without acknowledging anything
   reveal:function(id,focus=true,keepSearch=false){const c=items.find(c=>c.id===id);if(!c)return false;list=c.archived?'archive':'recent';if(!keepSearch){search='';searchOpen=false;}api.select(id,focus);return true;},
   // #1699 A7: a restored task or event thread opens in Recent
   show:function(name){navigated=true;if(list!==name){selection[list]=selected;list=name;selected=selection[list]||null;onSelect?.(selected);api.nav();}},
   resume:function(){readerIdentity=null;return !!focuses[selected];},
   fresh:function(copy){navigated=true;
    const prior=api.current();if(prior&&(prior.version||0)!==(prior.saved||0))api.save(prior);
    const c={id:api.id(),revision:0,title:'',root:copy?.root||snapshot.projects?.[0]?.root||'',runtime:copy?.runtime||snapshot.ai?.runtime?.id,text:copy?.text||'',caret:copy?.caret||[0,0],scroll:0,files:[],archived:false,version:1};
    resultFilter='all';completed=null;scopeReturn=null;resultPages.clear();stableOrder.clear();stableGroups.clear();sidebarProjection=null;scrolls.recent=0;
    items.push(c);list='recent';search='';searchOpen=false;selected=c.id;onSelect?.(c.id);selection.recent=c.id;listOnly=false;focusDraft=true;api.save(c);api.nav();draw();
   },
   save:function(c){
    checkpoint();
    if(!api.editable(c)||c.lazy)return;
    if((c.saved??-1)===(c.version||0)||pending?.conversation===c.id&&pending.action==='save'&&pending.version===c.version)return;
    const existing=queue.find(o=>o.conversation===c.id&&o.action==='save');
    const op={conversation:c.id,action:'save',draft:api.local(c),version:c.version||0};
    if(existing)Object.assign(existing,op);else queue.push(op);
    api.next();
   },
   next:function(){
    if(!pending&&!failure&&!queue.length&&recoveryQueue.length){const op=recoveryQueue.shift();failure={...op,recovered:true,reason:'Recovered local '+op.action+' needs an explicit retry; other recovered actions remain paused.'};api.note(failure.reason,op.conversation);draw();return;}
    if(pending||!queue.length||failure)return;
    const op=queue.shift(),c=items.find(c=>c.id===op.conversation);if(!c||!api.editable(c)&&!['archive','restore'].includes(op.action)){api.next();return;}
    if(op.action!=='save')api.note('',op.conversation);
    pending={...op,id:api.id(),revision:c.revision||0,version:op.version??c.version??0};
    checkpoint();
    ui.send({a:op.action==='window'?'conversation-window':'conversation-action',...pending});
    ui.cancel(timer);timer=ui.defer(()=>{if(pending){if(pending.action==='reply'){const c=items.find(c=>c.id===pending.conversation);if(c)c.reply={id:pending.id};}failure={...pending,reason:'Native receipt missing; refresh or retry saving. Submitted requests must use Check delivery.'};pending=null;api.note(failure.reason,failure.conversation);draw();}},30000);
   },
   action:function(action,extra={},id=selected){
    const c=items.find(c=>c.id===id);if(!c||!api.editable(c)&&!['archive','restore'].includes(action)||failure||c.archived&&!['restore','delete','preview'].includes(action))return;
    ui.cancel(saveTimer);api.save(c);
    if(action==='send'&&(c.sending||c.sent||c.request)||action==='reply'&&c.sending)return;
    if(action==='send'||action==='reply')c.sending=true;
    queue.push({conversation:c.id,action,...extra});api.next();draw();
   },
   pollActivity:function(){
    if(activityRead||activityTimer||!menu.querySelector('.conversation-list'))return;
    const visible=new Set([selected,...[...menu.querySelectorAll('.conversation-row[data-id]')].map(n=>n.dataset.id)]),candidates=items.filter(c=>visible.has(c.id)&&api.launched(c)&&!!c.archived===(list==='archive')&&!c.deleted).sort((a,b)=>(activityChecks[a.id]||0)-(activityChecks[b.id]||0));
    if(!candidates.length)return;const c=candidates[0],wait=(activityChecks[c.id]||0)+10000-Date.now();
    if(wait>0){activityTimer=ui.defer(()=>{activityTimer=null;api.pollActivity();},wait);return;}
    const id=api.id();delete activityChecks[c.id];activityChecks[c.id]=Date.now();while(Object.keys(activityChecks).length>24){const key=Object.keys(activityChecks)[0];delete activityChecks[key];delete activityTimes[key];}activityRead={id,conversation:c.id};
    ui.send({a:'conversation-dialogue',id,conversation:c.id,action:'activity'});
    ui.defer(()=>{if(activityRead?.id===id){activityTimes[c.id]={updated:null,reason:'Activity read receipt missing'};activityRead=null;draw();}},30000);
   },
   sidebar:function(action,key,value,root){
    if(sidebarPending)return;revealed=null;stableOrder.clear();stableGroups.clear();sidebarProjection=null;
    const id=api.id();sidebarPending={id,action,key,root};
    ui.send({a:'conversation-navigation',id,sidebar:{action,key,value,root}});
    ui.defer(()=>{if(sidebarPending?.id===id){sidebarPending=null;api.note('Sidebar save unknown; refresh before retrying.');draw();}},30000);draw();
   },
   receive:function(s){
    if(s.open)workerPaused=false;
    if(s.workerPanelClosed){api.workerClose();return true;}
    if(s.conversationSidebarReceipt){
     const r=s.conversationSidebarReceipt;if(r.id!==sidebarPending?.id)return true;
     const op=sidebarPending;sidebarPending=null;
     if(r.state==='accepted'&&r.sidebar){sidebar=r.sidebar;if(snapshot.conversations)snapshot.conversations.sidebar=sidebar;
      if(op.action==='archive')api.archived(op.key);
      if(op.action==='restore'){api.remember(op.key,op.root);api.show('recent');api.select(op.key,false);}
      api.note({pin:'Pin saved',collapse:'Group saved',archive:'Archived',restore:'Restored'}[op.action]+' · local view only');
     }else api.note(r.reason||'Sidebar save unknown; refresh before retrying.');return true;
    }
    if(s.dialogueReceipt){
     const r=s.dialogueReceipt;
     if(r.action==='task-body'){const found=[...taskReads].find(([,op])=>op.id===r.id);if(!found)return true;const [key,op]=found;ui.cancel(op.timer);const t=snapshot.projects?.find(p=>p.root===op.root)?.tasks?.find(t=>t.n===op.n);if(t&&(t.revision!==op.revision||t.body_source_revision!==op.source)){taskReads.delete(key);return true;}if(r.state==='accepted'&&r.value?.revision===op.revision&&r.value.n===op.n){const {card,question,verdict,conversation,answered,history,history_tail,history_count,history_lazy,history_source_revision,history_source_count}=r.value,row={card,question,verdict,conversation,answered,history,history_tail,history_count,history_lazy,history_source_revision,history_source_count};taskBodies.set(key,{row,bytes:new TextEncoder().encode(JSON.stringify(row)).length});taskReads.delete(key);api.taskTrim();}else op.reason=r.reason||'Task source changed. Refresh before reading or answering.';return true;}
     if(r.action==='history'){const state=[...historyViews.values()].find(v=>v.id===r.id);if(!state?.loading)return true;ui.cancel(state.timer);state.loading=false;if(r.state==='accepted'&&(state.previous?r.value?.next===state.cursor:r.value?.start===state.cursor)&&r.value.revision===state.revision){state.rows=r.value.rows;const bytes=new TextEncoder().encode(JSON.stringify(state.rows)).length;state.limitReason=bytes>1024*1024?'Selected recorded questions: '+(bytes/1048576).toFixed(1)+' MiB pinned while reading; inactive pages are evicted.':'';state.start=r.value.start;state.next=r.value.next;state.reason=r.value.complete===false?'Recorded questions are from a partial observation.':'';}else state.reason=r.reason||'Recorded questions changed; refresh their source.';let bytes=0,sources=0;for(const v of [...historyViews.values()].reverse())if(v.rows){bytes+=new TextEncoder().encode(JSON.stringify(v.rows)).length;sources++;if(v!==state&&(bytes>1024*1024||sources>1))delete v.rows;}return true;}
     if(r.action==='body'){const op=bodyReads.get(r.conversation),c=items.find(c=>c.id===r.conversation);if(!op||op.id!==r.id)return true;bodyReads.delete(r.conversation);ui.cancel(op.timer);if(!c||c.revision!==op.revision)return true;if(r.state==='accepted'&&r.value?.id===c.id&&r.value.revision===op.revision){Object.assign(c,r.value);api.restoreDrafts();bodyCache.delete(c.id);bodyCache.set(c.id,true);api.bodyTrim();}else {c.bodyReason=r.reason||'Saved draft unavailable; refresh to retry.';api.note(c.bodyReason,c.id);}return true;}
     if(r.action==='tasks'){
      const op=linkedReads[r.id];if(!op||r.conversation!==op.conversation)return true;delete linkedReads[r.id];if(linked?.id!==op.conversation)return true;
      if(!op.task)linked.busy=false;
      if(r.state==='accepted'){const rows=(r.value?.tasks||[]).filter(t=>t.conversation?.id===op.conversation);
       linked.rows=op.task?[...linked.rows.filter(t=>t.n!==op.task),...rows].slice(-ROWS):rows.slice(0,ROWS);linked.reason=op.task&&!rows.length?'Task #'+op.task+' has no matching originating-chat receipt; open the board or refresh.':'';if(!op.task){linked.page=op.page;linked.more=!!r.value.more;}
      }else linked.reason=r.reason||'Board read unavailable';return true;
     }
     if(r.id===activityRead?.id){sidebarProjection=null;const id=activityRead.conversation;activityRead=null;activityTimes[id]=r.state==='accepted'?r.value:{updated:null,reason:r.reason||'Activity unknown'};activityTimer=ui.defer(()=>{activityTimer=null;api.pollActivity();},100);return true;}
     if(r.action==='activity'||api.workerSettle(r))return true;
     api.settle(r);return true;
    }
    if(s.workspaceOwner){owner=s.workspaceOwner;if(owner.conversation||owner.select){selected=owner.conversation||owner.select;onSelect?.(selected);listOnly=false;focusDraft=true;}draw();return true;}
    if(s.conversationNavigationError!==undefined){if(s.conversationNavigationError)api.note(s.conversationNavigationError);return true;}
    if(s.conversationReceipt){
     const r=s.conversationReceipt;if(r.id!==pending?.id)return true;
     const op=pending;pending=null;if(Number.isSafeInteger(r.snapshot?.catalog_epoch))catalogEpoch=Math.max(catalogEpoch,r.snapshot.catalog_epoch);ui.cancel(timer);const c=items.find(c=>c.id===op.conversation),native=r.snapshot?.items?.find(c=>c.id===op.conversation);
     if(c&&native){c.reply=native.reply;c.revision=native.revision;c.request=native.request;c.sent=native.sent;c.dialogue=native.dialogue;c.updated=native.updated;if(['attach','remove'].includes(op.action))c.files=native.files;}
     if(r.state!=='accepted'&&['reply','watch','unwatch'].includes(op.action)&&!(op.action==='reply'&&r.state!=='rejected'&&(!native||native.reply)&&r.reason!=='working'&&!r.reason?.startsWith('open in '))){
      // D3: a refused turn or watch change is definite; its text shows and the draft stays.
      if(c)c.sending=false;api.note(op.action==='reply'?'Not sent: '+(r.reason||'delivery unknown')+'. Draft kept.':r.reason||'Watch change unknown',op.conversation);api.next();
     }else if(r.state!=='accepted'){
      failure={...op,reason:r.reason||'Native readback unknown; draft kept'};api.note(failure.reason,op.conversation);
      if(c)c.sending=false;
     }else {
      if(c&&op.action==='save')c.saved=op.version;
      if(c&&op.action==='send'){c.sending=false;if(c.text===c.sent?.text&&!c.next){c.text='';c.caret=[0,0];for(const f of c.sent.files)queue.push({conversation:c.id,action:'remove',file:f.id});c.files=c.files.filter(f=>!c.sent.files.some(sent=>sent.id===f.id));c.version=(c.version||0)+1;api.save(c);}api.note('Request saved. Native launch and replies are separate.',op.conversation);}
      if(op.action==='archive'&&c){scopeReturn=null;revealed=null;c.archived=true;undo=c.id;if(selected===c.id){const next=api.visible().find(row=>row.id!==c.id);selected=next?.id||null;selection[list]=selected;}api.note('Archived · '+api.title(c));api.nav();}
      if(op.action==='restore'&&c){c.archived=false;list='recent';search='';searchOpen=false;api.select(c.id);api.note('Restored',op.conversation);undo=null;}
      if(op.action==='delete'){items=items.filter(c=>c.id!==op.conversation);selected=api.visible()[0]?.id||null;api.note('Permanently deleted');confirmDelete=false;undo=null;api.nav();}
      if(op.action==='fork'&&r.value?.forked){if(c&&c.version===op.version)Object.assign(c,native);for(const row of r.snapshot.items)if(!items.some(c=>c.id===row.id))items.push({...row,version:0,saved:0});api.select(r.value.forked);api.note('Draft and files moved to a separate request. Nothing sent.');}
      if(op.action==='preview'&&r.value){
       if(r.value.file.mime.startsWith('image/'))ui.effect('conversations.previewImage',()=>new Promise((resolve,reject)=>{const image=new window.Image();ui.listen(image,'load',()=>resolve(r.value));ui.listen(image,'error',()=>reject(Error('Image preview unavailable; file and draft kept')));image.src='data:'+r.value.file.mime+';base64,'+r.value.data;}),value=>{if(value.file){preview=value;api.note('Local preview',op.conversation);}else api.note(value.reason,op.conversation);draw();});
       else {preview=r.value;api.note('Local preview',op.conversation);}
      }
      if(c&&['reply','reply-check'].includes(op.action)){c.replyAck=op.action==='reply'?op.id:op.op_id;c.reply=null;c.sending=false;if(c.version===op.typed&&(op.action==='reply'||r.value?.unchanged===true)){c.text='';c.caret=[0,0];for(const f of c.files)queue.push({conversation:c.id,action:'remove',file:f.id});c.files=[];}c.version=(c.version||0)+1;api.save(c);api.note('Message saved · check PM activity separately.',op.conversation);if(view?.id===c.id)api.read('status');}
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
    sidebarEpoch++;sidebarProjection=null;
    let incoming=s.conversations;if(Number.isSafeInteger(incoming?.catalog_epoch)){if(incoming.catalog_epoch<catalogEpoch){incoming={...incoming,items:[],complete:false,loading:false,reason:'Superseded conversation catalog; waiting for the current source'};s.conversations=incoming;}else catalogEpoch=incoming.catalog_epoch;}
    if(incoming?.sidebar&&!sidebarPending)sidebar=incoming.sidebar;
    if(incoming){
     const existing=new Map(items.map(c=>[c.id,c]));
     for(const n of incoming.items||[]){const c=existing.get(n.id);if(c&&n.revision<c.revision)continue;if(c){c.request=n.request;c.dialogue=n.dialogue;c.reply=n.reply;if(!pending&&!queue.some(o=>o.conversation===c.id)&&(c.saved??0)===(c.version??0)){if(n.lazy&&!c.lazy&&n.revision===c.revision){const {text,files,sent,lazy,...meta}=n;Object.assign(c,meta);if(sent)c.sent={...c.sent,...sent};}else Object.assign(c,n);}}else items.push({...n,version:0,saved:n.revision===0?-1:0});}
     const present=new Set((incoming.items||[]).map(n=>n.id));
     if(incoming.complete!==true){let kept=0,bytes=0;items=items.filter(c=>{if(present.has(c.id)||c.id===selected||c.sending||c.reply||pending?.conversation===c.id||queue.some(o=>o.conversation===c.id)||(c.saved??0)!==(c.version??0))return true;const {text,files,sent,...meta}=c;bytes+=new TextEncoder().encode(JSON.stringify(meta)).length;return ++kept<=ROWS&&bytes<=(s.index_spare??16*1024*1024)/2;});}
     if(incoming.complete&&incoming.absence_complete!==false)for(const c of items){if(c.revision>0&&!present.has(c.id)){c.deleted=true;if(c.text===c.sent?.text)c.text='';c.sent=null;c.request=null;c.files=[];queue=queue.filter(o=>o.conversation!==c.id);if(pending?.conversation===c.id){pending=null;ui.cancel(timer);}if(failure?.conversation===c.id)failure=null;}}
    }
    api.restoreDrafts();
    if(!hydrated&&!incoming?.loading){hydrated=true;const nav=navigated?null:incoming?.navigation;if(nav){list=nav.list==='archive'?'archive':'recent';search=nav.search||'';resultFilter=['all','new','needs','reply','decision','activity','finished'].includes(nav.filter)?nav.filter:'all';selection=nav.selection||{};scrolls=nav.scrolls||{};selected=nav.selected;listOnly=nav.listOnly===true;}if(nav?.threads&&typeof nav.threads==='object'){
      const t=nav.threads;focuses=Object.fromEntries(Object.entries(t.focuses||{}).filter(([k,v])=>k.length<=1024&&v&&/^(BUTTON|A|SUMMARY|INPUT|SELECT|TEXTAREA)$/.test(v.tag)&&typeof v.key==='string'&&v.key.length<=2048&&Number.isInteger(v.index)&&v.index>=0&&v.index<4096));completed=t.completed?views.criteria(t.completed):null;scopeReturn=t.scopeReturn&&['recent','archive'].includes(t.scopeReturn.list)&&['all','new','needs','reply','decision','activity','finished'].includes(t.scopeReturn.filter)&&typeof t.scopeReturn.search==='string'&&t.scopeReturn.search.length<=512?{...t.scopeReturn,completed:t.scopeReturn.completed?views.criteria(t.scopeReturn.completed):null}:null;unified=true;selected=typeof t.selected==='string'?t.selected:null;
      selection=Object.fromEntries(['recent','archive'].map(k=>[k,typeof t.selection?.[k]==='string'?t.selection[k]:null]));
      opened=Array.isArray(t.opened)?t.opened.filter(r=>r&&typeof r.id==='string'&&/^task:https?:\/\/.+\/[1-9][0-9]*$/.test(r.id)&&(r.root==null||typeof r.root==='string')).map(r=>({id:r.id,root:r.root||null})):[];
      positions=t.positions&&typeof t.positions==='object'?Object.fromEntries(Object.entries(t.positions).filter(([k,v])=>Number.isFinite(v)&&v>=0||api.anchored(v))):{};api.place();
     }else if(!navigated&&!selected)selected=api.visible()[0]?.id||null;}

    return false;
   },
   problem:function(c){
    if(c?.recoveryReason)return c.recoveryReason;
    if(c?.lazy)return 'Reading saved draft; editing waits for its revision-bound source.';
    if(!c)return 'Choose New to write a request.';
    if(c.deleting)return 'Deletion incomplete; retry Delete. History cannot be restored.';
    if(!api.editable(c))return c.deleted?'History permanently deleted; this draft cannot be sent.':'This item is editable in its detached window.';
    if(failure)return 'Native save/delivery unknown. Keep the draft and resolve the receipt.';
    if(snapshot.conversations?.complete!==true)return snapshot.conversations?.reason||'Native draft storage unavailable; refresh before sending.';
    if(api.launched(c)){
     if(c.reply)return 'Reply delivery needs readback. Draft and files kept.';
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
    if(!api.runtime(c).available)return api.runtime(c).reason||'Owner runtime unavailable; no fallback.';
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
   // CS1–CS6: the three source types share one identity, project assignment and activity order.
   matches:function(r,mode=resultFilter){const view=mode==='all'||mode==='new'&&r.unread||mode==='needs'&&(r.marker==='Needs reply'||r.needsReply||r.needsDecision)||mode==='reply'&&(r.marker==='Needs reply'||r.needsReply)||mode==='decision'&&r.needsDecision||mode==='activity'&&r.activityThread||mode==='finished'&&r.finished;if(!view||!completed)return view;const p=snapshot.projects?.find(p=>p.root===r.root),days={'7d':7,'30d':30}[completed.period];return (!completed.projects.length||completed.projects.includes(views.identity(p||{})))&&views.matches({...r,n:r.task},completed.query)&&(!days||r.closed>=(snapshot.now||Date.now()/1000)-days*86400)&&(!completed.outcomes.length||completed.outcomes.includes(['success','not_planned'].includes(r.outcome)?r.outcome:'unknown'));},
   nextUnresolved:function(){const rows=api.order(),at=rows.findIndex(r=>r.id===selected);return [...rows.slice(at+1),...rows.slice(0,Math.max(0,at))].find(r=>!r.archived&&(r.needsDecision||r.marker==='Needs reply'||r.needsReply));},
   sidebarGroups:function(extra){
    const projects=snapshot.projects||[],cacheKey=JSON.stringify([sidebarEpoch,selected,list,search,resultFilter,completed,sidebar]);
    let groups;
    if(sidebarProjection?.key===cacheKey&&sidebarProjection.rows===extra.rows)groups=sidebarProjection.groups;
    else {
     if(sidebarProjection?.selected===selected&&sidebarProjection.rows!==extra.rows)api.listMark();
     const byRoot=new Map(projects.map(p=>[p.root,p])),owners=new Map(items.map(c=>[c.id,c]));
     const source=[...(extra.rows||[]).map(r=>({...r,thread:true})),...items.filter(c=>!c.deleted).map(c=>({id:c.id,root:c.sent?.root||c.request?.root||c.root,title:api.title(c),archived:c.archived,updated:c.updated,...extra.outcomes?.[c.id],meta:api.rowStatus(c)+((owner.detached||[]).includes(c.id)?' · Detached':''),unread:!!(extra.marks?.[c.id]||extra.outcomes?.[c.id]?.unread),marker:extra.outcomes?.[c.id]?.needsReply?'Needs reply':extra.outcomes?.[c.id]?.needsDecision?'Needs decision':extra.marks?.[c.id]}))];
     const query=r=>!search||[r.title,r.meta,r.root,r.id,byRoot.get(r.root)?.name,r.project].join(' ').toLowerCase().includes(search.toLowerCase());
     for(const [mode]of MODES)modeCounts[mode]=source.filter(r=>!r.linkedAlias&&!!r.archived===(list==='archive')&&api.matches(r,mode)&&query(r)).length;
     const candidate=source.find(r=>r.id===selected),outside=candidate&&(candidate.linkedAlias||!api.matches(candidate)||!!candidate.archived!==(list==='archive')||!query(candidate));if(outside&&!scopeReturn)scopeReturn={...api.context(),selected:null};
     const buckets=new Map(),add=(id,name,r)=>{if(!buckets.has(id))buckets.set(id,{id,name,total:0,at:-1,rows:[]});const g=buckets.get(id);g.total++;g.at=Math.max(g.at,r.activityMs??-1);if(query(r)||r.id===selected)g.rows.push(r);};
     for(const raw of source){const external=outside&&scopeReturn&&raw.id===selected;if(!external&&(!!raw.archived!==(list==='archive')||!api.matches(raw)))continue;
      const p=byRoot.get(raw.root),pin=!external&&sidebar.pins.includes(raw.id),activity=Object.hasOwn(activityTimes,raw.id)?activityTimes[raw.id].updated:raw.updated,group=external?'linked':list==='archive'?'archive':pin?'pinned':p?JSON.stringify([p.root,p.issues||'']):'other';
      add(group,external?'Linked source':group==='archive'?'Archived':group==='pinned'?'Pinned':p?.name||'Other',{...raw,p,group,pin,activity,activityMs:time.value(activity),own:owners.has(raw.id)});
     }
     const compare=(a,b)=>(b.activityMs??-1)-(a.activityMs??-1)||(a.finished&&b.finished?(b.task||0)-(a.task||0):0)||a.id.localeCompare(b.id);
     groups=[...buckets.values()].filter(g=>g.rows.length).sort((a,b)=>{const rank=id=>id==='linked'?-3:id==='pinned'?-2:id==='other'?1:0;return rank(a.id)-rank(b.id)||b.at-a.at||a.id.localeCompare(b.id);});
     if(selected&&stableGroups.size)groups.sort((a,b)=>(stableGroups.get(a.id)??Infinity)-(stableGroups.get(b.id)??Infinity));
     for(const g of groups){g.rows.sort(compare);if(selected&&stableOrder.size)g.rows.sort((a,b)=>(stableOrder.get(a.id)??Infinity)-(stableOrder.get(b.id)??Infinity));}
     sidebarItems=groups.flatMap(g=>g.rows);stableOrder=new Map(sidebarItems.map((r,i)=>[r.id,i]));stableGroups=new Map(groups.map((g,i)=>[g.id,i]));sidebarProjection={key:cacheKey,rows:extra.rows,selected,groups};
    }
    const key=JSON.stringify([resultFilter,completed,search,list]);let page=resultPages.get(key)||{start:0,size:PAGE};
    if(page.start>=sidebarItems.length){page={...page,start:Math.max(0,Math.floor((sidebarItems.length-1)/PAGE)*PAGE)};}
    const at=sidebarItems.findIndex(r=>r.id===selected);if(selected!==pageSelected&&at>=0&&(at<page.start||at>=page.start+page.size))page={start:Math.floor(at/PAGE)*PAGE,size:PAGE};
    pageSelected=selected;resultPages.delete(key);resultPages.set(key,page);while(resultPages.size>SAVED)resultPages.delete(resultPages.keys().next().value);
    const mounted=new Set(sidebarItems.slice(page.start,page.start+page.size).map(r=>r.id));
    const before=page.start>0?h('button',{'data-c':'list-page','data-start':Math.max(0,page.start-PAGE)},'Previous threads'):null,after=page.start+page.size<sidebarItems.length?h('button',{'data-c':'results-more','data-page':key},'More threads'):null;
    return [before,...groups.filter(g=>g.rows.some(r=>mounted.has(r.id))).map(g=>{
     const shown=g.rows.filter(r=>mounted.has(r.id)),series=resultFilter==='new'?news.groups(shown):null;
     const project=g.id.startsWith('['),open=!project||!!search||resultFilter!=='all'||!sidebar.collapsed.includes(g.id)||g.rows.some(r=>r.id===revealed);
     const render=r=>{
       const own=items.find(c=>c.id===r.id),context=!r.p?(r.root||r.board||r.project?(r.project?r.project+' · ':'')+'Project unavailable':'No project'):(g.id==='pinned'||list==='archive'?r.p.name:''),meta=r.report&&r.needsDecision?'':r.thread?(r.status??r.meta??''):r.meta;
       const blocked=!!sidebarPending||!!pending||!!failure||!!extra.busy;
       return h('div',{key:r.id,class:'conversation-item'},h('button',{class:'conversation-row',...(r.report?{'data-report':'open','data-root':r.root,'data-n':r.report,'data-thread':r.id}:r.thread?{'data-a':'thread','data-thread':r.id}:{'data-c':'select','data-id':r.id}),title:r.title,'aria-current':r.id===selected?'page':null},h('span',null,r.title),h('small',null,[context,meta].filter(Boolean).join(' · '),(context||meta)?' · ':null,r.timeContext==='Request created'?'Created · ':null,time.stamp(r.activity,r.timeContext||'Last activity',activityTimes[r.id]?.reason||r.updated_reason||'The source did not supply an activity time',false),r.marker&&h('b',{class:'thread-marker'},' · '+r.marker))),
        h('div',{class:'conversation-row-actions'},icons.button(r.pin?'pin-off':'pin',r.pin?'Unpin':'Pin',{'data-c':'row-pin','data-id':r.id,class:'compact-action',disabled:blocked}),
         icons.button(r.archived?'archive-restore':'archive',r.archived?'Restore':'Archive',{'data-c':'row-archive','data-id':r.id,class:'compact-action',disabled:blocked,onClick:()=>{if(blocked)return;if(own)api.action(r.archived?'restore':'archive',{},r.id);else extra.archive?.(r);}})));
};
     return h('section',{key:g.id,'data-conversation-group':g.id,'aria-label':g.name},
      g.id!=='linked'&&h(project?'button':'div',{class:'conversation-group-head',...(project?{'data-c':'group-collapse','data-group':g.id,'aria-expanded':String(open),disabled:!!sidebarPending}:{})},project&&icons.icon('arrow-down'),h('span',null,g.name),h('small',null,search?g.rows.length+'/'+g.total:String(g.total))),
      open&&(series?series.map(s=>s.rows.length===1?render(s.rows[0]):h('details',{key:s.rows[0].id,class:'notification-series','data-notification-group':'','data-view-key':'notification-series:'+s.rows[0].id},h('summary',null,s.title+' · '+s.rows.length+' threads · '+s.rows.filter(r=>r.unread).length+' new'),s.rows.map(render))):shown.map(render)));
    }),after];
   },
   // #1699 A1/A4: one thread list: owner conversations plus the task and event threads the workspace passes in
   listMark:function(){const el=menu.querySelector('.conversation-rows'),top=el?.getBoundingClientRect().top,node=el&&[...el.querySelectorAll('.conversation-row')].find(n=>n.getBoundingClientRect().bottom>top);listAnchor=node?{id:node.dataset.thread||node.dataset.id,y:node.getBoundingClientRect().top-top}:null;},
   listEdge:function(){listObserver?.disconnect();const root=menu.querySelector('.conversation-rows'),edge=root?.querySelector('[data-c=results-more]');if(!edge||!window.IntersectionObserver)return;if(root!==listObservedRoot){listObservedRoot=root;listObserver=null;}listObserver??=new window.IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting)){const button=menu.querySelector('.conversation-rows [data-c=results-more]');if(button)api.click(button);}},{root,rootMargin:'0px 0px '+root.clientHeight+'px 0px'});listObserver.observe(edge);},
   body:function(extra={}){
    api.bodyTrim();const thread=extra.selected,c=thread?null:api.current(),disabled=!api.editable(c)||c.deleting||c.archived,bad=api.problem(c),button=icons.button,dialogue=api.launched(c);
    if(c?.deleted)return h('section',{class:'conversations'},h('p',{role:'status'},'History permanently deleted. Sent content cannot be restored. Independent saved drafts remain in Recent.'),c.text&&h('label',null,'Unsent local text',h('textarea',{readOnly:true,value:c.text})),owner.conversation?h('button',{'data-c':'return'},'Return to main'):h('button',{'data-c':'new'},'New request'));
    const rows=api.sidebarGroups(extra),matching=modeCounts[resultFilter]||0,incomplete=snapshot.conversations?.complete!==true||snapshot.messages?.complete!==true||!!extra.resultReason,count=n=>incomplete?(n?'≥'+n:'?'):String(n),detail=typeof extra.detail==='function'?extra.detail():extra.detail;
    // A6: the session runtime is the one the request recorded at launch, not the current owner default
    const runtime=c?.request?.runtime||c?.sent?.runtime;
    // #1773 A3: the composer bottom row holds the attach, new-from-draft and history icons and the send arrow
    const tools=c&&!c.archived?h('div',{class:'conversation-tools composer-bottom'},!disabled&&h('label',{class:'attach-label compact-action',title:'Attach files'},icons.icon('paperclip'),h('span',{class:'sr-only'},'Attach'),h('input',{type:'file',multiple:true,accept:'.txt,.md,.png,.jpg,.jpeg,.webp','data-c-field':'files','aria-label':'Attach files'})),
     (c.sent||c.request)&&button('file-plus-2','New from draft',{'data-c':'from-draft',class:'compact-action',disabled:disabled||!!owner.conversation||!(c.text||'').trim()}),
     h('details',{'data-view-key':'conversation-history',class:'command-history-menu'},h('summary',{class:'compact-action',title:'History','aria-label':'History'},icons.icon('history')),h('div',{class:'conversation-history'},(snapshot.ai?.history||[]).map((text,i)=>h('button',{'data-c':'history','data-index':i,disabled},text)),h('button',{'data-c':'clear-history'},'Clear command history'),h('p',{class:'meta'},'History fills only. Permanent deletion is available in Archive.'))),
     c.reply&&h('button',{'data-c':'reply-check',disabled:!!pending},'Check delivery'),h('button',{'data-c':dialogue?'reply':'send',class:'primary-action compact-action',title:'Send','aria-label':'Send',disabled:!!bad},icons.icon('arrow-up')),
     // #1775: file limits sit behind one info icon beside Send (conversations-input-scroll 06)
     h('details',{class:'composer-help','data-view-key':'composer-help'},h('summary',{class:'compact-action',title:'File limits and message help','aria-label':'File limits and message help'},icons.icon('info')),h('p',{class:'meta'},'UTF-8 text/Markdown, PNG/JPEG/WebP. 4 files, 5 MiB each, 20 MiB per draft, 100 MiB owned storage. PDF unavailable (0 pages). Sources stay untouched.'))):null;
    const win=owner.conversation?button('panel-left-close','Return to main',{'data-c':'return',class:'compact-action'}):(owner.detached||[]).includes(c?.id)?button('panels-top-left','Raise window',{'data-c':'raise',class:'compact-action',disabled:!!pending}):button('panels-top-left','Detach',{'data-c':'detach',class:'compact-action',disabled:!!pending});
    return h('section',{class:'conversations'+(listOnly||!thread&&!c?' list-only':'')+(owner.conversation?' detached':''),'aria-label':'Conversations'},
     h('div',{class:'conversation-toolbar chat-entry-bar'},button(listOnly?'arrow-left':'rows-3',listOnly?'Back':'List',{'data-c':'list',class:'conversation-list-toggle compact-action'}),h('button',{'data-c':'new',class:'new-chat-action',disabled:!!owner.conversation},icons.icon('plus'),'New chat'),button('search','Search threads',{'data-c':'search-open',class:'compact-action'}),button('terminal','Work sessions',{'data-route':'sessions',class:'compact-action'})),
     h('div',{class:'conversation-grid'},
      h('aside',{class:'conversation-list','aria-label':'Threads'},h('div',{class:'conversation-tools list-tabs'},
       h('details',{class:'conversation-view','data-view-key':'conversation-view'},h('summary',{title:(MODES.find(m=>m[0]===resultFilter)?.[1]||'All')+' · '+(incomplete?'Total unknown; '+matching+' loaded':'Total '+matching)+' matching threads in this scope; counts independent of resident pages'},icons.icon(MODES.find(m=>m[0]===resultFilter)?.[2]||'messages-square'),h('span',{class:'view-label'},MODES.find(m=>m[0]===resultFilter)?.[1]),h('span',{class:'view-count'},' · '+count(matching))),h('div',{class:'conversation-tools result-filters'},MODES.map(([value,label,glyph])=>h('button',{'data-c':'result-filter','data-filter':value,'aria-pressed':resultFilter===value,'aria-label':label+': '+(incomplete?'total unknown; '+modeCounts[value]+' loaded':modeCounts[value])},icons.icon(glyph),h('span',null,label),h('span',{class:'view-count'},count(modeCounts[value])))))),
       h('span',{class:'conversation-tools list-location'},button('inbox','Recent',{'data-c':'recent','aria-pressed':list==='recent',class:'compact-action'}),button('archive','Archive',{'data-c':'archive-list','aria-pressed':list==='archive',class:'compact-action'}))),
       (searchOpen||search)&&h('div',{class:'conversation-search'},h('label',null,icons.icon('search'),h('span',{class:'sr-only'},'Search threads'),h('input',{type:'search','data-c-field':'search',value:search,placeholder:'Search',maxLength:512})),button('x','Clear search',{'data-c':'clear-search',class:'compact-action'})),
       scopeReturn&&h('div',{class:'meta conversation-scope'},h('span',null,'Linked'),h('button',{'data-c':'scope-return',title:'Linked source outside this view; return to saved scope and selection'},icons.icon('arrow-left'),'Return to view')),
       completed&&h('details',{'data-view-key':'completed-scope',class:'completed-scope'},h('summary',null,'Finished · '+(completed.projects.length?views.scope(snapshot.projects||[],completed).map(p=>p.name).join(', ')||'Projects unavailable':'All projects')+' · '+completed.period+' · '+(completed.outcomes.join(', ')||'All outcomes')+(completed.query?' · '+completed.query:'')),h('p',{class:'meta'},'Projects: '+(completed.projects.length?views.scope(snapshot.projects||[],completed).map(p=>p.name).join(', ')||'Selected projects unavailable':'All')+' · '+completed.period+' · '+(completed.query||'No query')+' · '+(completed.outcomes.join(', ')||'All outcomes')),h('button',{'data-c':'scope-clear'},'All finished work')),
       extra.newsTools,
       h('div',{class:'conversation-rows'},search&&sidebarItems.length>0&&sidebarItems.every(r=>r.group==='linked')&&h('p',null,'No matches in this view'),rows.length?rows:h('p',null,search?'No matches':resultFilter!=='all'?'No matching items in this view':'No '+(list==='archive'?'archived threads':'threads yet'))),extra.historyTools,snapshot.conversations?.complete!==true&&h('p',{role:'status',class:'meta'},snapshot.conversations?.reason||'Conversation coverage unknown'),extra.resultReason&&h('p',{class:'meta',role:'status'},extra.resultReason)),
      thread?h('section',{class:'conversation-detail thread-detail','data-thread-id':detail?.readingKey||thread,'aria-label':'Selected thread'},detail?.header,h('div',{class:'conversation-reader'},detail?.reading),detail?.tools&&h('div',{class:'conversation-send'},extra.detail.tools)):
      h('section',{class:'conversation-detail','data-thread-id':c?.id||'','aria-label':'Selected request'},c?[h('div',{class:'conversation-tools thread-head'},h('h2',{class:'thread-title owner-title',title:api.title(c)},api.title(c)),h('span',{class:'options'},win,button(c.archived?'archive-restore':'archive',c.archived?'Restore':'Archive',{'data-c':c.archived?'restore':'archive',class:'compact-action',disabled:!api.editable(c)||!!pending}))),
       dialogue&&h('p',{class:'dialogue-activity'},h('span',{class:'meta dialogue-state','data-state':api.state(c).split(' ')[0],'aria-label':'Session state: '+api.state(c)},api.activity(c))),
       h('div',{class:'conversation-reader'},
       // UX07: one disclosure owns secondary controls; the transcript and composer stay clear by default.
       h('details',{class:'conversation-info dialogue-launch','data-view-key':'conversation-info:'+c.id},h('summary',null,icons.icon('info'),'Conversation info'),
        h('label',null,'Request name',h('input',{'data-c-field':'title','aria-label':'Request name',placeholder:api.title(c),value:c.title,disabled,maxLength:120})),
        h('div',{class:'conversation-tools'},c.archived?h('button',{'data-c':'delete-review',disabled:!api.editable(c)||!!pending},c.deleting?'Retry Delete':'Delete permanently'):null),
        h('label',null,'Project',h('select',{'data-c-field':'root',value:c.root,disabled},h('option',{value:''},'Choose a project'),!snapshot.projects?.some(p=>p.root===c.root)&&c.root&&h('option',{value:c.root},'Unavailable project'),(snapshot.projects||[]).map(p=>h('option',{value:p.root},p.name)))),
        !c.sent&&!c.request&&h('label',null,'Runtime',h('select',{'data-c-field':'runtime','aria-label':'Runtime',value:api.runtime(c).id,disabled},(snapshot.ai?.runtime?.choices||[snapshot.ai?.runtime]).filter(Boolean).map(r=>h('option',{value:r.id},r.name||r.id)))),
        dialogue&&h('p',{class:'meta'},'Session '+(runtime||'runtime not recorded')+' · '+(snapshot.host||'Host unknown')),
        h('p',{class:'meta'},'Request '+c.id),pm.handoff(c.request,true),c.request&&h('p',{class:'meta'},'Request created',time.stamp(c.request.created,'Request created')),
        dialogue&&!c.archived&&h('div',{class:'dialogue-bar'},api.status(c,disabled)),
        dialogue&&h('p',{class:'meta'},'Send replies to this session. New from draft starts a separate request.'),
        disabled&&h('p',{class:'meta'},'Attachments cannot be added while this record is read-only.')),
       c.archived&&h('div',null,h('p',{class:'meta'},'Archived in TaskQ only; the native conversation stays. Restore before reply.'),confirmDelete&&h('div',{role:'alert'},h('p',null,'Permanently remove this TaskQ record and its owned files? No undo. Source files, native chats, sessions and board tasks stay.'),h('button',{'data-c':'delete'},'Delete permanently now'),h('button',{'data-c':'delete-cancel'},'Keep record'))),
       api.preview(),dialogue?api.transcript(c):h('div',{class:'conversation-record',tabindex:'0','aria-label':'Request record'},c.sent&&!c.request&&h('p',{role:'status'},'Delivery unknown. Check the original request ID; nothing will be resent.'),c.request&&c.request.state!=='launched'&&h('p',{role:'status'},c.request.reason||c.request.next||'Native launch: '+c.request.state),c.lazy&&h('p',{role:'status'},c.bodyReason||'Reading saved draft…'),c.bodyReason&&h('button',{'data-c':'body-retry'},'Retry draft read'),c.sent&&h('article',null,h('h2',null,'Sent request',time.stamp(c.request?.created,'Request created')),h('p',{class:'meta'},(snapshot.projects?.find(p=>p.root===c.sent.root)?.name||'Original project unavailable')+' · '+c.sent.runtime),h('p',{class:'conversation-text'},c.sent.text),h('ul',null,(c.sent.files||[]).map(f=>h('li',{key:f.id},f.name+' · '+f.bytes+' bytes · SHA256 '+f.hash)))),c.request?.state==='pending'&&h('button',{'data-c':'cancel',disabled},'Cancel queued launch'),c.sent&&h('button',{'data-c':'reconcile',disabled:disabled||!!pending},'Check delivery')),
       extra.notice,extra.cards,
       !c.archived&&(extra.editor||pm.composer({text:c.text||'',disabled,id:'conversation-draft',placeholder:dialogue?'Reply to the '+(runtime||'native')+' session':c.sent?'Draft a separate next request':'Ask AI…',before:[dialogue&&view?.id===c.id&&api.jump(),h('p',{class:'meta composer-destination',title:dialogue?'Send replies to the recorded '+(runtime||'native')+' session. Change the separate request destination in Details.':'Change the project in Conversation info.'},icons.icon('folder'),(snapshot.projects?.find(p=>p.root===(dialogue?c.sent?.root||c.request?.root||c.root:c.root))?.name||'Project unavailable')+' · '+(dialogue?runtime||'Runtime unknown':api.runtime(c).name||'Runtime unknown')+' · '+(snapshot.host||'Host unknown')),
        // #1773 A3/A5: files sit above the input and the tools row closes the input box; the shared DashQ composer stays unchanged
               h('div',{class:'conversation-files'},(c.files||[]).map(f=>h('div',{class:'conversation-file',key:f.id},h('button',{'data-c':'preview','data-file':f.id,disabled},f.name+' · '+f.bytes+' B'),button('x','Remove '+f.name,{'data-c':'remove','data-file':f.id,disabled}))))],
        tools,reason:bad==='Write a message for the session.'?'':bad})))
      ]:h('p',null,'Choose a thread or New. Launched requests continue here as native turns.'))),
     h('footer',{class:'conversation-status',role:'status','aria-live':'polite'},(recoveryDrafts.length?'Recovered draft source unavailable. Local edits kept; refresh status to load it. ':'')+(notices[selected||'list']||''),bodyNotice&&h('span',{class:'meta'},bodyNotice),pending?.conversation===selected&&' · Saving…',undo&&h('button',{'data-c':'undo'},'Undo archive'),failure&&failure.conversation!==selected&&h('span',null,' · Saving blocked by ',h('button',{'data-c':'reply-open','data-id':failure.conversation},api.title(items.find(c=>c.id===failure.conversation)||{title:failure.conversation})),'. Open it to resolve the receipt.'),failure?.conversation===selected&&h('button',{'data-c':'retry-save'},failure.recovered?({save:'Save recovered draft',attach:'Save recovered attachment',remove:'Remove recovered attachment'}[failure.action]||'Retry recovered action'):failure.action==='send'||failure.action==='reconcile'?'Check original delivery':['reply','watch','unwatch'].includes(failure.action)?'Check status':'Retry native save'),extra.footer));
   },
   click:function(b){
    const action=b.dataset.c;if(!action)return false;const c=api.current();sidebarProjection=null;
    if(action==='body-retry'&&c){delete c.bodyReason;api.bodyRead(c);draw();}
    else if(action==='list-page'){const key=JSON.stringify([resultFilter,completed,search,list]);resultPages.delete(key);resultPages.set(key,{start:Number(b.dataset.start),size:PAGE});draw();}
    else if(action==='closed-open'||action==='closed-earlier'){const r=closedViews.get(b.dataset.root+'#'+b.dataset.task+'@'+b.dataset.sha);api.closedRead(b.dataset.root,Number(b.dataset.task),b.dataset.sha,action==='closed-earlier'?r?.page?.start:undefined,false,action==='closed-open'&&!!(r?.reason||r?.page));}
    else if(action==='task-body-retry'){for(const [key,op]of taskReads)if(op.root===b.dataset.root&&op.n===Number(b.dataset.task)){ui.cancel(op.timer);taskReads.delete(key);}draw();}
    else if(action==='linked-select'){linked.current=Number(b.dataset.task);draw();}
    else if(action==='linked-page'){linked.start=Number(b.dataset.start);linked.current=null;draw();}
    else if(action==='linked-refresh'){linked.checked.clear();api.linkedRead(selected);}
    else if(action==='linked-older')api.linkedRead(selected,undefined,linked.page+1);
    else if(action==='linked-newer')api.linkedRead(selected,undefined,Math.max(1,linked.page-1));
    else if(action==='worker-earlier'&&live){live.quiet.before=0;api.workerRead('before');}
    else if(action==='worker-later'&&live){live.quiet.after=0;api.workerRead('after',{after:live.end});}
    else if(action==='worker-latest'&&live){const reset=!!live.lost;live.stick=true;live.anchor=null;api.place(live.thread);if(reset||live.end<live.size){live.gen++;live.items=[];live.start=live.end=null;live.request=null;live.busy={};if(reset){live.lost=false;live.reason='';live.source=null;live.from=undefined;}api.workerRead('initial');}draw();}
    else if(action==='row-pin'){api.sidebar('pin',b.dataset.id,!sidebar.pins.includes(b.dataset.id));}
    else if(action==='result-filter'){api.filter(b.dataset.filter);menu.querySelector('.conversation-view')?.removeAttribute('open');draw();}
    else if(action==='scope-return')api.returnScope();
    else if(action==='scope-clear'){completed=null;api.nav();draw();}
    else if(action==='results-more'){api.listMark();const key=JSON.stringify([resultFilter,completed,search,list]),p=resultPages.get(key)||{start:0,size:PAGE};resultPages.set(key,{start:p.size<ROWS?p.start:p.start+PAGE,size:Math.min(ROWS,p.size+PAGE)});draw();}
    else if(action==='group-collapse')api.sidebar('collapse',b.dataset.group,!sidebar.collapsed.includes(b.dataset.group));
    else if(action==='row-archive'){return true;}
    else if(action==='reply-check'&&c?.reply){if(failure?.conversation===c.id)failure=null;api.action('reply-check',{op_id:c.reply.id,typed:c.version||0});}
    else if(action==='new'&&!owner.conversation)api.fresh();
    else if(action==='select'){if((owner.detached||[]).includes(b.dataset.id))ui.send({a:'conversation-window',action:'raise',conversation:b.dataset.id});else api.select(b.dataset.id);}
    else if(action==='search-open'){listOnly=true;searchOpen=true;focusSearch=true;api.nav();draw();}
    else if(action==='list'){listOnly=!listOnly;api.nav();draw();}
    else if(action==='recent'||action==='archive-list'){api.flush();scopeReturn=null;revealed=null;api.show(action==='recent'?'recent':'archive');listOnly=true;api.nav();draw();}
    else if(action==='clear-search'){search='';searchOpen=false;api.nav();draw();}
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
    else if(action==='earlier'&&view&&!view.lost&&!view.busy.before&&view.start>0){view.quiet.before=0;api.read('before',{before:view.start});draw();}
    else if(action==='later'&&view&&!view.lost&&!view.busy.after&&view.end<view.size){view.quiet.after=0;api.read('after',{after:view.end});draw();}
    else if(action==='latest'&&view){api.follow();draw();}
    else if(action==='retry-save'){
     const failed=failure;failure=null;
     // a reply with no receipt is never resent: the transcript and status tell whether it ran
     if(failed&&['reply','watch','unwatch'].includes(failed.action)){const f=items.find(c=>c.id===failed.conversation);if(f)f.sending=false;api.note('Delivery unknown; draft kept. Check the transcript and status before sending again.');if(view?.id===failed.conversation)api.read('status');api.next();draw();}
     else if(failed){if(['send','reconcile'].includes(failed.action)){queue=queue.filter(o=>o.action!=='send');queue.unshift({conversation:failed.conversation,action:'reconcile'});}else queue.unshift({...failed,id:undefined});api.next();draw();}
    }
    else if(['archive','restore','send','cancel','reconcile','preview','remove','delete'].includes(action))api.action(action,{...(b.dataset.file?{file:b.dataset.file}:{}),...(action==='send'?{runtime:api.runtime(c).id}:{})});
    return true;
   },
   mount:function(){
    time.mount(menu);
    ui.listen(menu,'focusout',e=>{if(e.target.dataset.cField==='search'&&!search)ui.defer(()=>{if(!menu.querySelector('[data-c-field=search]')?.matches(':focus')){searchOpen=false;draw();}},0);});
    ui.listen(menu,'focusin',e=>{if(e.target.closest('.conversation-reader'))api.scrollIntent('focus');const pane=e.target.closest('.conversation-detail'),key=pane?.dataset.threadId;if(!key||!e.target.matches('button,a,summary,input,textarea,select'))return;const target=e.target,data=JSON.stringify(target.dataset);delete focuses[key];focuses[key]={tag:target.tagName,key:data,index:[...pane.querySelectorAll(target.tagName)].filter(n=>JSON.stringify(n.dataset)===data).indexOf(target)};api.nav();});
    // WebKit accessibility selection can change the range without a click or keyup in the field.
    ui.listen(document,'selectionchange',()=>{const field=document.activeElement;if(field?.dataset?.cField!=='text')return;const c=api.current(),caret=[field.selectionStart,field.selectionEnd];if(api.editable(c)&&field.value===c.text&&JSON.stringify(c.caret)!==JSON.stringify(caret)){c.caret=caret;c.version=(c.version||0)+1;api.save(c);}});
    ui.listen(menu,'input',e=>{const watch=e.target.dataset.wField;if(view?.form&&(watch==='prompt'||watch==='every')){view.form[watch]=e.target.value;view.form.dirty=true;draw();return;}const field=e.target.dataset.cField;if(field==='search'){navigated=true;search=e.target.value;sidebarProjection=null;stableOrder.clear();stableGroups.clear();api.nav();draw();return;}if(!['text','title'].includes(field))return;const c=api.current();if(!api.editable(c))return;if(field==='text'&&(c.sent||c.sending))c.next=true;c[field]=e.target.value;sidebarProjection=null;if(field==='text')c.caret=[e.target.selectionStart,e.target.selectionEnd];c.version=(c.version||0)+1;ui.cancel(saveTimer);api.save(c);draw();});
    ui.listen(menu,'change',e=>{if(e.target.dataset.wField==='event'&&view?.form){view.form.events=view.form.events.filter(k=>k!==e.target.value).concat(e.target.checked?[e.target.value]:[]);view.form.dirty=true;draw();return;}const c=api.current();if(e.target.dataset.cField==='files'){api.files([...e.target.files]);e.target.value='';}if(['root','runtime'].includes(e.target.dataset.cField)&&api.editable(c)){const field=e.target.dataset.cField;if(field==='runtime'&&(c.sent||c.request||c.sending))return;if(c.sent||c.sending)c.next=true;c[field]=e.target.value;c.version=(c.version||0)+1;api.save(c);draw();}});
    for(const event of ['select','keyup','click'])ui.listen(menu,event,e=>{if(e.target.dataset.cField!=='text')return;const c=api.current();if(api.editable(c)&&JSON.stringify(c.caret)!==JSON.stringify([e.target.selectionStart,e.target.selectionEnd])){c.caret=[e.target.selectionStart,e.target.selectionEnd];c.version=(c.version||0)+1;api.save(c);}});
    ui.listen(menu,'wheel',e=>{if(e.target.closest('.conversation-reader')){api.scrollIntent('wheel');if(e.deltaY<0)api.pause();}},{passive:true});
    ui.listen(menu,'touchmove',e=>{if(e.target.closest('.conversation-reader'))api.scrollIntent('touch');},{passive:true});
    ui.listen(menu,'pointerdown',e=>{if(e.target.closest('.conversation-reader')){dragScroll=true;api.scrollIntent('pointer');}});
    ui.listen(window,'pointerup',()=>{dragScroll=false;});ui.listen(window,'pointercancel',()=>{dragScroll=false;});
    ui.listen(menu,'scrollend',e=>{if(e.target.classList?.contains('conversation-reader')){scrollIntent=null;dragScroll=false;}},true);
    ui.listen(menu,'keydown',e=>{if(e.key==='Escape'&&e.target.dataset.cField==='search'&&!search){e.stopPropagation();searchOpen=false;draw();menu.querySelector('[data-c=search-open]')?.focus();return;}if(e.target.closest('.conversation-row')&&['ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();const rows=api.order(),at=rows.findIndex(r=>r.id===(e.target.dataset.thread||e.target.dataset.id)),next=rows[at+(e.key==='ArrowUp'?-1:1)];if(next){api.activate(next);menu.querySelector('.conversation-row[aria-current=page]')?.focus({preventScroll:true});menu.querySelector('.conversation-row[aria-current=page]')?.scrollIntoView({block:'nearest'});}return;}if(e.target.closest('.conversation-reader')&&!e.target.matches('input,textarea,select')&&['ArrowUp','ArrowDown','PageUp','PageDown','Home','End',' '].includes(e.key)){api.scrollIntent('key');if(['ArrowUp','PageUp','Home'].includes(e.key))api.pause();}});
    ui.listen(menu,'scroll',e=>{
     if(e.target.classList?.contains('conversation-rows')){scrolls[list]=e.target.scrollTop;api.nav();return;}
     if(!e.target.classList?.contains('conversation-reader'))return;
     // Native layout/clamping scrolls are not user navigation; only input or an explicit instrument updates the anchor.
     if(e.isTrusted&&(correcting||!scrollIntent&&!dragScroll))return;
     if(scrollIntent==='wheel'||scrollIntent==='focus')scrollIntent=null;
     if(!e.target.querySelector('.dialogue-transcript')){const a=api.mark(e.target);api.place(e.target.parentElement.dataset.threadId||selected,a?.board?api.boardPosition(a):Math.round(e.target.scrollTop));api.nav();return;}
     if(live?.thread===selected){const was=live.stick;if(e.target.scrollHeight-e.target.scrollTop-e.target.clientHeight>=24)live.stick=false;live.anchor=api.mark(e.target);const a=live.anchor&&live.items.find(i=>api.key(i)===live.anchor.key);api.place(selected,live.stick?undefined:live.anchor?.board?{key:live.anchor.key,y:Math.round(live.anchor.y)}:!a?undefined:{at:a.at,i:a.i,y:Math.round(live.anchor.y),...(live.source?{g:live.source}:{}),...(live.anchor.part!==undefined?{part:live.anchor.part}:{})});api.nav();api.edge(e.target,live);if(was!==live.stick)draw();return;}
     if(view?.id!==selected||view.end===null||!view.items.length)return;
     // Q-A3/A4: a reader at the end of a window that holds the file end follows; anywhere else keeps its record anchor
     // #1747: while Jump to latest replaces the window, a scroll event of the old window keeps following
     const shown=api.latest();if(!view.busy.initial&&(view.end<view.size||e.target.scrollHeight-e.target.scrollTop-e.target.clientHeight>=24))view.stick=false;if(view.stick)view.unseen=false;
     view.anchor=api.mark(e.target);const a=view.anchor&&view.items.find(item=>api.key(item)===view.anchor.key);
     api.place(selected,view.stick?undefined:view.anchor?.board?{key:view.anchor.key,y:Math.round(view.anchor.y)}:!a?undefined:{at:a.at,i:a.i,y:Math.round(view.anchor.y),...(view.source?{g:view.source}:{}),...(view.anchor.part!==undefined?{part:view.anchor.part}:{})});api.nav();
     const before=view.busy.before,after=view.busy.after;api.edge(e.target);if(shown!==api.latest()||before!==view.busy.before||after!==view.busy.after)draw();
    },true);
    ui.listen(menu,'toggle',e=>{if(e.target.classList?.contains('conversation-info')&&view){view.stick=false;const detail=menu.querySelector('.conversation-reader');if(detail)view.anchor=api.mark(detail);}},true);
    ui.listen(window,'resize',()=>{api.grow();api.layout();});
    ui.listen(menu,'dragover',e=>{if(e.target.closest('.conversations'))e.preventDefault();});
    ui.listen(menu,'drop',e=>{if(!e.target.closest('.conversations'))return;e.preventDefault();api.files([...e.dataTransfer.files]);});
    ui.listen(menu,'compositionstart',()=>{composing=true;});ui.listen(menu,'compositionend',()=>{composing=false;});
    ui.listen(document,'keydown',e=>{if(e.target.dataset?.cField==='text'&&e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&!composing&&e.keyCode!==229){e.preventDefault();const c=api.current();if(api.problem(c))return;if(api.launched(c))api.action('reply',{typed:c.version||0});else api.action('send',{runtime:api.runtime(c).id});}});
   },
   grow:function(){
    pm.grow(menu);
    const reader=menu.querySelector('.conversation-reader'),editor=reader?.querySelector(':scope>.conversation-composer,:scope>.answer-composer');
    if(reader){
     reader.classList.toggle('long-draft',!!editor&&editor.getBoundingClientRect().height>reader.clientHeight);
     const tools=editor?.querySelector('.composer-bottom');if(tools){const r=reader.getBoundingClientRect(),e=editor.getBoundingClientRect();reader.style.setProperty('--composer-left',e.left+'px');reader.style.setProperty('--composer-width',e.width+'px');reader.style.setProperty('--composer-bottom',Math.max(0,window.innerHeight-r.bottom)+'px');reader.style.setProperty('--composer-tools-height',tools.getBoundingClientRect().height+'px');}
    }
   },
   afterDraw:function(){
    api.taskTrim();
    if(view&&view.id!==selected){ui.cancel(pollTimer);pollTimer=null;for(const op of Object.values(reads))ui.cancel(op.timer);reads={};view=null;}
    if(!menu.querySelector('.worker-live')?.checkVisibility())api.workerStop();
    api.pollActivity();api.listEdge();
    const row=menu.querySelector('.conversation-row[aria-current=page]');if(row&&!row.checkVisibility()){const group=row.closest('details[data-notification-group]');if(group)group.open=true;}
    const body=api.current();api.bodyRead(body);api.grow();
    const log=menu.querySelector('.dialogue-transcript'),detail=menu.querySelector('.conversation-reader'),open=api.current(),identity=detail?.parentElement.dataset.threadId,changed=identity!==readerIdentity;readerIdentity=identity;if(changed&&row)row.scrollIntoView({block:'nearest'});
    if(changed&&detail&&!log){const p=positions[identity];if(typeof p==='number'&&detail.scrollTop!==p)detail.scrollTop=p;else if(!p&&detail.scrollTop!==0)detail.scrollTop=0;}
    if(log&&open)api.open(open);
    api.observe(detail);api.layout(true);
    if(live&&detail&&log){if(api.trim(detail,live))draw();else api.edge(detail,live);}
    const c=api.current(),input=menu.querySelector('[data-c-field=text]'),record=menu.querySelector('.conversation-record');const rows=menu.querySelector('.conversation-rows');if(focusSearch){focusSearch=false;menu.querySelector('[data-c-field=search]')?.focus();}if(listAnchor&&rows){const a=listAnchor;listAnchor=null;const n=[...rows.querySelectorAll('.conversation-row')].find(n=>(n.dataset.thread||n.dataset.id)===a.id);if(n)rows.scrollTop+=n.getBoundingClientRect().top-rows.getBoundingClientRect().top-a.y;}else if(rows&&rows.scrollTop!==scrolls[list]&&changed)rows.scrollTop=scrolls[list]||0;if(record&&c)record.scrollTop=c.scroll||0;if(focusDraft&&input&&!input.disabled){focusDraft=false;const row=menu.querySelector('[data-c=select][data-id="'+c.id+'"]');if(row?.checkVisibility()){row.scrollIntoView({block:'nearest'});scrolls[list]=rows.scrollTop;api.nav();}input.focus({preventScroll:true});input.setSelectionRange(...(c.caret||[0,0]));}
    if(changed&&identity&&focuses[identity]&&!focusDraft){const f=focuses[identity],target=[...detail.parentElement.querySelectorAll(f.tag)].filter(n=>JSON.stringify(n.dataset)===f.key)[f.index];if(target?.checkVisibility()&&!target.disabled)target.focus({preventScroll:true});}
    if(log&&open){if(api.trim(detail))draw();else api.edge(detail);}},
   bounds:function(){const resident=live||view;return {listRows:menu.querySelectorAll('.conversation-row').length,indexRows:items.length+sidebarItems.filter(r=>r.thread).length,queryKeys:resultPages.size,nativeRecords:(view?.items.length||0)+(live?.items.length||0),nativeBytes:[...(view?.items||[]),...(live?.items||[])].reduce((n,i)=>n+api.bytes(i),0),anchor:resident?.anchor||null,bodySources:bodyCache.size,bodyBytes:[...bodyCache.keys()].reduce((n,id)=>{const c=items.find(c=>c.id===id);return n+(c?new TextEncoder().encode(JSON.stringify(api.local(c))+JSON.stringify(c.sent||'')).length:0);},0),closedSources:closedViews.size,historyKeys:historyViews.size,historyRecords:[...historyViews.values()].reduce((n,v)=>n+(v.rows?.length||0),0),historyBytes:[...historyViews.values()].reduce((n,v)=>n+new TextEncoder().encode(JSON.stringify(v.rows||[])).length,0),textPages:textPages.size,bodyReads:bodyReads.size,taskBodies:taskBodies.size,taskBytes:[...taskBodies.values()].reduce((n,v)=>n+v.bytes,0),taskReads:taskReads.size,linkedRows:linked?.total||0,linkedKnown:linked?.known.size||0,linkedChecked:linked?.checked.size||0};},
   recovery:function(){return {drafts:[...recoveryDrafts,...items.filter(c=>!c.lazy&&(c.saved??0)!==(c.version??0)).map(c=>({id:c.id,revision:c.revision,version:c.version,...api.local(c),files:c.files,sentRevision:c.recoverySource?c.recoverySource.sentRevision:c.sent?.revision}))],pending:pending||failure,localQueue:[...queue.filter(o=>['save','attach','remove'].includes(o.action)),...recoveryQueue],context:{selected,list,selection,scrolls,positions,focuses,opened,listOnly,search,filter:resultFilter}};},
   restoreDrafts:function(){
    recoveryDrafts=recoveryDrafts.filter(draft=>{
     const c=items.find(c=>c.id===draft.id);if(!c||c.deleted||c.lazy)return true;
     const mismatch=c.root!==draft.root||c.sent?.revision!==draft.sentRevision;
     Object.assign(c,api.local(draft),{files:draft.files,lazy:false,recoverySource:{sentRevision:draft.sentRevision},version:(draft.version??0)+1,saved:0});c.recoveryReason=mismatch?'Recovered draft source changed; text/files retained. Use New from draft or inspect the source.':null;api.note(c.recoveryReason||'Recovered local edits; pending delivery requires the same receipt.',c.id);if(c.id===selected)focusDraft=true;return false;
    });
   },
   restoreRecovery:function(r){
    recoveryDrafts=r?.drafts||[];api.restoreDrafts();
    queue=[];recoveryQueue=(r?.localQueue||[]).filter(o=>['save','attach','remove'].includes(o.action));
    const interrupted=r?.pending||recoveryQueue.shift();
    if(interrupted){failure={...interrupted,reason:'Reader reloaded; original operation outcome unknown. Check delivery or explicitly retry the native save.'};const c=items.find(c=>c.id===interrupted.conversation);if(c&&interrupted.action==='reply')c.reply={id:interrupted.id};api.note(failure.reason,interrupted.conversation);}

    if(r?.context){navigated=true;unified=true;const c=r.context;({selected,list,selection,scrolls,positions,focuses,opened,listOnly,search}=c);resultFilter=c.filter;focusDraft=!!api.current();onSelect?.(selected);}
   },
   recoverable:function(){return !recoveryDrafts.length&&!pending&&!failure&&!queue.length&&!recoveryQueue.length&&!items.some(c=>!c.lazy&&(c.saved??0)!==(c.version??0));},
   flush:function(){ui.cancel(saveTimer);const c=api.current();if(c&&!c.lazy)api.save(c);api.nav();},
  });return api;
 }
});
