// A1/A3: Home must open a saved exchange without replaying the request.
const assert=require('node:assert/strict');
const c={S:{projects:[],ai:{target:'p',history:[],request:{id:'2',root:'p',text:'Hello',answer:{state:'result',text:'Full answer'}}}},aiText:'',aiMenu:false};
require('./tray-harness.cjs').install(c);
assert.match(c.ai(),/data-a="ai-open"/);
console.log('PASS Home opens the saved exchange');
