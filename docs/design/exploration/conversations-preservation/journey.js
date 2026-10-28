// Review-copy projection only. The parent fixture owns every simulated transition.
'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),icons=require('./ui-icons.js');
module.exports=function(snapshot,conversation,task){
 const j=snapshot.review_journey;if(!j||!(conversation===j.conversation||task===j.task))return null;
 const send=action=>ui.send({a:'review-journey',action,conversation:j.conversation,task:j.task,generation:j.generation});
 const button=(action,glyph,label,primary=false)=>h('button',{class:'journey-action'+(primary?' primary-action':''),onClick:()=>send(action),'data-journey':action},icons.icon(glyph),h('span',null,label));
 if(task)return h('div',{class:'journey-return'},button('return','arrow-left','Back to PM'),h('span',{class:'meta'},'Your answer returns to this chat automatically.'));
 if(j.phase==='briefing')return h('p',{class:'journey-intro'},'Describe the task below. Your PM will prepare a short brief before you assign it.');
 if(j.phase==='planning')return h('p',{class:'journey-intro',role:'status'},'PM is preparing the task brief…');
 const info={held:['held','Ready to assign','PM prepared the brief. Work has not started.'],working:['doing','Agent is working','Worker is checking the existing menu and Conversations flow.'],ask:['ask','Needs your reply','Worker paused for one decision. Open the question to continue.'],continuing:['doing','Answer accepted · work continues','Worker is applying your decision to the same task.'],review:['review','Review in progress','A separate reviewer is checking the draft and retained functions.'],complete:['complete','Result ready','The review finished. Read the result below.'],failed:['failed','Work stopped','The demo worker exited with an error. Its exit is confirmed; your input is kept.']}[j.phase];
 if(!info)return null;
 const [state,title,description]=info;
 return h('section',{class:'journey-card','aria-label':'Assigned task','data-phase':j.phase},
  h('div',{class:'journey-heading'},h('strong',null,'#'+j.task+' · '+j.title),h('span',{class:'journey-state','data-state':state},icons.icon({held:'pause',doing:'play',ask:'message-circle-question',review:'git-pull-request',complete:'check-check',failed:'circle-x'}[state]),title)),
  h('p',{class:'journey-description',role:'status'},description),
  h('p',{class:'meta'},(j.phase==='review'?'Reviewer · ':['held','complete'].includes(j.phase)?'Project manager · ':'Worker · ')+j.runtime+' · local simulation'),
  j.phase==='held'&&h('p',{class:'journey-brief'},'Compact menu, clear PM dialogue, questions and results. Keep every existing function. Deliver an interactive design draft; implementation stays held.'),
  j.answer&&h('p',{class:'journey-decision'},'Your answer: '+j.answer),
  h('div',{class:'journey-actions'},j.phase==='held'&&[button('assign','play','1 · Assign task ★',true),!j.parked&&button('hold','pause','2 · Keep for later')],j.phase==='ask'&&button('question','message-circle-question','Answer question',true),j.phase==='failed'&&button('retry','rotate-ccw','Retry this task',true)),
  ['working','continuing','review'].includes(j.phase)&&h('p',{class:'journey-wait'},'You can keep using the chat. Progress and questions appear here.'),
  j.phase==='complete'&&h('div',{class:'journey-result'},h('h3',null,'Interactive draft ready'),h('ul',null,h('li',null,'Menu opens the exact question without trapping list scroll.'),h('li',null,'PM → task → clarification → result stays connected.'),h('li',null,'All 72 function groups remain in scope.')),h('p',null,'Demo review: flow and retained controls checked. Native delivery, performance and implementation remain untested.'),h('p',{class:'meta'},'Next: 1 · Review this draft ★ · 2 · Request changes in this chat.')),
  h('details',{class:'journey-history','data-view-key':'journey-history'},h('summary',null,'Task activity'),h('ol',null,j.events.map((e,i)=>h('li',{key:i},e)))));
};
