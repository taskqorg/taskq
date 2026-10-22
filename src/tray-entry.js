// Preserve the public component APIs exposed by the original tray page.
Object.defineProperty(globalThis,'TaskqAnswerCard',{get:()=>require('./answer-card.js')});
globalThis.TaskqProjectBlocks=require('./project-blocks.js');
Object.defineProperty(globalThis,'TaskqResources',{get:()=>require('./resource-controls.js')});
const tray=require('./tray-app.js');
tray.create(tray.createState(),{}).mount();
