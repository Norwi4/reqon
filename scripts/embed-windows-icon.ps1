param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Icon
)
$ErrorActionPreference = 'Stop'
# Windows' resource API embeds the ICO without an SDK/resource compiler or
# extra build dependencies. All images and their group live inside the EXE.
if (-not ('ReqonPackaging.IconResources' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;

namespace ReqonPackaging {
    public static class IconResources {
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        static extern IntPtr BeginUpdateResourceW(string file, bool deleteExisting);
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        static extern bool UpdateResourceW(IntPtr handle, IntPtr type, IntPtr name,
            ushort language, byte[] data, uint size);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool EndUpdateResourceW(IntPtr handle, bool discard);

        public static void Write(string executable, byte[] ico) {
            if (ico.Length < 6 || BitConverter.ToUInt16(ico, 0) != 0 ||
                BitConverter.ToUInt16(ico, 2) != 1)
                throw new InvalidDataException("Expected a Windows ICO file.");
            int count = BitConverter.ToUInt16(ico, 4);
            if (count == 0 || count > 256 || ico.Length < 6 + count * 16)
                throw new InvalidDataException("Invalid ICO directory.");
            var frames = new byte[count][];
            var group = new byte[6 + count * 14];
            Buffer.BlockCopy(ico, 0, group, 0, 6);
            for (int i = 0; i < count; i++) {
                int entry = 6 + i * 16;
                uint length = BitConverter.ToUInt32(ico, entry + 8);
                uint offset = BitConverter.ToUInt32(ico, entry + 12);
                if (length == 0 || (ulong)offset + length > (ulong)ico.Length)
                    throw new InvalidDataException("ICO image exceeds its file.");
                frames[i] = new byte[length];
                Buffer.BlockCopy(ico, (int)offset, frames[i], 0, (int)length);
                Buffer.BlockCopy(ico, entry, group, 6 + i * 14, 12);
                Buffer.BlockCopy(BitConverter.GetBytes((ushort)(101 + i)), 0,
                    group, 6 + i * 14 + 12, 2);
            }
            IntPtr handle = BeginUpdateResourceW(executable, false);
            if (handle == IntPtr.Zero) throw new Win32Exception();
            try {
                for (int i = 0; i < count; i++) {
                    if (!UpdateResourceW(handle, new IntPtr(3), new IntPtr(101 + i),
                        0, frames[i], (uint)frames[i].Length))
                        throw new Win32Exception();
                }
                if (!UpdateResourceW(handle, new IntPtr(14), new IntPtr(1),
                    0, group, (uint)group.Length))
                    throw new Win32Exception();
                var commitHandle = handle;
                handle = IntPtr.Zero;
                if (!EndUpdateResourceW(commitHandle, false)) throw new Win32Exception();
            } finally {
                if (handle != IntPtr.Zero) EndUpdateResourceW(handle, true);
            }
        }
    }
}
'@
}
$exePath = (Resolve-Path -LiteralPath $Executable).Path
$icoPath = (Resolve-Path -LiteralPath $Icon).Path
[ReqonPackaging.IconResources]::Write($exePath, [System.IO.File]::ReadAllBytes($icoPath))
