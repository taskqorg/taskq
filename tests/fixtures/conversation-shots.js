// #1699 A8: native Conversations captures (`tray --snapshot --route conversations`) over the real pushed state with
// invented projects, a task question, an unread notification and an owner draft; nothing reaches a board or session.
function taskqConversationShot(step,state){
 const board='https://github.com/o/r/issues',q='1'.repeat(64),r='2'.repeat(64);
 const card='Done: compared both clients\nProblem: Should the new admin API keep compatibility with existing clients?\nEvidence: [#12](https://github.com/o/r/pull/12)\n1. Keep compatibility ★\n2. Remove the old API\nTo answer: `taskq answer 841.K` or comment `K` or the option text.';
 const row={n:841,title:'Keep the old API?',state:'ask',url:board+'/841',revision:4,card,question:{parsed:true,text:card,done:'compared both clients',problem:'Should the new admin API keep compatibility with existing clients?',evidence:[{label:'#12',url:'https://github.com/o/r/pull/12'}],
  choices:[{id:1,text:'Keep compatibility',recommended:true,consequential:false},{id:2,text:'Remove the old API',recommended:false,consequential:false}],limits:{lines:4,chars:240}}};
 const s={...state,host:'fixture-host',projects:[{root:'fixture',name:'Fixture',issues:board,alive:true,complete:true,age:1,tasks_age_seconds:1,focus:[],labels:[],tasks:[row,{n:900,title:'Plain running task',state:'doing',revision:1}]}],
  messages:{complete:true,reason:'',count:2,entries:[
   {key:q,kind:'question',title:row.title,project:'Fixture',root:'fixture',board,task:row.n,revision:row.revision,state:'ask',pending:true,read:false,archived:false},
   {key:r,kind:'request',title:'Setup request · launched',root:'fixture',host:'fixture-host',id:'native-live',state:'launched',pending:true,read:false,archived:false,request:{next:'Read replies in the native runtime.',url:'https://claude.ai/code/session_Invented123'}}]},
  conversations:{complete:true,items:[{id:'owner-1',revision:1,title:'Owner plan',text:'Plan the next release: list the open review items first, then the hosts that still run 4.6.39.',root:'fixture',caret:[0,0],scroll:0,files:[],archived:false}],navigation:{selected:'owner-1',list:'recent'}},
  ai:{...state.ai,draft:'',history:[],runtime:{available:true,name:'Claude Code',id:'claude'}},workspace_local:{}};
 delete s.page;
 taskq.receive(s);
 // the menu badge names the unread thread; the open question's task thread is selected and nothing is acknowledged
 taskq.receive({open:{page:'conversations',thread:'unread'}});
 const type=(el,text)=>{el.focus();el.value=text;el.dispatchEvent(new Event('input',{bubbles:true}));};
 setTimeout(()=>{
  if(step==='question'){const a=document.querySelector('[data-c-field=answer]');if(a)type(a,'Keep compatibility for one more release, then remove it.');}
  if(step==='owner'){document.querySelector('[data-c=select][data-id=owner-1]')?.click();setTimeout(()=>{const t=document.querySelector('[data-c-field=text]');if(t){t.focus();t.setSelectionRange(t.value.length,t.value.length);}},50);}
 },50);
}
