// The tray mounts the published task workspace bundle (TaskqTaskWorkspace, loaded first) and keeps its public globals.
const w=globalThis.TaskqTaskWorkspace;
globalThis.TaskqAnswerCard=w.answerCard;
globalThis.TaskqProjectBlocks=w.projectBlocks;
globalThis.TaskqResources=w.resources;
w.create(w.createState(),{}).mount();
