// Import the actual controller; use its explicit state and view ports.
const tray=require('../src/tray-app.js');
// Test-only markup of a vnode tree, so assertions read text; the apps render vnodes with Preact.
function escape(value){return String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));}
function html(v){
 if(v==null||typeof v==='boolean')return '';
 if(Array.isArray(v))return v.map(html).join('');
 if(typeof v!=='object')return escape(v);
 if(typeof v.type==='function')return html(v.type(v.props));
 const {children,dangerouslySetInnerHTML:raw,key,ref,...attributes}=v.props;
 const open='<'+v.type+Object.entries(attributes).map(([k,x])=>x==null||x===false?'':x===true?' '+k:' '+k+'="'+escape(x)+'"').join('')+'>';
 return ['input','img','br'].includes(v.type)?open:open+(raw?raw.__html:html(children))+'</'+v.type+'>';
}
function install(context){
 const state=tray.createState();
 for(const key of Object.keys(state)){
  if(Object.hasOwn(context,key))state[key]=context[key];
  Object.defineProperty(context,key,{configurable:true,enumerable:true,get:()=>state[key],set:value=>state[key]=value});
 }
 context.document??={scrollingElement:{scrollTop:0},activeElement:null,fonts:{ready:Promise.resolve()},addEventListener(){}};
 context.CSS??={escape:String};
 context.menu??={querySelector:()=>null,querySelectorAll:()=>[]};
 const app=tray.create(state,{workspace:context.operationController===true,window:context,document:context.document,menu:context.menu,send:m=>context.send?.(m),
  render:(...args)=>context.render?.(...args),measureScrollbar:context.gutter,
  timers:{defer:(f,ms)=>context.setTimeout?.(f,ms),cancel:t=>context.clearTimeout?.(t)}});
 // Pages are vnodes; assertions read their markup.
 for(const [key,value] of Object.entries(app))context[key]=typeof value==='function'?(...args)=>{const r=value(...args);return r&&typeof r==='object'&&'props' in r?html(r):r;}:value;
 return app;
}
module.exports={install,html};
