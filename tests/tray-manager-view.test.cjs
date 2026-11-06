// Chain: R6/M3.7 tray Home shows the service's manager view from status.json `report`. Risk: anomaly rows hidden from the tray.
'use strict';
const assert=require('node:assert/strict');
const c={S:{projects:[]}};
require('./tray-harness.cjs').install(c);
assert.equal(c.managerView({name:'p'}),'','no report, no block');
const report='Owner (answer N.M):\n| Task | Question | Options | Next |\n\nAnomalies:\n- host win [invented #2](https://example.invalid/2) · STALLED 13m: ready 2 without a spawn · Next: inspect\n  Task: runtime codex\n\nRunning:\n| Task |';
const html=c.managerView({root:'/invented',name:'invented',report});
assert.match(html,/invented: manager view · 1 anomaly/);
assert.match(html,/incident-attention">host win invented #2 · STALLED 13m: ready 2 without a spawn · Next: inspect</);
assert.match(html,/manager-report[^>]*>Owner \(answer N\.M\)/);
assert.doesNotMatch(html,/example\.invalid/,'markdown link targets fold to labels');
c.S.projects=[{root:'/invented',name:'invented',alive:true,report,tasks:[]}];
assert.match(c.attention(),/manager view · 1 anomaly/,'Attention lists the manager view');
console.log('PASS tray manager view: report rows and anomalies in Attention');
