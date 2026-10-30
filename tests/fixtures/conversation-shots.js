// #1699 A8: native Conversations captures (`tray --snapshot --route conversations`) over the real pushed state with
// invented projects, a task question, an unread notification and an owner draft; nothing reaches a board or session.
function taskqConversationShot(step,state){
 const board='https://github.com/acme/storefront/issues',q='1'.repeat(64);
 const card='Done: Cart merge reaches duplicate products.\nProblem: Which quantity should a duplicate product keep?\nEvidence: [#41](https://github.com/acme/storefront/issues/41)\n1. Keep the larger quantity ★\n2. Add the quantities\nTo answer: `taskq answer 41.K` or comment `K` or the option text.';
 const row={n:41,title:'Keep the cart',state:'ask',url:board+'/41',revision:4,card,question:{parsed:true,text:card,done:'Cart merge reaches duplicate products.',problem:'Which quantity should a duplicate product keep?',evidence:[{label:'#41',url:'https://github.com/acme/storefront/issues/41'}],
  choices:[{id:1,text:'Keep the larger quantity',recommended:true,consequential:false},{id:2,text:'Add the quantities',recommended:false,consequential:false}],limits:{lines:4,chars:240}}};
 const s={...state,host:'example-mac',projects:[{root:'storefront',name:'storefront',issues:board,alive:true,complete:true,age:1,tasks_age_seconds:1,focus:[],labels:[],tasks:[row,{n:43,title:'Fix the login redirect',state:'doing',revision:1}]}],
  messages:{complete:true,reason:'',count:1,entries:[
   {key:q,kind:'question',title:row.title,project:'storefront',root:'storefront',board,task:row.n,revision:row.revision,state:'ask',pending:true,read:false,archived:false}]},
  conversations:{complete:true,items:[{id:'owner-1',revision:1,title:'Keep carts after sign-in',text:'Keep the cart after sign-in and return shoppers to their previous page. Plan the work first; ask before starting tasks.',root:'storefront',caret:[0,0],scroll:0,files:[],archived:false},
   {id:'pm-1',revision:1,title:'Release planning',text:'',root:'storefront',caret:[0,0],scroll:0,files:[],archived:false,sent:{text:'Start',root:'storefront',runtime:'claude',files:[]},request:{id:'pm-1',state:'launched',kind:'launched',url:'https://claude.ai/code/session_Invented456',next:'Reply here'},dialogue:{state:'idle',watch:'no watch',turn:null}}],navigation:{selected:'owner-1',list:'recent'}},
  ai:{...state.ai,draft:'',history:[],runtime:{available:true,name:'Claude Code',id:'claude'}},workspace_local:{}};
 delete s.page;
 // #1773 A6: the PM chat reads an invented transcript locally, so a snapshot never reaches a native session
 const log=[['user','What needs my attention today?'],['assistant','One design question needs your answer. Two tasks are held for later; current work continues while you decide.'],['user','Which changes are we keeping?'],['assistant','The compact menu stays the place to find tasks. Conversations is the place to read and answer; saved views, pins, files and task actions stay available.']].map(([role,text],at)=>({role,text,at,i:0}));
 // the native view freezes window.ipc, so the stub wraps the WebKit handler it calls instead
 const sink=window.webkit?.messageHandlers?.ipc||window.ipc,post=sink&&sink.postMessage.bind(sink);
 const stub=t=>{const m=JSON.parse(t);if(m.a!=='conversation-dialogue'||m.conversation!=='pm-1')return post&&post(t);
  setTimeout(()=>taskq.receive({dialogueReceipt:{id:m.id,conversation:m.conversation,action:m.action,state:'accepted',value:m.action==='status'?{state:'idle',watch:'no watch',turn:{state:'done',watch:false,outcome:'done'},last_line:log.at(-1).text}:{items:m.after!==undefined?[]:log,start:0,end:log.length,size:log.length}}}),0);};
 if(sink)sink.postMessage=stub;else window.ipc={postMessage:stub};
 taskq.receive(s);
 // the menu badge names the unread thread; the open question's task thread is selected and nothing is acknowledged
 taskq.receive({open:{page:'conversations',thread:'unread'}});
 const type=(el,text)=>{el.focus();el.value=text;el.dispatchEvent(new Event('input',{bubbles:true}));};
 setTimeout(()=>{
  // #1781: the badge screen shows the empty answer with its placeholder, even after an earlier screen typed a draft
  if(step==='badge'){const a=document.querySelector('[data-c-field=answer]');if(a&&a.value)type(a,'');}
  if(step==='question'){const a=document.querySelector('[data-c-field=answer]');if(a)type(a,'Keep the larger quantity. Retain unavailable items with a warning.');}
  if(step==='chat'){document.querySelector('[data-c=select][data-id=pm-1]')?.click();setTimeout(()=>{const t=document.querySelector('[data-c-field=text]');if(t)type(t,'What should we check before implementation?');},300);}
  if(step==='owner'){document.querySelector('[data-c=select][data-id=owner-1]')?.click();setTimeout(()=>{const t=document.querySelector('[data-c-field=text]');if(t){t.focus();t.setSelectionRange(t.value.length,t.value.length);}},50);}
 },50);
}
