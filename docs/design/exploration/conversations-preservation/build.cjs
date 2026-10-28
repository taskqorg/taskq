// Review-only copy of current components. No application source or native effects are changed.
'use strict';
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),cp=require('node:child_process');
const root=path.resolve(__dirname,'../../../..'),here=__dirname;
const lab=process.env.TASKQ_REVIEW_LAB||'/Users/kirs/.codex/visualizations/2026/10/27/01a17f18-a2ef-7dc0-9c63-b55a366083ae/source-ui-lab';
const esbuild=require(path.join(lab,'node_modules/esbuild')),copy=path.join(lab,'candidate-src'),out=path.join(here,'preview');
fs.mkdirSync(copy,{recursive:true});fs.mkdirSync(out,{recursive:true});
const hashes={},patches=[];const sha=s=>crypto.createHash('sha256').update(s).digest('hex');
for(const name of fs.readdirSync(path.join(root,'src')).filter(n=>/\.(js|css|html|json)$/.test(n))){const bytes=fs.readFileSync(path.join(root,'src',name));hashes[name]=sha(bytes);fs.writeFileSync(path.join(copy,name),bytes);}
function edit(name,from,to){const p=path.join(copy,name),s=fs.readFileSync(p,'utf8');if(!s.includes(from))throw Error('Source drift: '+name+' / '+from.slice(0,60));fs.writeFileSync(p,s.replace(from,to));patches.push({file:name,reason:to.includes('candidate-reading')?'Separate reading from composer':'Review interaction correction',before:from,after:to});}
// Reuse actual details/actions; change only where the question is read and who owns scrolling.
edit('task-dashboard.js','app.taskRow(t,true)','api.row(t,true)');
edit('task-dashboard.js','rows.map(t=>app.taskRow(t,true))','rows.map(t=>api.row(t,true))');
edit('task-dashboard.js','group.map(t=>app.taskRow(t,false))','group.map(t=>api.row(t,false))');
edit('task-dashboard.js','   question:function(t){',"   row:function(t,project){return h(Fragment,null,app.taskRow(t,project),view.popup?.kind==='task'&&view.popup.key===t.key&&h('section',{id:'taskq-popover',class:'candidate-task-detail',tabIndex:-1,'aria-label':'Task details'},api.detail()));},\n   question:function(t){");
let td=fs.readFileSync(path.join(copy,'task-dashboard.js'),'utf8'),a=td.indexOf('   question:function(t){'),b=td.indexOf('   detail:function(){',a);if(a<0||b<a)throw Error('Question boundary');
patches.push({file:'task-dashboard.js',reason:'Move complete question/options to the existing exact task thread; keep short cue',before:td.slice(a,b)});
td=td.slice(0,a)+"   question:function(t){if(t.state!=='ask')return null;const text=t.question?.parsed?t.question.problem:t.question?.text||t.card||'Open the question to read and answer.';return h('p',{class:'candidate-question-cue'},text.length>170?text.slice(0,167)+'…':text);},\n"+td.slice(b);
const da=td.indexOf('   detail:function(){'),db=td.indexOf('   // P-W4:',da);let detail=td.slice(da,db);
detail=detail.replace("    return h(Fragment,null,h('div',{class:'popover-heading'}", "    const body=h(Fragment,null,h('div',{class:'popover-heading'}");
// Keep the short cue beside the existing task actions.

