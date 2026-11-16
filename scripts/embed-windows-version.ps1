param([Parameter(Mandatory=$true)][string]$Executable,[Parameter(Mandatory=$true)][string]$Version,[string]$Description='Reqon',[string]$OriginalFilename='Reqon.exe')
$ErrorActionPreference='Stop'
if (-not ('ReqonPackaging.VersionResource' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace ReqonPackaging {
 public static class VersionResource {
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr BeginUpdateResourceW(string path,bool delete);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateResourceW(IntPtr handle,IntPtr type,IntPtr name,ushort language,byte[] data,uint length);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool EndUpdateResourceW(IntPtr handle,bool discard);
  static byte[] Block(string name,ushort kind,byte[] value,ushort valueLength,params byte[][] children) {
   using(var stream=new MemoryStream()) using(var writer=new BinaryWriter(stream)) {
    writer.Write((ushort)0);writer.Write(valueLength);writer.Write(kind);writer.Write(Encoding.Unicode.GetBytes(name+"\0"));
    while(stream.Position%4!=0)writer.Write((byte)0);
    writer.Write(value);
    foreach(var child in children){while(stream.Position%4!=0)writer.Write((byte)0);writer.Write(child);}
    var bytes=stream.ToArray();Array.Copy(BitConverter.GetBytes((ushort)bytes.Length),bytes,2);return bytes;
   }
  }
  static byte[] Text(string key,string value){return Block(key,1,Encoding.Unicode.GetBytes(value+"\0"),(ushort)(value.Length+1));}
  public static void Write(string path,string version,string description,string originalFilename) {
   var v=Version.Parse(version);uint a=(uint)((v.Major<<16)|v.Minor),b=(uint)(Math.Max(0,v.Build)<<16);
   byte[] fixedInfo;
   using(var ms=new MemoryStream())using(var w=new BinaryWriter(ms)){
    foreach(uint n in new uint[]{0xFEEF04BD,0x10000,a,b,a,b,0x3F,0,0x40004,1,0,0,0})w.Write(n);fixedInfo=ms.ToArray();
   }
   var strings=Block("StringFileInfo",1,new byte[0],0,Block("040904B0",1,new byte[0],0,
    Text("CompanyName","Reqon"),Text("FileDescription",description),Text("ProductName","Reqon"),
    Text("InternalName",Path.GetFileNameWithoutExtension(originalFilename)),Text("OriginalFilename",originalFilename),Text("FileVersion",version+".0"),Text("ProductVersion",version)));
   var vars=Block("VarFileInfo",1,new byte[0],0,Block("Translation",0,new byte[]{9,4,176,4},4));
   var resource=Block("VS_VERSION_INFO",0,fixedInfo,(ushort)fixedInfo.Length,strings,vars);
   var h=BeginUpdateResourceW(path,false);if(h==IntPtr.Zero)throw new Win32Exception();
   try{if(!UpdateResourceW(h,new IntPtr(16),new IntPtr(1),0x0409,resource,(uint)resource.Length))throw new Win32Exception();
    var handle=h;h=IntPtr.Zero;if(!EndUpdateResourceW(handle,false))throw new Win32Exception();
   }finally{if(h!=IntPtr.Zero)EndUpdateResourceW(h,true);}
  }
 }
}
'@
}
[ReqonPackaging.VersionResource]::Write((Resolve-Path -LiteralPath $Executable).Path,$Version,$Description,$OriginalFilename)
