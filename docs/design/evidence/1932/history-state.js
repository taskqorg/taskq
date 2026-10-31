// Native wrapper for the existing tests/conversations-render.browser.cjs history fixture.

const owner={id,revision:1,title:runtime+' · 10,000 fixture records',text:'',root:'fixture-alpha',caret:[0,0],files:[],archived:false,sent:{text:'Start',root:'fixture-alpha',runtime,files:[]},request:{id,state:'launched',kind:'launched'},dialogue:{state:'idle',watch:'no watch',turn:null}};
if(history1742.records.filter(r=>r.items.length).length!==10000)throw Error('fixture count');
taskq.receive({...state,projects:[{root:'fixture-alpha',name:'Alpha',alive:true,complete:true,issues:'https://example.org/alpha/issues',tasks:[]}],messages:{complete:true,entries:[]},conversations:{complete:true,items:[owner,{...owner,id:other,title:'Other disposable thread'}],navigation:{selected:id,list:'recent'}}});taskq.receive({open:{page:'conversations'}});
