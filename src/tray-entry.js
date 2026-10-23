// The tray mounts the published task workspace bundle (TaskqTaskWorkspace, loaded first) and keeps its public globals.
const w=globalThis.TaskqTaskWorkspace;
globalThis.TaskqAnswerCard=w.answerCard;
globalThis.TaskqProjectBlocks=w.projectBlocks;
globalThis.TaskqResources=w.resources;
// R22: the Settings window loads this page as its own surface.
w.create(w.createState(),{settings:globalThis.taskqSurface==='settings'}).mount();
