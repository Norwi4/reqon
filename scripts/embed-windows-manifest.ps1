param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
if (-not ('ReqonPackaging.ManifestResource' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
namespace ReqonPackaging {
 public static class ManifestResource {
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr BeginUpdateResourceW(string file,bool delete);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateResourceW(IntPtr handle,IntPtr type,IntPtr name,ushort language,byte[] data,uint size);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool EndUpdateResourceW(IntPtr handle,bool discard);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr LoadLibraryExW(string file,IntPtr handle,uint flags);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr FindResourceW(IntPtr module,IntPtr name,IntPtr type);
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr LoadResource(IntPtr module,IntPtr resource);
  [DllImport("kernel32.dll",SetLastError=true)] static extern uint SizeofResource(IntPtr module,IntPtr resource);
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr LockResource(IntPtr resource);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool FreeLibrary(IntPtr module);
  public static void Write(string path,byte[] data) {
   var handle=BeginUpdateResourceW(path,false);if(handle==IntPtr.Zero)throw new Win32Exception();
   try {
    // RT_MANIFEST, CREATEPROCESS_MANIFEST_RESOURCE_ID, neutral language.
    if(!UpdateResourceW(handle,new IntPtr(24),new IntPtr(1),0,data,(uint)data.Length))throw new Win32Exception();
    var commit=handle;handle=IntPtr.Zero;if(!EndUpdateResourceW(commit,false))throw new Win32Exception();
   }finally{if(handle!=IntPtr.Zero)EndUpdateResourceW(handle,true);}
  }
  public static string Read(string path) {
   // Map resources only. Reading the packaged EXE must never run it.
   var module=LoadLibraryExW(path,IntPtr.Zero,0x22);if(module==IntPtr.Zero)throw new Win32Exception();
   try {
    var resource=FindResourceW(module,new IntPtr(1),new IntPtr(24));if(resource==IntPtr.Zero)throw new Win32Exception();
    var size=SizeofResource(module,resource);if(size==0 || size>65536)throw new InvalidOperationException("Invalid manifest size");
    var loaded=LoadResource(module,resource);if(loaded==IntPtr.Zero)throw new Win32Exception();
    var address=LockResource(loaded);if(address==IntPtr.Zero)throw new Win32Exception();
    var bytes=new byte[size];Marshal.Copy(address,bytes,0,(int)size);return Encoding.UTF8.GetString(bytes);
   }finally{FreeLibrary(module);}
  }
 }
}
'@
}
$reqonExe=(Resolve-Path -LiteralPath $Executable).Path
$reqonManifest=Join-Path (Split-Path $PSScriptRoot -Parent) 'packaging\reqon.manifest'
[ReqonPackaging.ManifestResource]::Write($reqonExe,[IO.File]::ReadAllBytes($reqonManifest))
[xml]$reqonEmbedded=[ReqonPackaging.ManifestResource]::Read($reqonExe)
$reqonLevel=$reqonEmbedded.SelectSingleNode('//*[local-name()="requestedExecutionLevel"]')
if(-not $reqonLevel -or $reqonLevel.level -ne 'asInvoker' -or $reqonLevel.uiAccess -ne 'false') {
    throw 'The packaged executable must explicitly run as the current user.'
}
