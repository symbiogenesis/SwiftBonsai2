#nullable enable
using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Globalization;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace SwiftBonsai {
// Format and type table vetted before implementation against the locked llama.cpp:
// https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/include/gguf.h
// https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/gguf-py/gguf/gguf_reader.py
// https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/gguf-py/gguf/constants.py
// Sanity choices: tighter service-specific limits; skip vocab contents; retain small
// layout arrays; checked descriptor arithmetic; no mapping or model loading.
/// <summary>Inspects bounded GGUF metadata and optionally streams a pinned Prism PQ2 symbol census.</summary>
public sealed partial class GgufInspector : IDisposable {
    private const string PrismDialect = "Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b";
    private const int Pq2BlockBytes = 34;
    private const int Pq2BlockElements = 128;
    private const long HeaderLimit = 256L * 1024 * 1024;
    private const ulong StringLimit = 8UL * 1024 * 1024;
    private const ulong ArrayLimit = 2UL * 1024 * 1024;
    private const ulong TotalElementLimit = 8UL * 1024 * 1024;
    private readonly FileStream file;
    private readonly BinaryReader reader;
    private readonly byte[] hashBuffer = new byte[65536];
    private readonly UTF8Encoding utf8 = new UTF8Encoding(false, true);
    private ulong elements;
    private readonly Dictionary<string, object> scalars = new Dictionary<string, object>(StringComparer.Ordinal);
    private readonly Dictionary<string, object> arrays = new Dictionary<string, object>(StringComparer.Ordinal);
    private readonly Dictionary<string, object> templates = new Dictionary<string, object>(StringComparer.Ordinal);
    private readonly Dictionary<string, object> special = new Dictionary<string, object>(StringComparer.Ordinal);
    private readonly Collection<Tensor> tensors = new Collection<Tensor>();
    private readonly Dictionary<uint, Kind> kinds = new Dictionary<uint, Kind>();
    private readonly string dialect;
    private sealed record Kind(string Name, ulong Block, ulong Bytes);
    private sealed record Tensor(string Name, uint Type, ulong Offset, ulong Bytes, ulong Elements, long TypePosition);
    private long fileTypePosition = -1;
    private GgufInspector(string path, string dialect) {
        if(dialect!="Stock" && dialect!=PrismDialect) throw Bad("unknown dialect; use Stock or the explicitly pinned Prism dialect.");
        this.dialect=dialect;
        file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
        reader = new BinaryReader(file, utf8, true);
        // The numeric IDs are ggml tensor types, not general.file_type quantization labels.
        Add(0,"F32",1,4); Add(1,"F16",1,2); Add(2,"Q4_0",32,18); Add(3,"Q4_1",32,20);
        Add(6,"Q5_0",32,22); Add(7,"Q5_1",32,24); Add(8,"Q8_0",32,34); Add(9,"Q8_1",32,36);
        Add(10,"Q2_K",256,84); Add(11,"Q3_K",256,110); Add(12,"Q4_K",256,144);
        Add(13,"Q5_K",256,176); Add(14,"Q6_K",256,210); Add(15,"Q8_K",256,292);
        Add(16,"IQ2_XXS",256,66); Add(17,"IQ2_XS",256,74); Add(18,"IQ3_XXS",256,98);
        Add(19,"IQ1_S",256,50); Add(20,"IQ4_NL",32,18); Add(21,"IQ3_S",256,110);
        Add(22,"IQ2_S",256,82); Add(23,"IQ4_XS",256,136); Add(24,"I8",1,1);
        Add(25,"I16",1,2); Add(26,"I32",1,4); Add(27,"I64",1,8); Add(28,"F64",1,8);
        Add(29,"IQ1_M",256,56); Add(30,"BF16",1,2); Add(34,"TQ1_0",256,54);
        Add(35,"TQ2_0",256,66); Add(39,"MXFP4",32,17); Add(40,"NVFP4",64,36);
        Add(41,"Q1_0",128,18); Add(42,"Q2_0",64,18);
        // Never reinterpret stock type IDs. This table is opt-in and tied to:
        // https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/include/ggml.h
        // https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h
        if(dialect==PrismDialect) {
            Add(142,"PQ2_0",Pq2BlockElements,Pq2BlockBytes);
            Add(143,"PTQ1_0",128,28);
        }
    }
    private void Add(uint id, string name, ulong block, ulong bytes) { kinds.Add(id,new Kind(name,block,bytes)); }
    private static InvalidDataException Bad(string message) { return new InvalidDataException("GGUF: " + message); }
    private void Need(ulong bytes) {
        if (bytes > (ulong)(file.Length-file.Position) || bytes > (ulong)Math.Max(0,HeaderLimit-file.Position))
            throw Bad("truncated or over-limit metadata/tensor descriptor region.");
    }
    private uint U32() { Need(4); return reader.ReadUInt32(); }
    private ulong U64() { Need(8); return reader.ReadUInt64(); }
    private string Text(int limit) {
        ulong size=U64();
        if (size>(ulong)limit) throw Bad("name/string exceeds inspection limit.");
        Need(size);
        return ReadText((int)size);
    }
    private string ReadText(int size) {
        // Names and retained strings fit the shared buffer. Decode directly
        // from its span instead of allocating a byte array for every string.
        Span<byte> bytes=hashBuffer.AsSpan(0,size);
        file.ReadExactly(bytes);
        return utf8.GetString(bytes);
    }
    private static int Width(uint type) => type switch {
        0 or 1 or 7 => 1,
        2 or 3 => 2,
        4 or 5 or 6 => 4,
        10 or 11 or 12 => 8,
        _ => 0
    };
    private object Scalar(uint type) {
        int width=Width(type); if(width==0) throw Bad("unknown scalar type."); Need((ulong)width);
        switch(type) {
            case 0: return reader.ReadByte(); case 1: return reader.ReadSByte();
            case 2: return reader.ReadUInt16(); case 3: return reader.ReadInt16();
            case 4: return reader.ReadUInt32(); case 5: return reader.ReadInt32();
            case 6: { float v=reader.ReadSingle(); if(!float.IsFinite(v)) throw Bad("non-finite float metadata."); return v; }
            case 7: { byte v=reader.ReadByte(); if(v>1) throw Bad("invalid boolean."); return v!=0; }
            case 10: return reader.ReadUInt64(); case 11: return reader.ReadInt64();
            case 12: { double v=reader.ReadDouble(); if(!double.IsFinite(v)) throw Bad("non-finite float metadata."); return v; }
            default: throw Bad("unknown scalar type.");
        }
    }
    private static Dictionary<string,object>? ArrayDescriptor(bool retain, uint itemType, ulong count) {
        // Only the outer retained record appears in the report. Nested skipped
        // arrays still receive every validation, without allocating a descriptor.
        return retain ? new Dictionary<string,object>{{"ElementTypeId",itemType},{"Count",count}} : null;
    }
    private void SkipBooleans(ulong count) {
        Need(count);
        while(count>0) {
            int n=file.Read(hashBuffer.AsSpan(0,(int)Math.Min((ulong)hashBuffer.Length,count)));
            if(n==0) throw Bad("truncated boolean array.");
            if(hashBuffer.AsSpan(0,n).ContainsAnyExceptInRange((byte)0,(byte)1)) throw Bad("invalid boolean.");
            count-=(ulong)n;
        }
    }
    // Depth and aggregate counts cap both recursion and adversarial nested-array work.
    private object? Value(uint type, int depth, bool retain) {
        if(depth>4) throw Bad("array nesting exceeds inspection limit.");
        if(type==8) {
            ulong size=U64(); if(size>StringLimit) throw Bad("string exceeds 8 MiB inspection limit."); Need(size);
            if(retain && size<=(ulong)hashBuffer.Length) return ReadText((int)size);
            file.Position+=checked((long)size); return null;
        }
        if(type!=9) return Scalar(type);
        uint itemType=U32(); if(itemType>12) throw Bad("unknown array element type.");
        ulong count=U64(); if(count>ArrayLimit || count>TotalElementLimit-elements) throw Bad("array element limit exceeded.");
        elements+=count;
        int width=Width(itemType);
        bool keep=retain && count<=4096 && itemType!=8 && itemType!=9;
        if(width>0 && !keep && itemType!=7) {
            ulong bytes=checked(count*(ulong)width); Need(bytes); file.Position+=checked((long)bytes);
            return ArrayDescriptor(retain,itemType,count);
        }
        if(!keep && itemType==7) {
            // Validate every boolean, including values past each buffer boundary,
            // without a recursive call and boxed object for each discarded value.
            if(count>0 && depth==4) throw Bad("array nesting exceeds inspection limit.");
            SkipBooleans(count);
            return ArrayDescriptor(retain,itemType,count);
        }
        var values=keep ? new List<object>((int)count) : null;
        for(ulong i=0;i<count;i++) {
            object? value=Value(itemType,depth+1,keep);
            if(values is not null) values.Add(value ?? throw Bad("retained scalar cannot be null."));
        }
        return values is not null ? values : ArrayDescriptor(retain,itemType,count);
    }
    private void HashRange(IncrementalHash hash, long start, long length) {
        ArgumentNullException.ThrowIfNull(hash);
        long restore=file.Position;
        file.Position=start;
        while(length>0) {
            int n=file.Read(hashBuffer.AsSpan(0,(int)Math.Min(hashBuffer.Length,length)));
            if(n==0) throw Bad("truncated hash region."); hash.AppendData(hashBuffer.AsSpan(0,n)); length-=n;
        }
        file.Position=restore;
    }
    private string Digest(long start,long length) {
        using(var hash=IncrementalHash.CreateHash(HashAlgorithmName.SHA256)) {
            HashRange(hash,start,length); return Convert.ToHexStringLower(hash.GetHashAndReset());
        }
    }
    private object? Lookup(string key) { object? value; return scalars.TryGetValue(key,out value) ? value : null; }
    private static ulong SignBytes(ulong nonzero) { return checked((nonzero+7)/8); }
    private static Dictionary<string,object> Candidate(bool eligible, ulong bitmapBytes, ulong signLowerBound, ulong blockSignBytes, ulong scaleBytes) {
        var result=new Dictionary<string,object>{{"Eligible",eligible},{"IncludesFormatOrIndexOverhead",false}};
        if(eligible) {
            result.Add("BitmapBytes",bitmapBytes);
            result.Add("UnchangedFp16ScaleBytes",scaleBytes);
            result.Add("TensorPackedSignBytesLowerBound",signLowerBound);
            result.Add("Block128BytePaddedSignBytes",blockSignBytes);
            result.Add("TensorPackedPayloadBytesLowerBound",checked(bitmapBytes+scaleBytes+signLowerBound));
            result.Add("Block128BytePaddedPayloadBytes",checked(bitmapBytes+scaleBytes+blockSignBytes));
        }
        return result;
    }
    private Dictionary<string,object> CensusPq2(ulong dataOffset) {
        if(dialect!=PrismDialect) throw Bad("PQ2 census requires the explicitly pinned Prism dialect.");
        // Count four two-bit symbols at once. Work is linear in serialized PQ2
        // bytes, with one shared 64 KiB buffer, one 256-entry table, and no
        // per-block/per-weight allocations or dequantized floating-point arrays.
        // Four byte-wide lanes hold each block's four counts. A block has only
        // 128 symbols, so lane addition cannot carry into an adjacent lane.
        var codeCounts=new uint[256];
        for(int value=0;value<256;value++)
            for(int shift=0;shift<8;shift+=2) codeCounts[value]+=1U<<(((value>>shift)&3)*8);
        var reports=new List<object>();
        ulong[] totalCodes=new ulong[4];
        ulong totalBytes=0, totalBlocks=0, totalZeroScales=0, totalNonfiniteScales=0;
        ulong totalSignLowerBound=0, totalBlockSigns=0, unchangedBytes=0;
        bool allEligible=true;
        foreach(Tensor tensor in tensors.OrderBy(t=>t.Offset)) {
            if(tensor.Type!=142) { unchangedBytes=checked(unchangedBytes+tensor.Bytes); continue; }
            file.Position=checked((long)(dataOffset+tensor.Offset));
            ulong blocks=tensor.Elements/Pq2BlockElements, remaining=blocks;
            ulong[] counts=new ulong[4];
            ulong zeroScales=0, nonfiniteScales=0, blockSigns=0;
            while(remaining>0) {
                int batch=(int)Math.Min(remaining,(ulong)(hashBuffer.Length/Pq2BlockBytes));
                int bytes=checked(batch*Pq2BlockBytes);
                // Descriptor extent checks run before this method. ReadExactly
                // still rejects truncation if a non-Windows filesystem changes.
                file.ReadExactly(hashBuffer.AsSpan(0,bytes));
                for(int block=0;block<batch;block++) {
                    int start=block*Pq2BlockBytes;
                    int scale=BinaryPrimitives.ReadUInt16LittleEndian(hashBuffer.AsSpan(start,2));
                    if((scale&0x7fff)==0) zeroScales++;
                    else if((scale&0x7c00)==0x7c00) nonfiniteScales++;
                    uint blockCodeCounts=0;
                    for(int offset=2;offset<Pq2BlockBytes;offset++) {
                        byte value=hashBuffer[start+offset];
                        blockCodeCounts+=codeCounts[value];
                    }
                    counts[0]+=blockCodeCounts&255;
                    uint zeros=(blockCodeCounts>>8)&255;
                    counts[1]+=zeros;
                    counts[2]+=(blockCodeCounts>>16)&255;
                    counts[3]+=blockCodeCounts>>24;
                    blockSigns+=SignBytes(Pq2BlockElements-zeros);
                }
                remaining-=(ulong)batch;
            }
            ulong nonzero=checked(counts[0]+counts[2]+counts[3]);
            bool eligible=counts[3]==0 && zeroScales==0 && nonfiniteScales==0;
            ulong signLowerBound=SignBytes(nonzero), bitmapBytes=checked(blocks*16), scaleBytes=checked(blocks*2);
            reports.Add(new Dictionary<string,object>{
                {"Name",tensor.Name},{"TypeId",tensor.Type},{"Blocks",blocks},{"Scales",blocks},{"ScaleBytes",scaleBytes},{"Elements",tensor.Elements},{"SerializedBytes",tensor.Bytes},
                {"CodeMinusOne",counts[0]},{"CodeZero",counts[1]},{"CodePlusOne",counts[2]},{"CodePlusTwo",counts[3]},
                {"ZeroScaleBlocks",zeroScales},{"NonfiniteScaleBlocks",nonfiniteScales},{"StrictTernaryCodes",counts[3]==0},
                {"LosslessThreeSymbolEligible",eligible},{"BitcosCandidate",Candidate(eligible,bitmapBytes,signLowerBound,blockSigns,scaleBytes)}
            });
            for(int code=0;code<4;code++) totalCodes[code]=checked(totalCodes[code]+counts[code]);
            totalBytes=checked(totalBytes+tensor.Bytes); totalBlocks=checked(totalBlocks+blocks);
            totalZeroScales=checked(totalZeroScales+zeroScales); totalNonfiniteScales=checked(totalNonfiniteScales+nonfiniteScales);
            totalSignLowerBound=checked(totalSignLowerBound+signLowerBound); totalBlockSigns=checked(totalBlockSigns+blockSigns);
            allEligible &= eligible;
        }
        if(reports.Count==0) throw Bad("PQ2 census found no PQ2_0 tensors.");
        var candidate=Candidate(allEligible,checked(totalBlocks*16),totalSignLowerBound,totalBlockSigns,checked(totalBlocks*2));
        if(allEligible) {
            ulong retained=checked((ulong)file.Length-totalBytes);
            candidate.Add("WholeFileBytesLowerBoundWithExistingNonPq2Bytes",checked(retained+(ulong)candidate["TensorPackedPayloadBytesLowerBound"]));
            candidate.Add("WholeFileBlock128BytePaddedBytesWithExistingNonPq2Bytes",checked(retained+(ulong)candidate["Block128BytePaddedPayloadBytes"]));
        }
        return new Dictionary<string,object>{
            {"CodecCommit","73bfe6cc485554822f80c473a3c0c4f25e80f41b"},{"TensorCount",reports.Count},{"Tensors",reports},
            {"Blocks",totalBlocks},{"Scales",totalBlocks},{"ScaleBytes",checked(totalBlocks*2)},{"SerializedBytesRead",totalBytes},{"UnchangedOtherTensorBytes",unchangedBytes},
            {"CodeMinusOne",totalCodes[0]},{"CodeZero",totalCodes[1]},{"CodePlusOne",totalCodes[2]},{"CodePlusTwo",totalCodes[3]},
            {"ZeroScaleBlocks",totalZeroScales},{"NonfiniteScaleBlocks",totalNonfiniteScales},{"LosslessThreeSymbolEligible",allEligible},
            {"BitcosCandidate",candidate},{"PayloadBufferBytes",hashBuffer.Length},{"MaximumBlocksPerRead",hashBuffer.Length/Pq2BlockBytes},
            {"Definition","PQ2_0 stores little-endian FP16 scale then 32 bytes of low-to-high 2-bit codes; decoded value is (code-1)*scale. Counts are raw codes, never decoded zeros. Eligibility requires no +2 codes and finite nonzero scales. Candidate bytes retain exact scale bytes and exclude format, tensor offsets, indices and alignment overhead. They are serialized-size estimates, not RAM or throughput measurements."}
        };
    }
    private Dictionary<string,object> Inspect(string path) {
        if(U32()!=0x46554747) throw Bad("invalid magic.");
        if(U32()!=3) throw Bad("only little-endian version 3 is supported.");
        ulong tensorCount=U64(), metadataCount=U64();
        if(tensorCount>100000 || metadataCount>10000) throw Bad("tensor/KV count exceeds inspection limit.");
        var keys=new HashSet<string>(StringComparer.Ordinal);
        uint alignment=32;
        string tokenizerHash; long tokenizerBytes=0; int tokenizerRecords=0;
        using(var hash=IncrementalHash.CreateHash(HashAlgorithmName.SHA256)) {
            for(ulong i=0;i<metadataCount;i++) {
                long start=file.Position; string key=Text(1024);
                if(key.Length==0 || !keys.Add(key)) throw Bad("empty or duplicate metadata key.");
                uint type=U32();
                if(key=="general.file_type") fileTypePosition=file.Position;
                bool template=key=="tokenizer.chat_template" || key.StartsWith("tokenizer.chat_template.",StringComparison.Ordinal);
                if(template && type==8) {
                    long valueStart=file.Position; ulong size=U64();
                    if(size>StringLimit) throw Bad("template exceeds inspection limit."); Need(size); file.Position+=checked((long)size);
                    templates.Add(key,new Dictionary<string,object>{{"Utf8Bytes",size},{"Sha256",Digest(valueStart+8,(long)size)}});
                } else {
                    object? value=Value(type,0,true);
                    if(type==9) arrays.Add(key,value ?? throw Bad("retained array descriptor is required.")); else if(value!=null) scalars.Add(key,value);
                    if(key.StartsWith("tokenizer.",StringComparison.Ordinal) && key.EndsWith("_token_id",StringComparison.Ordinal)) {
                        if(type!=4 && type!=5 && type!=10 && type!=11) throw Bad("special token ID must be an integer.");
                        special.Add(key,value ?? throw Bad("special token ID is required."));
                    }
                    if(key=="general.alignment") {
                        if(value is not uint alignmentValue) throw Bad("general.alignment must be UINT32."); alignment=alignmentValue;
                        if(alignment==0 || (alignment&(alignment-1))!=0 || alignment>1048576) throw Bad("invalid/over-limit alignment.");
                    }
                }
                if(key.StartsWith("tokenizer.",StringComparison.Ordinal)) {
                    long bytes=file.Position-start; HashRange(hash,start,bytes); tokenizerBytes=checked(tokenizerBytes+bytes); tokenizerRecords++;
                }
            }
            tokenizerHash=Convert.ToHexStringLower(hash.GetHashAndReset());
        }
        long metadataEnd=file.Position;
        object? architecture=Lookup("general.architecture");
        if(architecture is not string arch || arch.Length==0) throw Bad("general.architecture string is required.");
        var names=new HashSet<string>(StringComparer.Ordinal);
        for(ulong i=0;i<tensorCount;i++) {
            string name=Text(1024); if(name.Length==0 || !names.Add(name)) throw Bad("empty or duplicate tensor name.");
            uint dimensions=U32(); if(dimensions<1 || dimensions>4) throw Bad("invalid tensor dimension count.");
            ulong n=1, first=0;
            for(uint d=0;d<dimensions;d++) { ulong size=U64(); if(size==0 || size>long.MaxValue) throw Bad("invalid tensor dimension."); if(d==0) first=size; n=checked(n*size); }
            long typePosition=file.Position;
            uint type=U32(); Kind? kind; if(!kinds.TryGetValue(type,out kind)) throw Bad("unknown/unsupported tensor type ID "+type+".");
            if(first%kind.Block!=0) throw Bad("tensor row is not divisible by quantization block size.");
            ulong bytes=checked(n/kind.Block*kind.Bytes), offset=U64();
            if(offset%alignment!=0) throw Bad("unaligned tensor offset.");
            tensors.Add(new Tensor(name,type,offset,bytes,n,typePosition));
        }
        long descriptorsEnd=file.Position;
        ulong dataOffset=(ulong)descriptorsEnd;
        if(tensorCount>0) dataOffset=checked((dataOffset+alignment-1)/alignment*alignment);
        if(dataOffset>(ulong)file.Length) throw Bad("tensor data start exceeds file length.");
        ulong previousEnd=0;
        foreach(Tensor tensor in tensors.OrderBy(t=>t.Offset)) {
            ulong end=checked(tensor.Offset+tensor.Bytes);
            if(end>(ulong)file.Length-dataOffset || tensor.Offset<previousEnd) throw Bad("tensor payload extent is truncated or overlapping.");
            previousEnd=end;
        }
        var histogram=new List<object>();
        foreach(var group in tensors.GroupBy(t=>t.Type).OrderBy(g=>g.Key)) {
            ulong bytes=0, parameters=0;
            foreach(Tensor t in group) { bytes=checked(bytes+t.Bytes); parameters=checked(parameters+t.Elements); }
            histogram.Add(new Dictionary<string,object>{{"TypeId",group.Key},{"Name",kinds[group.Key].Name},{"Count",group.Count()},{"Bytes",bytes},{"Elements",parameters}});
        }
        ulong blocks=0, nextn=0;
        object? blockValue=Lookup(arch+".block_count"), nextnValue=Lookup(arch+".nextn_predict_layers");
        if(blockValue!=null) blocks=Convert.ToUInt64(blockValue,CultureInfo.InvariantCulture);
        if(nextnValue!=null) nextn=Convert.ToUInt64(nextnValue,CultureInfo.InvariantCulture);
        if(nextn>blocks) throw Bad("MTP block count exceeds block count.");
        var mtp=new List<string>();
        foreach(Tensor t in tensors) {
            bool match=t.Name.StartsWith("nextn.",StringComparison.Ordinal) || t.Name.Contains(".nextn.",StringComparison.Ordinal);
            if(nextn>0 && t.Name.StartsWith("blk.",StringComparison.Ordinal)) {
                int end=t.Name.IndexOf('.',4); ulong block;
                if(end>4 && ulong.TryParse(t.Name.AsSpan(4,end-4),NumberStyles.None,CultureInfo.InvariantCulture,out block) && block>=blocks-nextn && block<blocks) match=true;
            }
            if(match) mtp.Add(t.Name);
        }
        return new Dictionary<string,object>{
            {"SchemaVersion",1},{"Format","GGUF"},{"Version",3},{"ByteOrder","little"},{"ModelPath",path},
            {"FileBytes",file.Length},{"Architecture",architecture},{"Alignment",alignment},{"MetadataCount",metadataCount},
            {"MetadataEndOffset",metadataEnd},{"TensorDescriptorsEndOffset",descriptorsEnd},{"TensorDataOffset",dataOffset},
            {"TensorCount",tensorCount},{"TensorTypeHistogram",histogram},{"Scalars",scalars},{"Arrays",arrays},
            {"SpecialTokenIds",special},{"EmbeddedTemplates",templates},{"MtpTensorCount",mtp.Count},{"MtpTensorNames",mtp},
            {"TokenizerMetadataSha256",tokenizerHash},{"TokenizerMetadataBytes",tokenizerBytes},{"TokenizerMetadataRecords",tokenizerRecords},
            {"TokenizerDigestDefinition","SHA-256 of complete raw serialized tokenizer.* KV records concatenated in file order, including chat templates"},
            {"InspectionLimits",new Dictionary<string,object>{{"HeaderBytes",HeaderLimit},{"StringBytes",StringLimit},{"ArrayElements",ArrayLimit},{"TotalArrayElements",TotalElementLimit},{"ArrayNesting",4},{"MetadataRecords",10000},{"Tensors",100000}}},
            {"TensorPayloadRead",false}
        };
    }
    /// <summary>Reads a validated metadata report from a GGUF version 3 file.</summary>
    /// <param name="path">Path to the input GGUF file.</param>
    /// <returns>The inspected metadata, descriptor summaries, and inspection limits.</returns>
    public static Dictionary<string,object> Read(string path) { return Read(path,"Stock",false); }
    /// <summary>Inspects GGUF and optionally streams PQ2 payloads using an explicitly pinned codec dialect.</summary>
    /// <param name="path">Path to the input GGUF file.</param>
    /// <param name="dialect">Stock, or Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b.</param>
    /// <param name="censusPq2">Whether to read every PQ2 payload for symbol and candidate-size counts.</param>
    /// <returns>Validated inspection and optional bounded payload census.</returns>
    public static Dictionary<string,object> Read(string path, string dialect, bool censusPq2) {
        using(var parser=new GgufInspector(path,dialect)) {
            try {
                Dictionary<string,object> report=parser.Inspect(path);
                report.Add("Dialect",dialect);
                if(censusPq2) {
                    report.Add("Pq2Census",parser.CensusPq2((ulong)report["TensorDataOffset"]));
                    report["TensorPayloadRead"]=true;
                }
                return report;
            }
            catch(OverflowException e) { throw new InvalidDataException("GGUF: descriptor/count arithmetic overflow.",e); }
            catch(EndOfStreamException e) { throw new InvalidDataException("GGUF: truncated tensor payload.",e); }
        }
    }
    /// <summary>Closes the metadata reader and its file handle.</summary>
    public void Dispose() { reader.Dispose(); file.Dispose(); }
}
}
