// Invented nine-project outage; shared by browser regression and native snapshots (#2351).
function taskqErrorsFixture(base,count=9){
 const now=200000,until='1970-01-03T08:00:00Z';
 const s={...base,page:'home',host:'fixture',now,font:16,theme:base.theme||'dark',settings_error:null,opacity_error:null,agentq:null,setup:null,
  service:{state:'running',fresh:true},ai:{history:[],runtime:{id:'codex',available:true}},notify:{inbox:{}},messages:{complete:true,entries:[]},conversations:{complete:true,items:[]},projects:[]};
 for(let i=1;i<=count;i++){
  const root='invented-'+i,issues='https://github.com/invented/project-'+i+'/issues';
  s.projects.push({root,name:'Project '+i,issues,board_kind:'github',host:'fixture',alive:true,complete:true,age:0,age_text:'0s',ttl:300,poll:60,tasks_age_seconds:0,tasks_live:true,observed:now,focus:[],labels:[],saved:true,mode:'watch',partial:[],counts:[],events:[],since:now-3600,reports:{complete:true,reports:[],history:[]},
   hosts:[{name:'fixture',live:true,updated:now,age:0,age_text:'0s'}],tasks:[{n:i,title:'Retained task '+i,key:issues+'/'+i,url:issues+'/'+i,state:'doing',state_name:'Doing',revision:1,attempt:i,host:'fixture',controls:[],executions:[{id:i,attempt:i,host:'fixture',runtime:'codex',role:'worker',state:'running',live:true,observed:now}]}]});
 }
 s.projects[0].tasks.push({n:41,title:'Keep this question in Needs me',key:s.projects[0].issues+'/41',state:'ask',state_name:'Ask',revision:4,controls:[],question:{parsed:true,problem:'Which quantity should a duplicate product keep?',choices:[{id:1,text:'Keep the larger quantity',recommended:true},{id:2,text:'Add the quantities'}]}});
 s.messages.entries=[{kind:'question',key:'invented-question',root:s.projects[0].root,project:'Project 1',board:s.projects[0].issues,task:41,revision:4,state:'ask',title:'Keep this question in Needs me',read:false}];
 const states=['held','ready','waiting','ask','doing','review','later','failed'];
 s.display={states:states.map(id=>({id,name:id})),all:{counts:states.map(state=>({state,name:state,n:state==='doing'?count:state==='ask'?1:0})),partial:[],open:count+1}};
 return {fresh:s,failed:{...structuredClone(s),now:now+900,messages:{...structuredClone(s.messages),complete:false,reason:s.projects.map(p=>p.name+': messages source stale, incomplete or unavailable').join('; ')},projects:s.projects.map(p=>({...structuredClone(p),complete:false,tasks_live:false,tasks_age_seconds:900,error:'GET '+p.issues+': HTTP 403 rate limit: board requests paused until '+until,reports:{complete:false,reports:[],history:[],reason:'rate limit: board requests paused until '+until},hosts:p.hosts.map(h=>({...h,live:false,age:900,age_text:'15m'})),tasks:p.tasks.map(t=>({...t,executions:t.executions?.map(x=>({...x,live:false}))}))}))}};
}
function taskqErrorsShot(surface,base){
 const f=taskqErrorsFixture(base);taskq.receive(f.fresh);taskq.receive(f.failed);
 if(document.querySelector('.workspace'))taskq.receive({open:{page:'conversations',filter:'all'}});
 setTimeout(()=>{const errors=document.querySelector('.errors-panel');if(errors){errors.open=true;const cause=errors.querySelector('.errors-list>li>details');if(cause)cause.open=true;}},100);
}
if(typeof module!=='undefined')module.exports=taskqErrorsFixture;
