import Cocoa
import ImageIO
var result:[[String:Any]]=[]
for path in CommandLine.arguments.dropFirst() {
 let source=CGImageSourceCreateWithURL(URL(fileURLWithPath:path) as CFURL,nil)!
 let image=CGImageSourceCreateImageAtIndex(source,0,nil)!
 let w=image.width,h=image.height
 var bytes=[UInt8](repeating:0,count:w*h*4)
 bytes.withUnsafeMutableBytes { buffer in
  let context=CGContext(data:buffer.baseAddress,width:w,height:h,bitsPerComponent:8,bytesPerRow:w*4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue)!
  context.draw(image,in:CGRect(x:0,y:0,width:w,height:h))
 }
 var counts=[Int](repeating:0,count:256)
 for i in stride(from:3,to:bytes.count,by:4){counts[Int(bytes[i])]+=1}
 assert(counts.reduce(0,+)==w*h)
 result.append(["file":URL(fileURLWithPath:path).lastPathComponent,"width":w,"height":h,"alpha_histogram":counts,"transparent":counts[0],"opaque":counts[255]])
}
let data=try! JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys])
FileHandle.standardOutput.write(data)