detail=detail.replace("     app.answerNotices(p.issues));", "     app.answerNotices(p.issues));return body;");
td=td.slice(0,da)+detail+td.slice(db);
fs.writeFileSync(path.join(copy,'task-dashboard.js'),td);
edit('task-dashboard.js','popup:function(){const p=view.popup;if(!p)return null;','popup:function(){const p=view.popup;if(!p||p.kind===\'task\')return null;');
edit('task-dashboard.js','if(view.popup)popover.place(','if(view.popup&&view.popup.kind!==\'task\')popover.place(');
edit('tray-app.js',"t.state==='ask'?'Answer in Conversations…':'Open Conversations…'","t.state==='ask'?'Open question':'Open conversation'");
// A project identity has three distinct targets; reuse native overview/folder actions.
edit('task-dashboard.js',"   bar:function(){", "   projectHeader:function(p){const board=require('./review-board-links.js').board(p.issues);return h('div',{class:'project-group project-shortcuts'},p.issues?h('a',{class:'project-board compact-action',href:p.issues,'data-url':p.issues,title:'Open '+p.name+' board on '+board.name,'aria-label':'Open '+p.name+' board on '+board.name},icon(board.kind)):button('external-link','Project board unavailable',{disabled:true,class:'compact-action'}),h('button',{class:'project-overview','data-route':p.root,disabled:!p.root,title:'Open '+p.name+' project overview','aria-label':'Open '+p.name+' project overview'},p.name),button('folder',p.root?'Open '+p.name+' project folder on this computer':'Project folder unavailable: no registered root',{'data-a':'folder','data-root':p.root,disabled:!p.root,class:'compact-action project-folder'}));},\n   bar:function(){");
edit('task-dashboard.js',"h('div',{class:'project-group'},h('button',{'data-route':p.root},icon('folder'),p.name),h('a',{href:p.issues,'data-url':p.issues,title:'Open '+p.name+' Board','aria-label':'Open '+p.name+' Board'},icon(p.board_kind)))", "api.projectHeader(p)");
// Keep all source answer-card behavior; narrow only the selected-thread historical receipt feed.
edit('workspace-answers.js','answerNotices:function(board){','answerNotices:function(board,task){');
edit('workspace-answers.js',"(!board||r.board===board)&&!view.S", "(!board||r.board===board)&&(task==null||r.op.task===task)&&!view.S");
edit('workspace.js','answers.answerNotices(p.issues)','answers.answerNotices(p.issues,t.n)');
edit('workspace-answers.js',"  const answered=shown.filter","  if(task!=null)return shown.map(x=>line(x,'status'));\n  const answered=shown.filter");
edit('workspace.js',"answers.answerToast(),h('footer'","null,h('footer'");
// Split only the existing composer vnode. Other source children/actions/validators stay intact.
edit('conversations.js','   body:function(extra={}){',"   splitThread:function(detail){const flat=(xs)=>Array.isArray(xs)?xs.flatMap(flat):[xs],parts=flat(detail),input=parts.find(x=>x?.props?.class==='answer-composer');return [h('div',{class:'candidate-reading'},parts.filter(x=>x!==input)),input];},\n   body:function(extra={}){");
edit('conversations.js',"'aria-label':'Selected thread'},extra.detail)","'aria-label':'Selected thread'},api.splitThread(extra.detail))");
edit('conversations.js',"       dialogue?[h('div'","       h('div',{class:'candidate-reading'},dialogue?[h('div'");
edit('conversations.js','       extra.notice,','       extra.notice),');
// The reading element now owns the same existing source scroll capture/restore and pagination hooks.
let cv=fs.readFileSync(path.join(copy,'conversations.js'),'utf8');cv=cv.replaceAll("querySelector('.conversation-detail')","querySelector('.candidate-reading')");
cv=cv.replace("h('div',{class:'candidate-reading'},dialogue?","h('div',{class:'candidate-reading','data-thread-id':c.id},dialogue?");
cv=cv.replace("h('div',{class:'candidate-reading'},parts.filter", "h('div',{class:'candidate-reading','data-thread-id':selected},parts.filter");
fs.writeFileSync(path.join(copy,'conversations.js'),cv);
edit('conversations.js',"let view=null,reads={},pollTimer=null,opened=[],positions={},synced=false,unified=false;","let view=null,reads={},pollTimer=null,opened=[],positions={},synced=false,unified=false,previewReturn=null,previewFocus=false,previewRestore=false,previewFile=null;");
edit('conversations.js',"if(op.action==='preview'&&r.value){","if(op.action==='preview'&&r.value){previewReturn=menu.querySelector('.candidate-reading')?.scrollTop??0;previewFile=op.file;previewFocus=true;if(view)view.stick=false;");
edit('conversations.js',"else if(action==='preview-close'){preview=null;draw();}","else if(action==='preview-close'){preview=null;previewRestore=true;draw();}");
edit('conversations.js',"if(record&&c)record.scrollTop=c.scroll||0;if(focusDraft","if(record&&c)record.scrollTop=c.scroll||0;const reading=menu.querySelector('.candidate-reading');if(reading&&preview&&previewFocus){previewFocus=false;reading.scrollTop=0;menu.querySelector('[data-c=preview-close]')?.focus({preventScroll:true});}if(reading&&previewRestore){previewRestore=false;reading.scrollTop=previewReturn??0;menu.querySelector('[data-c=preview][data-file=\"'+CSS.escape(previewFile||'')+'\"]')?.focus({preventScroll:true});}if(focusDraft");
edit('conversations.js',"   mount:function(){\n    ui.listen(menu,'input'","   mount:function(){\n    ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&preview){preview=null;previewRestore=true;e.preventDefault();e.stopImmediatePropagation();draw();}});\n    ui.listen(menu,'input'");
fs.copyFileSync(path.join(here,'activity.js'),path.join(copy,'review-activity.js'));
fs.copyFileSync(path.join(here,'journey.js'),path.join(copy,'review-journey.js'));
edit('conversations.js',"if(s.workspaceOwner){owner=s.workspaceOwner;", "if(s.workspaceOwner){api.flush();owner=s.workspaceOwner;");
edit('conversations.js','api.transcript(c)]:',"api.transcript(c),require('./review-journey.js')(snapshot,c.id)]:");
edit('conversations.js','   receive:function(s){',"   receive:function(s){if(s.reviewTick&&view){if(!view.busy.status)api.read('status');if(view.end!==null&&!view.busy.after)api.read('after',{after:view.end});return true;}");
edit('conversations.js',"label={user:'You',assistant:'Assistant'}", "label={user:'You',assistant:'Project manager'}");
edit('workspace.js',"return [h('div',{class:'conversation-tools thread-head'}", "return [require('./review-journey.js')(snapshot.value,null,row.task),h('div',{class:'conversation-tools thread-head'}");
edit('workspace.js',"sessions:{title:'Work sessions'", "sessions:{title:'Task activity'");
edit('conversations.js',"class:'conversation-detail','data-thread-id':c?.id||''", "class:'conversation-detail'+(c&&!c.sent&&!c.request?' new-chat':''),'data-thread-id':c?.id||''");
edit('conversations.js',"'New request')", "'New chat')");
let sessionCode=fs.readFileSync(path.join(copy,'conversations.js'),'utf8'),ss=sessionCode.indexOf('   sessions:function(){'),se=sessionCode.indexOf('   click:function(b){',ss);if(ss<0||se<ss)throw Error('Sessions source drift');sessionCode=sessionCode.slice(0,ss)+"   sessions:function(){return require('./review-activity.js')(snapshot,navigate);},\n"+sessionCode.slice(se);fs.writeFileSync(path.join(copy,'conversations.js'),sessionCode);
edit('conversations.js',"   launched:function(c){","   rowStatus:function(c){if(api.launched(c))return ({working:'Working',idle:'Ready'}[api.state(c)]||'Status unknown');return c.request?({pending:'Waiting to start',launching:'Starting chat',failed:'Could not start',interrupted:'Start interrupted',unknown:'Delivery unknown'}[c.request.state]||'Status unknown'):c.sent?'Delivery unknown':'Draft';},\n   launched:function(c){");
edit('conversations.js',"(row.request?.kind||(row.sent?'Delivery unknown':'Draft'))+(api.launched(row)?' · '+api.state(row):'')","api.rowStatus(row)");
edit('conversations.js',"class:'conversation-record',tabindex:'0'","class:'conversation-record','data-start-state':c.request?.state||(c.sent?'unknown':'draft'),tabindex:'0'");
// Presentation layer changes the source vnode tree without changing event dispatch or validation.
fs.copyFileSync(path.join(here,'presentation.js'),path.join(copy,'review-presentation.js'));
for(const name of ['reading','board-links'])fs.copyFileSync(path.join(here,name+'.js'),path.join(copy,'review-'+name+'.js'));
const reviewIcons=JSON.parse(fs.readFileSync(path.join(here,'review-icons.json'),'utf8'));
const originalIcons=JSON.parse(fs.readFileSync(path.join(copy,'ui-icons.json'),'utf8'));
fs.writeFileSync(path.join(copy,'ui-icons.json'),JSON.stringify({...originalIcons,...reviewIcons}));
edit('app-shell.js',"frame.value={route,depth:location.history.length,nodes};","frame.value={route,depth:location.history.length,nodes:require('./review-presentation.js').present(nodes)};");
edit('conversations.js',"   read:function(kind,extra={}){", "   activity:function(c){const state=api.state(c),at=view?.id===c.id?view.status?.turn?.at:null;return state==='working'?'PM is replying'+(Number.isFinite(at)?' · '+Math.max(0,Math.floor((Date.now()-at)/1000))+' s':''):state==='idle'?'PM is ready':'PM status unknown';},\n   read:function(kind,extra={}){");
edit('conversations.js',"'State: '+api.state(c)","api.activity(c)");
edit('conversations.js',"view.items.push(...v.items.map((item,i)=>({...item,n:last+1+i})));view.end=v.end;","view.items.push(...v.items.map((item,i)=>({...item,n:last+1+i})));view.end=v.end;if(v.items.length&&!view.busy.status)api.read('status');");
// The composer has one fixed input/actions region. Configuration and explanatory text remain reachable in a labelled disclosure.
let pm=fs.readFileSync(path.join(copy,'pm-composer.js'),'utf8');
const ps=pm.indexOf('  return h(\'div\',{class:\'conversation-composer\'},before'),pe=pm.indexOf('\n },',ps);
if(ps<0||pe<ps)throw Error('Composer boundary');
const replacement=`  const flat=x=>Array.isArray(x)?x.flatMap(flat):x==null?[]:[x],parts=flat(before),project=parts.find(x=>x?.type==='label')?.props.children,select=flat(project).find(x=>x?.type==='select'),option=select&&flat(select.props.children).find(x=>x?.props?.value===select.props.value),name=option?.props.children||'Project',meta=parts.find(x=>x?.type==='p')?.props.children||'';
  const hint=reason||'',quiet=/^(Write a message|Write a request before|Type an answer|Send goes to|Share this request|Replies open here)/.test(hint),task=field==='answer';
  const toolNodes=flat(tools?.props?.children).filter(x=>!(task&&x?.type==='label'&&x.props['aria-disabled']));
  return h('div',{class:'conversation-composer'},before&&h('details',{class:'composer-destination shell-overflow','data-view-key':'composer-destination'},h('summary',{title:'Message destination and conversation options'},h('span',{class:'ui-icon','aria-hidden':'true',dangerouslySetInnerHTML:{__html:require('./ui-icons.js').asset('folder')}}),h('span',null,name+' · '+String(meta).replace(/^Session /,''))),h('div',{class:'composer-options overflow-actions'},before)),h('label',{for:id,class:'sr-only'},label),!task&&files,h('div',{class:'composer-input'},h('textarea',{id,'data-c-field':field,rows:2,value:text,disabled,'aria-describedby':id==='conversation-draft'?'conversation-reason':id+'-reason',placeholder,onInput})),h('div',{class:'composer-bottom'},h('div',{class:'conversation-tools'},toolNodes),h('details',{class:'composer-help shell-overflow','data-view-key':'composer-help'},h('summary',{title:task?'Answer help and file support':'File limits and message help','aria-label':task?'Answer help':'File limits and message help'},require('./ui-icons.js').icon('info')),h('div',{class:'composer-options overflow-actions'},task&&files,h('p',null,hint),after))),h('p',{id:id==='conversation-draft'?'conversation-reason':id+'-reason',class:'meta composer-reason'+(quiet||!hint?' sr-only':'')},hint));`;
