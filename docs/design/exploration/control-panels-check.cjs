const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync(`${__dirname}/control-panels.html`, 'utf8');
const scripts = [...source.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(m => m[1]);
scripts.forEach(code => new vm.Script(code));
const bridge = source.slice(source.indexOf('let shared='), source.indexOf('const I='));
let snapshot;
const notices = [];
function jsonValue(value) {
  assert.notEqual(value, undefined);
  if (value && typeof value === 'object') Object.values(value).forEach(jsonValue);
}
const context = {window: {openai: {widgetState: {}, setWidgetState(value) {
  jsonValue(value);
  snapshot = JSON.parse(JSON.stringify(value));
  return Promise.resolve();
}}}, status: text => notices.push(text)};
vm.createContext(context);
vm.runInContext(bridge, context);
const store = context.window.tqControlStore;
store.save('chat', {selected: 'first', aiConversations: [{id: 'first', draft: 'Retain me'}]});
assert.equal(snapshot.privateContent.chat.aiConversations[0].draft, 'Retain me');
assert.equal(snapshot.modelContent.conversation, 'first');
store.save('menu', {selected: 'all', views: [{id: 'all', name: 'All tasks'}], trayView: 'icon'});
assert.equal(snapshot.privateContent.chat.selected, 'first');
store.save('chat', {selected: undefined, aiConversations: []});
assert.equal(snapshot.privateContent.menu.selected, 'all');
jsonValue(snapshot);
store.save('chat', {oversized: 'x'.repeat(15000)});
assert.deepEqual(snapshot.privateContent.chat.aiConversations, []);
assert.equal(notices.length, 1);
vm.runInContext(source.match(/^function chatTitle\([^\n]+/m)[0], context);
assert.equal(context.chatTitle({title: 'New conversation', draft: '\n Review filters\nKeep context'}), 'Review filters');
assert.equal(context.chatTitle({title: 'Named conversation', draft: 'A different draft'}), 'Named conversation');
console.log('PASS: syntax, JSON-safe first chat save, state isolation, empty selection, size refusal, draft identity');
