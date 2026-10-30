// Chain: Product / RC2–RC7 -> shared card controller -> result adapter -> source-bound receipts.
// Risk: closed results cannot use the shared choices, a reason silently submits an answer, or read hides decisions.
const assert=require('node:assert/strict'),card=require('../src/answer-card.js'),{create}=require('../src/reports.js');
const row={n:12,state:'closed',revision:'a:decision',resultDecision:true,title:'Invented report',url:'https://example.invalid/12',question:{parsed:true,choices:[{id:1,label:'Accept',text:'Accept',recommended:true}],evidence:[]}};
const chosen=card.step(row,card.fresh(row),{answer:true,composer:true},{type:'choose',id:1},()=> 'fixed-op');
assert.equal(chosen.send?.choice,1,'a typed result uses shared choices while the source stays closed');
assert.equal(card.step(row,chosen.local,{answer:true},{type:'choose',id:1},()=> 'duplicate').send,null,'sending disables duplicate choices');
assert.equal(card.status({...row,revision:'new-sha:decision',repeatable:true},{...chosen.local,receipt:{state:'accepted'}},{answer:true}),'stale','an old accepted result never unlocks a newer source');
const correction={...row,revision:'a:correction',question:{...row.question,resultReason:true,choices:[{id:1,label:'Reopen',text:'Reopen',reason:'reopen'}]}};
const intent=card.step(correction,card.fresh(correction),{answer:true,composer:true},{type:'choose',id:1},()=> 'unused');
assert.equal(intent.send,null,'selecting a correction only opens reason entry');assert.equal(intent.local.intent,'reopen');
assert(card.textProblem(correction.question,'two\nlines'));assert(card.textProblem(correction.question,'x'.repeat(181)));
const keep={...row,question:{...row.question,choices:[{id:1,label:'Keep report',text:'Keep report',local:true}]}};
assert.equal(card.step(keep,card.fresh(keep),{answer:true},{type:'choose',id:1},()=> 'unused').send,null,'Keep report never claims a board answer');
const sent=[],v=create({send:m=>sent.push(m),draw(){},defer(){},id:()=>String(sent.length+1)}),sha='a'.repeat(40);
const report={n:12,sha,title:'Invented report',kind:'report',closed:40,read:true,decision:'none',visible:true,body:'Report'};
const projects=[{root:'fixture',name:'Alpha',issues:'https://example.invalid/issues',reports:{complete:true,reports:[report],history:[{...report,n:13,decision:'drop',visible:false}]}}];
v.sync(projects);assert.equal(v.threads().length,2,'Finished retains decided history');assert.equal(v.threads().find(r=>r.report===12).needsDecision,true,'reading leaves a report undecided');assert.equal(v.unread().length,0);
v.open('fixture',13);assert.equal(v.current().n,13,'decided history opens locally');assert.equal(sent.length,0);
const k='result:https://example.invalid/issues#12@'+sha+':decision';v.hydrate({[k]:{draft:'',bound:sha+':decision',op:{id:'original',task:12,revision:sha+':decision',choice:1},request:{id:'original',root:'fixture',n:12,sha,action:'accept'},receipt:{state:'sending'}}});
assert.equal(sent.length,0,'restoring unknown delivery never replays it');assert.equal(v.capability({...report,root:'fixture'},'correction').answer,false,'restored unknown blocks conflicting actions');
const wrong=create({send:m=>sent.push(m),draw(){},defer(){},id:()=> 'wrong'});wrong.sync(projects);wrong.open('fixture',12);wrong.act('accept');wrong.receive({id:'wrong',root:'fixture',report});assert.match(wrong.feedback(),/not confirmed/,'a matching source alone does not confirm the requested action');
const late=create({send:m=>sent.push(m),draw(){},defer(){},id:()=> 'late-successor'});late.sync([{...projects[0],reports:{complete:true,reports:[{...report,comments_through:10}]}}]);late.open('fixture',12);late.act('followup','More work');late.sync([{...projects[0],reports:{complete:true,reports:[],history:[{...report,decision:'accepted',visible:false,comments_through:20}]}}]);late.receive({id:'late-successor',root:'fixture',report:{...report,comments_through:10},successor:42});assert.equal(late.current().decision,'accepted','late successor cannot overwrite newer collected acceptance');assert.equal(late.current().comments_through,20);assert.match(late.feedback(),/Created held draft/);assert.equal(late.threads()[0].needsDecision,false);
assert.equal(late.open('fixture',12,'b'.repeat(40)),false,'an exact old-SHA deep link never substitutes another result');
console.log('PASS result cards: closed-source choices, intent then validated reason, duplicate guard, decided history and independent attention');
