// Published task workspace, task detail and menu (docs/design/task-workspace.md): the tray and DashQ load this bundle unchanged.
'use strict';
const tray=require('./tray-app.js');
module.exports={schema:'taskq.task-workspace/1',createState:tray.createState,create:tray.create,
 answerCard:require('./answer-card.js'),projectBlocks:require('./project-blocks.js'),resources:require('./resource-controls.js')};