pm=pm.slice(0,ps)+replacement+pm.slice(pe);fs.writeFileSync(path.join(copy,'pm-composer.js'),pm);
// Static exported task-control markup keeps the same labels/dispatch tuple and receives the same bundled icons.
edit('task-controls.js',"'+LABELS[a]+'</button>'", "'+require('./ui-icons.js').asset(({start:'play',stop:'square',later:'clock-3',retry:'rotate-ccw'})[a])+'</button>'");
edit('task-controls.js',"aria-label=\"'+LABELS[a]+' #'+row.n+'\">", "title=\"'+LABELS[a]+' #'+row.n+'\" aria-label=\"'+LABELS[a]+' #'+row.n+'\" class=\"compact-action\">");

// Scope transient owner-conversation feedback too; a late receipt must not describe another selected thread.
let notices=fs.readFileSync(path.join(copy,'conversations.js'),'utf8');
notices=notices.replace("undo=null,notice='',failure", "undo=null,noticeByThread=new Map(),failure");
notices=notices.replace("   id:function(){", "   notice:function(value,owner=selected){noticeByThread.set(owner,value);},\n   id:function(){");
notices=notices.replace("},notice,pending&&' · Saving…'", "},noticeByThread.get(selected)||'',pending?.conversation===selected&&' · Saving…'");
const acorn=require(path.join(lab,'node_modules/acorn')),assignmentEdits=[];
function noticeWalk(n){if(!n||typeof n!=='object')return;if(n.type==='AssignmentExpression'&&n.operator==='='&&n.left.type==='Identifier'&&n.left.name==='notice'){
 const value=notices.slice(n.right.start,n.right.end);let owner="typeof op==='object'&&op?op.conversation:selected";
 if(value.includes('result.reason')||value.includes('File read finished'))owner='cid';
 if(value.includes('Draft and files moved')||value==="'Restored'")owner='selected';
 assignmentEdits.push({start:n.start,end:n.end,text:'api.notice('+value+','+owner+')'});
 }for(const [k,v]of Object.entries(n))if(k!=='start'&&k!=='end'){if(Array.isArray(v))v.forEach(noticeWalk);else if(v&&typeof v==='object')noticeWalk(v);}}
