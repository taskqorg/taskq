// #1930 A2–A4: reject ambiguous/normalized dates; units never come from magnitude or transcript cursors.
'use strict';
const assert=require('node:assert/strict'),time=require('../src/relative-time.js');
const now=Date.parse('2026-10-31T12:00:00Z');
for(const bad of [null,undefined,'',0,-1,NaN,Infinity,{},'1793448000000','2026-10-31','2026-10-31T12:00:00','2026-02-30T12:00:00Z','2026-10-31T24:00:00Z','2026-10-31T12:00:00+25:00'])assert.equal(time.value(bad),null,String(bad));
assert.equal(time.value('2026-10-31T19:00:00+07:00'),now);
assert.equal(time.value(now),now);assert.equal(time.seconds(now/1000),now);assert.equal(time.seconds(String(now/1000)),null);
for(const [delta,want]of [[0,'<1m'],[-180000,'3m'],[-7200000,'2h'],[-345600000,'4d'],[-60000,'1m'],[-3600000,'1h'],[-86399000,'23h'],[-86400000,'1d'],[86400000,'in 1d'],[60000,'in 1m'],[1000,'in <1m']])assert.equal(time.label(now+delta,now),want);
const unknown=time.stamp(null,'Tool','Native record has no timestamp');assert.match(unknown.props['aria-label'],/Tool.*unknown.*Native record/);
const known=time.stamp('2026-10-31T19:00:00+07:00','Message');assert.equal(known.props['data-time-source'],'2026-10-31T19:00:00+07:00');assert.match(known.props.title,/2026.*\d\d:\d\d:\d\d.*GMT/);
assert(known.props.title.endsWith(' · '+Intl.DateTimeFormat().resolvedOptions().timeZone));
const zone=process.env.TZ;try{process.env.TZ='UTC';assert.match(time.stamp(now,'Message').props.title,/12:00:00 GMT.*UTC/);process.env.TZ='Asia/Bangkok';assert.match(time.stamp(now,'Message').props.title,/19:00:00 GMT\+07:00.*Asia\/Bangkok/);}finally{if(zone===undefined)delete process.env.TZ;else process.env.TZ=zone;}
console.log('PASS #1930 relative formatting, explicit units, malformed dates and source attribution');
