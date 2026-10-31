import AppKit
import WebKit
// #1932: production renderer instrument only. No native IPC, provider, board, service or persistent web store.
final class Probe: NSObject, WKNavigationDelegate {
 let web: WKWebView, window: NSWindow, root: String, out: String
 var mode = "question", frames = 0, recording = false
 init(_ root: String, _ out: String) throws {
  self.root=root; self.out=out
  let config=WKWebViewConfiguration(); config.websiteDataStore = .nonPersistent()
  web=WKWebView(frame:NSRect(x:0,y:0,width:800,height:600),configuration:config)
  window=NSWindow(contentRect:NSRect(x:80,y:100,width:800,height:600),styleMask:[.titled,.closable,.miniaturizable,.resizable],backing:.buffered,defer:false)
  super.init(); try FileManager.default.createDirectory(atPath:out,withIntermediateDirectories:true)
  window.title="TaskQ 1932 · disposable renderer · no live IPC"; window.contentView=web
  web.navigationDelegate=self; window.makeKeyAndOrderFront(nil)
  let menu=NSMenu(), item=NSMenuItem();menu.addItem(item);let controls=NSMenu();item.submenu=controls
  for (title,selector,key) in [("Question dark 20",#selector(question),"1"),("Question light 20",#selector(light),"2"),("History dark 16",#selector(history),"3"),("History light 20",#selector(historyLight),"4"),("Measure / snapshot",#selector(measure),"m"),("Start / stop recording",#selector(record),"r"),("Quit instrument",#selector(quit),"q")] {
   let entry=NSMenuItem(title:title,action:selector,keyEquivalent:key);entry.target=self;controls.addItem(entry)
  }
  let editItem=NSMenuItem();menu.addItem(editItem);let edit=NSMenu(title:"Edit");editItem.submenu=edit
  for (title,action,key) in [("Undo","undo:","z"),("Cut","cut:","x"),("Copy","copy:","c"),("Paste","paste:","v"),("Select All","selectAll:","a")] {edit.addItem(NSMenuItem(title:title,action:NSSelectorFromString(action),keyEquivalent:key))}
  NSApp.mainMenu=menu
  let screen=NSScreen.main!
  let env:[String:Any]=["os":ProcessInfo.processInfo.operatingSystemVersionString,"webkit":Bundle(identifier:"com.apple.WebKit")?.object(forInfoDictionaryKey:"CFBundleVersion") ?? "unknown","screen":NSStringFromRect(screen.frame),"workArea":NSStringFromRect(screen.visibleFrame),"scale":screen.backingScaleFactor,"systemFont":NSFont.systemFont(ofSize:20).fontName]
  try JSONSerialization.data(withJSONObject:env,options:[.prettyPrinted,.sortedKeys]).write(to:URL(fileURLWithPath:out+"/environment.json"))
  web.loadHTMLString(try String(contentsOfFile:root+"/build/web/workspace.html",encoding:.utf8),baseURL:nil)
  Timer.scheduledTimer(withTimeInterval:0.5,repeats:true){[weak self] _ in if self?.recording == true {self?.snapshot()}}
 }
 func webView(_ webView:WKWebView,didFinish navigation:WKNavigation!){question()}
 func load(_ scene:String,_ theme:String,_ font:Int){
  mode="\(scene)-\(theme)-\(font)"
  let fixture=try! String(contentsOfFile:root+"/tests/fixtures/conversation-shots.js",encoding:.utf8)
  let probe=try! String(contentsOfFile:root+"/docs/design/evidence/1932/interaction.js",encoding:.utf8)
  let palette=theme=="dark" ? "--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--new:#6EA8FF;--review:#BCA3EB" : "--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--new:#1F65C5;--review:#8250DF"
  let state="{font:\(font),theme:'\(theme)',ai:{history:[],runtime:{id:'claude',available:true}},palette:'\(palette)',platform:'macos',version:'1932 renderer'}"
  let script:String
  if scene=="chat" {
   // Reuse the deterministic 10,000-record fixture from the existing browser instrument.
   let source=try! String(contentsOfFile:root+"/tests/conversations-render.browser.cjs",encoding:.utf8)
   let body=source.components(separatedBy:"await page.evaluate(({runtime,id,other})=>{")[1].components(separatedBy:"},{runtime,id,other});")[0]
    .replacingOccurrences(of:"new PerformanceObserver(list=>{for(const e of list.getEntries())h.longtasks.push(e.duration);}).observe({entryTypes:['longtask']});",with:"window.longTaskApi1932=PerformanceObserver.supportedEntryTypes.includes('longtask');")
   let setup=try! String(contentsOfFile:root+"/docs/design/evidence/1932/history-state.js",encoding:.utf8)
   script="(()=>{const state="+state+";const runtime='claude',id='claude-history',other='claude-other';"+body+setup+"})();"+probe
  } else {script="window.ipc={postMessage:()=>{}};"+fixture+";taskqConversationShot('\(scene)',"+state+");"+probe}
  web.evaluateJavaScript(script){_,error in
   if let error=error {self.append("error",String(data:try! JSONSerialization.data(withJSONObject:["message":String(describing:error)]),encoding:.utf8)!)}
  }
 }
 @objc func question(){load("question","dark",20)}
 @objc func light(){load("question","light",20)}
 @objc func history(){load("chat","dark",16)}
 @objc func historyLight(){load("chat","light",20)}
 func append(_ kind:String,_ value:String){
  let line="{\"at\":\(Date().timeIntervalSince1970),\"scene\":\"\(mode)\",\"kind\":\"\(kind)\",\"value\":\(value)}\n"
  let path=out+"/observations.jsonl";if !FileManager.default.fileExists(atPath:path){FileManager.default.createFile(atPath:path,contents:nil)}
  if let handle=FileHandle(forWritingAtPath:path){handle.seekToEndOfFile();handle.write(Data(line.utf8));try? handle.close()}
 }
 @objc func measure(){web.evaluateJavaScript("JSON.stringify(window.probe1932.snapshot())"){value,error in
  if let text=value as? String {self.append("measurement",text);print(text)}
  if let error=error {print(error)}
 };snapshot()}
 func snapshot(){let config=WKSnapshotConfiguration();config.rect=web.bounds
  web.takeSnapshot(with:config){image,error in
   guard let image=image,let tiff=image.tiffRepresentation,let bitmap=NSBitmapImageRep(data:tiff),let png=bitmap.representation(using:.png,properties:[:]) else{return}
   self.frames+=1;try? png.write(to:URL(fileURLWithPath:self.out+String(format:"/frame-%05d.png",self.frames)))
  }
 }
 @objc func record(){recording.toggle();append("recording",recording ? "true":"false");measure()}
 @objc func quit(){recording=false;measure();DispatchQueue.main.asyncAfter(deadline:.now()+0.5){NSApp.terminate(nil)}}
}
NSApplication.shared.setActivationPolicy(.regular)
let root=CommandLine.arguments.count==3 ? CommandLine.arguments[1] : Bundle.main.object(forInfoDictionaryKey:"ProbeRoot") as! String
let out=CommandLine.arguments.count==3 ? CommandLine.arguments[2] : Bundle.main.object(forInfoDictionaryKey:"ProbeOut") as! String
let probe=try Probe(root,out);NSApp.activate(ignoringOtherApps:true);NSApp.run()
