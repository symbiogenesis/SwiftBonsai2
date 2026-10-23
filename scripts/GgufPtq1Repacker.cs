#nullable enable
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace SwiftBonsai {
/// <summary>Experimental bounded export of exact PQ2 symbols/scales into Prism PTQ1 GGUF.</summary>
public sealed partial class GgufInspector {
    // Shares the inspector's validated descriptor parser, write-denying source
    // handle, limits and pinned dialect. Only serialization changes. The native
    // PTQ1 backend uses different activation quantization, so this export is NOT
    // an inference-equivalence or performance qualification.
    private static ulong Align(ulong value, uint alignment) { return checked((value+alignment-1)/alignment*alignment); }
    private void CopyTo(Stream output, long start, long bytes, Stopwatch clock, int timeoutSeconds) {
        ArgumentNullException.ThrowIfNull(output);
        ArgumentNullException.ThrowIfNull(clock);
        file.Position=start;
        while(bytes>0) {
            if(clock.Elapsed.TotalSeconds>timeoutSeconds) throw new TimeoutException("PTQ1 export exceeded its deadline.");
            int count=(int)Math.Min(bytes,hashBuffer.Length);
            file.ReadExactly(hashBuffer.AsSpan(0,count));
            output.Write(hashBuffer,0,count);
            bytes-=count;
        }
    }
    private static void PadTo(Stream output, long position, byte[] zeros) {
        ArgumentNullException.ThrowIfNull(output);
        ArgumentNullException.ThrowIfNull(zeros);
        if(output.Position>position) throw Bad("export tensor extents overlap.");
        while(output.Position<position) output.Write(zeros,0,(int)Math.Min(zeros.Length,position-output.Position));
    }
    private Dictionary<string,object> ExportPtq1(string outputPath, Dictionary<string,object> report, int timeoutSeconds) {
        ArgumentNullException.ThrowIfNull(report);
        if(Lookup("general.file_type") is not uint fileType || (fileType!=141 && fileType!=142) || fileTypePosition<0)
            throw Bad("PTQ1 export requires a UINT32 Prism PQ2 general.file_type (141 or legacy 142).");
        if(!tensors.Any(t=>t.Type==142)) throw Bad("PTQ1 export requires at least one PQ2 tensor.");
        uint alignment=(uint)report["Alignment"];
        ulong dataOffset=(ulong)report["TensorDataOffset"];
        var ordered=tensors.OrderBy(t=>t.Offset).ToArray();
        var offsets=new ulong[ordered.Length];
        ulong extent=0, blocks=0;
        for(int index=0;index<ordered.Length;index++) {
            Tensor tensor=ordered[index];
            offsets[index]=Align(extent,alignment);
            ulong bytes=tensor.Type==142 ? checked(tensor.Elements/128*28) : tensor.Bytes;
            extent=checked(offsets[index]+bytes);
            if(tensor.Type==142) blocks=checked(blocks+tensor.Elements/128);
        }
        ulong outputBytes=checked(dataOffset+Align(extent,alignment));
        if(outputBytes>long.MaxValue) throw Bad("export exceeds supported file size.");
        var clock=Stopwatch.StartNew();
        const int batchBlocks=1927;
        var input=new byte[batchBlocks*34];
        var packed=new byte[batchBlocks*28];
        var restored=new byte[batchBlocks*34];
        var zeros=new byte[8192];
        using(var output=new FileStream(outputPath,FileMode.CreateNew,FileAccess.ReadWrite,FileShare.None)) {
            using(var writer=new BinaryWriter(output,Encoding.UTF8,true)) {
                CopyTo(output,0,checked((long)dataOffset),clock,timeoutSeconds);
                output.Position=fileTypePosition; writer.Write(143U);
                for(int index=0;index<ordered.Length;index++) {
                    Tensor tensor=ordered[index];
                    output.Position=tensor.TypePosition;
                    writer.Write(tensor.Type==142 ? 143U : tensor.Type);
                    writer.Write(offsets[index]);
                }
                output.Position=checked((long)dataOffset);
                for(int index=0;index<ordered.Length;index++) {
                    Tensor tensor=ordered[index];
                    PadTo(output,checked((long)(dataOffset+offsets[index])),zeros);
                    long sourceStart=checked((long)(dataOffset+tensor.Offset));
                    if(tensor.Type!=142) { CopyTo(output,sourceStart,checked((long)tensor.Bytes),clock,timeoutSeconds); continue; }
                    file.Position=sourceStart;
                    ulong remaining=tensor.Elements/128;
                    while(remaining>0) {
                        if(clock.Elapsed.TotalSeconds>timeoutSeconds) throw new TimeoutException("PTQ1 export exceeded its deadline.");
                        int count=(int)Math.Min(remaining,batchBlocks);
                        var source=input.AsSpan(0,count*34);
                        var destination=packed.AsSpan(0,count*28);
                        file.ReadExactly(source);
                        Pq2Ptq1Codec.EncodeBlocks(source,destination);
                        Pq2Ptq1Codec.DecodeBlocks(destination,restored.AsSpan(0,count*34));
                        if(!source.SequenceEqual(restored.AsSpan(0,count*34))) throw Bad("PTQ1 export failed exact PQ2 reconstruction.");
                        output.Write(destination);
                        remaining-=(ulong)count;
                    }
                }
                PadTo(output,checked((long)outputBytes),zeros);
                output.Flush(true);
            }
            output.Position=0;
            string sha=Convert.ToHexStringLower(SHA256.HashData(output));
            return new Dictionary<string,object>{
                {"SchemaVersion",1},{"CodecCommit","73bfe6cc485554822f80c473a3c0c4f25e80f41b"},
                {"SourceBytes",file.Length},{"OutputBytes",outputBytes},{"OutputSha256",sha},
                {"ConvertedTensorCount",ordered.Count(t=>t.Type==142)},{"ConvertedBlocks",blocks},
                {"ExactPq2ReconstructionVerified",true},{"NonPq2TensorBytesCopiedVerbatim",true},
                {"MetadataChanges","general.file_type UINT32 -> 143; PQ2 tensor type/offset fields; alignment padding normalized"},
                {"WorkingBufferBytes",input.Length+packed.Length+restored.Length+hashBuffer.Length+zeros.Length},
                {"Qualification","weight-symbol-and-scale parity only; PTQ1 native activation quantization, logits, quality and performance unqualified"}
            };
        }
    }
    /// <summary>Exports a new experimental PTQ1 GGUF, verifying every converted block by exact PQ2 reconstruction.</summary>
    /// <param name="path">Read-only source GGUF. The caller must verify its independently pinned identity.</param>
    /// <param name="outputPath">A new partial file in an existing external directory; never overwritten or removed on failure.</param>
    /// <param name="timeoutSeconds">Conversion/copy deadline, between one second and one hour.</param>
    /// <returns>Output digest, byte count and explicit experimental qualification.</returns>
    public static Dictionary<string,object> RepackPq2ToPtq1(string path,string outputPath,int timeoutSeconds) {
        if(timeoutSeconds<1 || timeoutSeconds>3600) throw new ArgumentOutOfRangeException(nameof(timeoutSeconds));
        using(var parser=new GgufInspector(path,PrismDialect)) {
            try { return parser.ExportPtq1(outputPath,parser.Inspect(path),timeoutSeconds); }
            catch(OverflowException e) { throw new InvalidDataException("GGUF: export arithmetic overflow.",e); }
            catch(EndOfStreamException e) { throw new InvalidDataException("GGUF: truncated export source.",e); }
        }
    }
}
}