noticeWalk(acorn.parse(notices,{ecmaVersion:'latest'}));if(assignmentEdits.length<15)throw Error('Notice assignments drift');
for(const e of assignmentEdits.sort((a,b)=>b.start-a.start))notices=notices.slice(0,e.start)+e.text+notices.slice(e.end);
notices=notices.replace("if(op.action==='preview'&&r.value){", "if(op.action==='preview'&&r.value&&selected===op.conversation){");
notices=notices.replace("value=>{if(value.file){preview=value", "value=>{if(selected!==op.conversation)return;if(value.file){preview=value");
notices=notices.replace("'Undo archive'", "'Undo archiving '+api.title(items.find(row=>row.id===undo)||{})");
fs.writeFileSync(path.join(copy,'conversations.js'),notices);

// Empty search is an explicit transient control; a saved nonempty query is never hidden.
edit('task-dashboard.js',"bar:function(){const m=api.model(),v=api.current(),c=v.working;", "bar:function(){const m=api.model(),v=api.current(),c=v.working,searching=!!c.query||!!view.searchOpen;");
edit('task-dashboard.js',"button('sliders-horizontal','Edit view conditions',", "button('search','Search tasks',{'data-a':'search-open','aria-controls':'task-query','aria-expanded':String(searching)}),button('sliders-horizontal','Edit view conditions',");
edit('task-dashboard.js',"h('label',{class:'task-search'},icon('search'),h('input',{type:'search','data-task-query':'',maxLength:1024,'aria-label':'Search tasks',placeholder:'Search tasks',value:c.query}))", "searching&&h('div',{class:'task-search'},icon('search'),h('input',{id:'task-query',type:'search','data-task-query':'',maxLength:1024,'aria-label':'Search tasks',placeholder:'Search tasks',value:c.query}),c.query&&button('x','Clear task search',{'data-a':'search-clear'}))");
edit('task-dashboard.js',"const d=b.dataset,m=api.model();", "const d=b.dataset,m=api.model();if(d.a==='search-open'){view.searchOpen=!!api.criteria().query||!view.searchOpen;present();menu.querySelector(view.searchOpen?'[data-task-query]':'[data-a=search-open]')?.focus({preventScroll:true});return true;}if(d.a==='search-clear'){api.criteria().query='';view.searchOpen=false;present();menu.querySelector('[data-a=search-open]')?.focus({preventScroll:true});return true;}");
edit('task-dashboard.js',"   mount:function(){", "   mount:function(){\n    ui.listen(menu,'focusin',e=>{if(e.target.hasAttribute('data-task-query'))view.searchOpen=true;});\n    ui.listen(menu,'focusout',e=>{if(e.target.hasAttribute('data-task-query'))ui.defer(()=>{if(!api.criteria().query&&view.searchOpen&&!document.activeElement?.closest('.task-search')){view.searchOpen=false;present();}},0);});\n    ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&e.target.hasAttribute('data-task-query')){e.preventDefault();e.stopImmediatePropagation();if(!api.criteria().query)view.searchOpen=false;present();menu.querySelector('[data-a=search-open]')?.focus({preventScroll:true});}},true);");
edit('task-dashboard.js',"h('header',{class:'filter-header tqp',dangerouslySetInnerHTML:{__html:blocks.counters(counts.counts,'','Selected projects',partial,c.states,counts.open)}})", "h('header',{class:'filter-header tqp'},h('div',{class:'state-filters',dangerouslySetInnerHTML:{__html:blocks.counters(counts.counts,'','Selected projects',partial,c.states,counts.open)}}),rows.filter(t=>t.state==='held').length>1&&h('span',{class:'held-shortcut'},app.startButton(rows)))");
edit('task-dashboard.js',"rows.filter(t=>t.state==='held').length>1&&h('div',{class:'row'},app.startButton(rows)),", "");

