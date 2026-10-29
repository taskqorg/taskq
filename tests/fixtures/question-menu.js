// Snapshot-only (#1781, menu.md): the Tasks menu over invented tasks with one open question, as in the approved
// input-scroll 04 and 05 renders. `phase`: menu (the ask row with its question glyph) or details (that row's details).
// Merged into the real pushed state (palette, font, theme); the filter chip counts stay the install's. No board operation: nothing is clicked that dispatches. A failed check rejects, which faults the snapshot (exit 1).
window.taskqQuestionMenu=async(phase,state)=>{
  const q=s=>document.querySelector(s),check=(ok,what)=>{if(!ok)throw new Error(what);};
  const board='https://github.com/o/website/issues',root='website';
  const card='Done: compared both clients\nProblem: Should the new admin API keep compatibility with existing clients?\nEvidence: [#12](https://github.com/o/website/pull/12)\n1. Keep compatibility ★\n2. Remove the old API';
  const t=(n,state,title,age,controls,extra={})=>({n,state,state_name:state[0].toUpperCase()+state.slice(1),title,revision:1,url:board+'/'+n,key:board+'/'+n,root,age,controls,runtime:'claude',...extra});
  const tasks=[t(7,'doing','Prepare the release notes','1m',['stop','later']),
    t(9,'ask','Choose the compatibility approach','2h',['stop','later'],{card,question:{parsed:true,text:card,done:'compared both clients',problem:'Should the new admin API keep compatibility with existing clients?',evidence:[{label:'#12',url:'https://github.com/o/website/pull/12'}],
      choices:[{id:1,text:'Keep compatibility',recommended:true,consequential:false},{id:2,text:'Remove the old API',recommended:false,consequential:false}],limits:{lines:4,chars:240}}}),
    t(8,'ready','Check the payment flow','<1m',['start','later']),t(3,'ready','Verify the mobile layout','5m',['start','later']),t(5,'ready','Review translations','2h',['start','later']),t(4,'later','Revisit onboarding','10m',['retry'])];
  const s={...state,page:'home',projects:[{root,name:'Website',issues:board,board_kind:'github',alive:true,complete:true,age:5,focus:['all'],labels:[],closed:3,tasks}]};
  taskq.receive(s);
  await new Promise(r=>setTimeout(r,50));
  // the previous shot may leave the details open
  q('#taskq-question-details [data-a=popup-close]')?.click();await new Promise(r=>setTimeout(r,50));
  const glyph=q('.question-shortcut[data-task="9"]');
  check(glyph,'the ask row shows its question glyph');
  if(phase==='details'){q('[data-a="task-detail"][data-key="'+board+'/9"]')?.click();await new Promise(r=>setTimeout(r,50));check(q('#taskq-question-details'),'the ask row opens its details inline');}
};