// Apply the same explicit-search rule to the only other persistent search: the chat list.
edit('conversations.js',"list='recent',search='',selection", "list='recent',search='',searchOpen=false,searchReturn=false,selection");
edit('conversations.js',"listOnly?'Back':'List'),h('button',{'data-c':'new'", "listOnly?'Back':'List'),!owner.conversation&&icons.button('search','Search conversations',{'data-c':'search-open','aria-expanded':String(!!search||searchOpen),'aria-controls':'thread-search'}),h('button',{'data-c':'new'");
edit('conversations.js',"h('label',{class:'conversation-search'},h('span',{class:'sr-only'},'Search threads'),h('input',{type:'search','data-c-field':'search',value:search,placeholder:'Search',maxLength:512})),search&&h('button',{'data-c':'clear-search'},'Clear search')", "(search||searchOpen)&&h('div',{class:'conversation-search'},h('label',{class:'sr-only',for:'thread-search'},'Search threads'),h('input',{id:'thread-search',type:'search','aria-label':'Search threads','data-c-field':'search',value:search,placeholder:'Search chats',maxLength:512}),search&&icons.button('x','Clear thread search',{'data-c':'clear-search'}))");
edit('conversations.js',"if(action==='new'&&!owner.conversation)api.fresh();", "if(action==='search-open'){searchReturn=listOnly;searchOpen=true;listOnly=true;draw();menu.querySelector('[data-c-field=search]')?.focus({preventScroll:true});}else if(action==='new'&&!owner.conversation)api.fresh();");
edit('conversations.js',"else if(action==='clear-search'){search='';api.nav();draw();}", "else if(action==='clear-search'){search='';searchOpen=false;api.nav();draw();menu.querySelector('[data-c=search-open]')?.focus({preventScroll:true});}");
edit('conversations.js',"items.push(c);list='recent';search='';selected=c.id;", "items.push(c);list='recent';search='';searchOpen=false;selected=c.id;");
edit('conversations.js',"   mount:function(){", "   mount:function(){\n    ui.listen(menu,'focusin',e=>{if(e.target.dataset.cField==='search')searchOpen=true;});\n    ui.listen(menu,'focusout',e=>{if(e.target.dataset.cField==='search')ui.defer(()=>{if(!search&&searchOpen&&!document.activeElement?.closest('.conversation-search')){searchOpen=false;draw();}},0);});\n    ui.listen(document,'keydown',e=>{if(e.key==='Escape'&&e.target.dataset.cField==='search'){e.preventDefault();e.stopImmediatePropagation();if(!search){searchOpen=false;listOnly=searchReturn;}api.nav();draw();menu.querySelector('[data-c=search-open]')?.focus({preventScroll:true});}},true);");

// One reading owner, natural input height and anchored follow state replace source pixel-only restoration.
edit('conversations.js',"  const api=execution.module('conversations.instance',{", "  const scrolling=require('./review-reading.js')(menu),dialogueViews=new Map();\n  const api=execution.module('conversations.instance',{");
edit('conversations.js',"capture:function(){const el=menu.querySelector('.candidate-reading');if(el?.dataset.threadId)positions[el.dataset.threadId]=el.scrollTop;}","capture:function(){scrolling.capture();const el=menu.querySelector('.candidate-reading');if(el?.dataset.threadId)positions[el.dataset.threadId]=el.scrollTop;}");
edit('conversations.js',"const action=b.dataset.c;if(!action)return false;", "const action=b.dataset.c;if(action==='jump-latest'){scrolling.follow();return true;}if(!action)return false;");
edit('conversations.js',"view.stick=e.target.scrollHeight-e.target.scrollTop-e.target.clientHeight<24;", "view.stick=scrolling.following();");
edit('conversations.js',"classList?.contains('conversation-detail')", "classList?.contains('candidate-reading')");
edit('conversations.js',"class:'dialogue-item '+item.role", "class:'dialogue-item '+item.role,'data-record':item.n");
let mechanics=fs.readFileSync(path.join(copy,'conversations.js'),'utf8');
mechanics=mechanics.replace("if(view?.id!==c.id){reads={};view={", "if(view?.id!==c.id){if(view)dialogueViews.set(view.id,view);const prior=dialogueViews.get(c.id);reads={};view={");
mechanics=mechanics.replace("api.read('initial');api.read('status');}","if(prior){view={...prior,busy:{},anchor:null};api.read('after',{after:view.end});}else api.read('initial');api.read('status');}");
const sa=mechanics.indexOf('    if(detail)detail.scrollTop=positions[selected]'),sb=mechanics.indexOf('    const c=api.current(),input=',sa);if(sa<0||sb<sa)throw Error('Reading restore drift');mechanics=mechanics.slice(0,sa)+"    if(log&&open)api.open(open);if(view)view.anchor=null;\n"+mechanics.slice(sb);
mechanics=mechanics.replace("input.setSelectionRange(...(c.caret||[0,0]));}},", "input.setSelectionRange(...(c.caret||[0,0]));}scrolling.after(selected,{preview:!!preview});},");
fs.writeFileSync(path.join(copy,'conversations.js'),mechanics);
// Context needed to answer is visible; only technical metadata remains on demand.
edit('answer-card.js','<details class="qac-context"><summary>Checked &amp; context</summary>', '<section class="qac-context">');
edit('answer-card.js',".join(' · ') + '</p>' : '') + '</details>';", ".join(' · ') + '</p>' : '') + '</section>';");
// The menu keeps its task identity inspector; its existing question glyph opens the exact question directly.
edit('project-blocks.js',"'+icon(mark)+'</span>'+run+elapsed", "'+(row.state==='ask'?'<button class=\"question-shortcut\" data-a=\"workspace\" data-page=\"answer\" data-root=\"'+esc(row.root||p.root||'')+'\" data-board=\"'+esc(row.key.slice(0,row.key.lastIndexOf('/')))+'\" data-revision=\"'+row.revision+'\" data-task=\"'+row.n+'\" aria-label=\"Open question #'+row.n+'\" title=\"Open question #'+row.n+'\">'+icon(mark)+'</button>':icon(mark))+'</span>'+run+elapsed");
edit('task-controls.js',"Close on board ↗</a>","'+require('./review-board-links.js').asset(url)+'</a>");
edit('task-controls.js',"aria-label=\"Close #'+row.n+' on the board\">", "class=\"compact-action board-close\" title=\"Open #'+row.n+' on '+require('./review-board-links.js').board(url).name+' to close\" aria-label=\"Open #'+row.n+' on '+require('./review-board-links.js').board(url).name+' to close\">");

function bundle(dir,name,globalName){return esbuild.buildSync({entryPoints:[path.join(dir,name+'.js')],bundle:true,write:false,platform:'browser',format:'iife',minify:true,globalName,target:'es2022',nodePaths:[path.join(lab,'node_modules')]}).outputFiles[0].text;}
const style=['task-workspace','answer-card','project-blocks','resource-controls','app-shell','task-dashboard'].map(n=>fs.readFileSync(path.join(root,'src',n+'.css'),'utf8')).join('\n');
const nativePalette=fs.readFileSync(path.join(root,'src/icon.rs'),'utf8'),paletteRows=[...nativePalette.matchAll(/Palette \{ ink:\[([^\]]+)\], work:\[([^\]]+)\], ask:\[([^\]]+)\], muted:\[([^\]]+)\], failed:\[([^\]]+)\], review:\[([^\]]+)\]/g)];if(paletteRows.length!==2)throw Error('Palette source drift');const hex=s=>'#'+s.split(',').slice(0,3).map(n=>Number(n).toString(16).padStart(2,'0').toUpperCase()).join(''),palettes=Object.fromEntries(['dark','light'].map((name,i)=>[name,`--work:${hex(paletteRows[i][2])};--ask:${hex(paletteRows[i][3])};--failed:${hex(paletteRows[i][5])};--ready:${hex(paletteRows[i][4])};--muted:${hex(paletteRows[i][4])};--review:${hex(paletteRows[i][6])}`]));
const bridge=fs.readFileSync(path.join(here,'frame.js'),'utf8'),candidateCSS=fs.readFileSync(path.join(here,'candidate.css'),'utf8');
for(const baseline of [false,true])for(const surface of ['menu','conversations','settings','views']){
 const dir=baseline?path.join(root,'src'):copy,workspace=surface==='conversations';
 const js=workspace?bundle(dir,'workspace-entry'):bundle(dir,'task-workspace','TaskqTaskWorkspace')+';'+fs.readFileSync(path.join(dir,'tray-entry.js'),'utf8');
 const init=`globalThis.taskqSurface=${JSON.stringify(surface==='menu'?'menu':surface)};globalThis.reviewBaseline=${baseline};globalThis.reviewPalettes=${JSON.stringify(palettes)};\n`+bridge+(!baseline?fs.readFileSync(path.join(here,'tooltips.js'),'utf8'):'');
 const html='<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark"><style>'+style+(workspace?fs.readFileSync(path.join(root,'src/conversations.css'),'utf8'):'')+(!baseline?candidateCSS:'')+'</style></head><body><div class="menu'+(workspace?' workspace':'')+'" role="menu"></div><script>'+init.replaceAll('</script','<\\/script')+'</script><script>'+js.replaceAll('</script','<\\/script')+'</script></body></html>';
 fs.writeFileSync(path.join(out,(baseline?'baseline-':'')+surface+'.html'),html);
}
let reviewHTML=fs.readFileSync(path.join(here,'index.html'),'utf8');reviewHTML=reviewHTML.replace(/<i data-icon="([^"]+)"><\/i>/g,(_,name)=>reviewIcons[name]||originalIcons[name]||'');fs.writeFileSync(path.join(out,'index.html'),reviewHTML);
for(const name of ['fixture.js','fixture.js','features.json','seed.json','review-icons.LICENSE'])fs.copyFileSync(path.join(here,name),path.join(out,name));
cp.execFileSync('python3',['/Users/kirs/.codex/plugins/cache/openai-bundled/visualize/1.0.55/skills/visualize/scripts/render.py',path.join(here,'../conversations-usability.html'),path.join(out,'interaction.html'),'--force']);
let patch='';for(const name of Object.keys(hashes)){if(!fs.readFileSync(path.join(copy,name)).equals(fs.readFileSync(path.join(root,'src',name)))){const d=cp.spawnSync('diff',['-u',path.join(root,'src',name),path.join(copy,name)],{encoding:'utf8'});if(d.status!==1)throw Error('diff failed');patch+=d.stdout;}}fs.writeFileSync(path.join(out,'candidate-patch.diff'),patch);
fs.writeFileSync(path.join(out,'source-manifest.json'),JSON.stringify({baseline:cp.execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),sourceHashes:hashes,nativePaletteSource:{path:'src/icon.rs',sha256:sha(nativePalette)},reviewAssets:Object.fromEntries(['presentation.js','activity.js','journey.js','reading.js','board-links.js','fixture.js','tooltips.js','candidate.css','review-icons.json','review-icons.LICENSE','frame.js'].map(name=>[name,sha(fs.readFileSync(path.join(here,name)))])),candidatePatches:patches,scope:'Review copies only. Native bridge is a synthetic local reducer; not the installed app.',retainedFramework:'Preact/Signals/Wouter/CoreQ; existing dependency pins'},null,2)+'\n');
console.log('Built source-backed review: '+out);
