using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace SwiftBonsai {
// Format and type table vetted before implementation against the locked llama.cpp:
// https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/include/gguf.h
// https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/gguf-py/gguf/gguf_reader.py
// https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/gguf-py/gguf/constants.py
// Sanity choices: tighter service-specific limits; skip vocab contents; retain small
// layout arrays; checked descriptor arithmetic; no mapping or tensor-payload reads.
public sealed class GgufInspector : IDisposable {
    const long HeaderLimit = 256L * 1024 * 1024;
    const ulong StringLimit = 8UL * 1024 * 1024;
    const ulong ArrayLimit = 2UL * 1024 * 1024;
    const ulong TotalElementLimit = 8UL * 1024 * 1024;
    readonly FileStream file;
    readonly BinaryReader reader;
    readonly byte[] hashBuffer = new byte[65536];
    readonly UTF8Encoding utf8 = new UTF8Encoding(false, true);
    ulong elements;
    readonly Dictionary<string, object> scalars = new Dictionary<string, object>(StringComparer.Ordinal);
    readonly Dictionary<string, object> arrays = new Dictionary<string, object>(StringComparer.Ordinal);
    readonly Dictionary<string, object> templates = new Dictionary<string, object>(StringComparer.Ordinal);
    readonly Dictionary<string, object> special = new Dictionary<string, object>(StringComparer.Ordinal);
    readonly List<Tensor> tensors = new List<Tensor>();
    readonly Dictionary<uint, Kind> kinds = new Dictionary<uint, Kind>();
    sealed class Kind {
        public string Name; public ulong Block; public ulong Bytes;
        public Kind(string name, ulong block, ulong bytes) { Name=name; Block=block; Bytes=bytes; }
    }
    sealed class Tensor {
        public string Name; public uint Type; public ulong Offset; public ulong Bytes; public ulong Elements;
    }
    GgufInspector(string path) {
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
    }
    void Add(uint id, string name, ulong block, ulong bytes) { kinds.Add(id,new Kind(name,block,bytes)); }
    static InvalidDataException Bad(string message) { return new InvalidDataException("GGUF: " + message); }
    void Need(ulong bytes) {
        if (bytes > (ulong)(file.Length-file.Position) || bytes > (ulong)Math.Max(0,HeaderLimit-file.Position))
            throw Bad("truncated or over-limit metadata/tensor descriptor region.");
    }
    uint U32() { Need(4); return reader.ReadUInt32(); }
    ulong U64() { Need(8); return reader.ReadUInt64(); }
    string Text(int limit) {
        ulong size=U64();
        if (size>(ulong)limit) throw Bad("name/string exceeds inspection limit.");
        Need(size);
        return utf8.GetString(reader.ReadBytes((int)size));
    }
    static int Width(uint type) {
        switch(type) {
            case 0: case 1: case 7: return 1;
            case 2: case 3: return 2;
            case 4: case 5: case 6: return 4;
            case 10: case 11: case 12: return 8;
            default: return 0;
        }
    }
    object Scalar(uint type) {
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
    object ArrayDescriptor(bool retain, uint itemType, ulong count) {
        // Only the outer retained record appears in the report. Nested skipped
        // arrays still receive every validation, without allocating a descriptor.
        return retain ? new Dictionary<string,object>{{"ElementTypeId",itemType},{"Count",count}} : null;
    }
    void SkipBooleans(ulong count) {
        Need(count);
        while(count>0) {
            int n=file.Read(hashBuffer,0,(int)Math.Min((ulong)hashBuffer.Length,count));
            if(n==0) throw Bad("truncated boolean array.");
            for(int i=0;i<n;i++) if(hashBuffer[i]>1) throw Bad("invalid boolean.");
            count-=(ulong)n;
        }
    }
    // Depth and aggregate counts cap both recursion and adversarial nested-array work.
    object Value(uint type, int depth, bool retain) {
        if(depth>4) throw Bad("array nesting exceeds inspection limit.");
        if(type==8) {
            ulong size=U64(); if(size>StringLimit) throw Bad("string exceeds 8 MiB inspection limit."); Need(size);
            if(retain && size<=65536) return utf8.GetString(reader.ReadBytes((int)size));
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
            object value=Value(itemType,depth+1,keep);
            if(keep) values.Add(value);
        }
        return keep ? (object)values : ArrayDescriptor(retain,itemType,count);
    }
    void HashRange(IncrementalHash hash, long start, long length) {
        long restore=file.Position;
        file.Position=start;
        while(length>0) {
            int n=file.Read(hashBuffer,0,(int)Math.Min(hashBuffer.Length,length));
            if(n==0) throw Bad("truncated hash region."); hash.AppendData(hashBuffer,0,n); length-=n;
        }
        file.Position=restore;
    }
    string Digest(long start,long length) {
        using(var hash=IncrementalHash.CreateHash(HashAlgorithmName.SHA256)) {
            HashRange(hash,start,length); return Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
        }
    }
    object Lookup(string key) { object value; return scalars.TryGetValue(key,out value) ? value : null; }
    Dictionary<string,object> Inspect(string path) {
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
                bool template=key=="tokenizer.chat_template" || key.StartsWith("tokenizer.chat_template.",StringComparison.Ordinal);
                if(template && type==8) {
                    long valueStart=file.Position; ulong size=U64();
                    if(size>StringLimit) throw Bad("template exceeds inspection limit."); Need(size); file.Position+=checked((long)size);
                    templates.Add(key,new Dictionary<string,object>{{"Utf8Bytes",size},{"Sha256",Digest(valueStart+8,(long)size)}});
                } else {
                    object value=Value(type,0,true);
                    if(type==9) arrays.Add(key,value); else if(value!=null) scalars.Add(key,value);
                    if(key.StartsWith("tokenizer.",StringComparison.Ordinal) && key.EndsWith("_token_id",StringComparison.Ordinal)) {
                        if(type!=4 && type!=5 && type!=10 && type!=11) throw Bad("special token ID must be an integer.");
                        special.Add(key,value);
                    }
                    if(key=="general.alignment") {
                        if(type!=4) throw Bad("general.alignment must be UINT32."); alignment=(uint)value;
                        if(alignment==0 || (alignment&(alignment-1))!=0 || alignment>1048576) throw Bad("invalid/over-limit alignment.");
                    }
                }
                if(key.StartsWith("tokenizer.",StringComparison.Ordinal)) {
                    long bytes=file.Position-start; HashRange(hash,start,bytes); tokenizerBytes=checked(tokenizerBytes+bytes); tokenizerRecords++;
                }
            }
            tokenizerHash=Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
        }
        long metadataEnd=file.Position;
        object architecture=Lookup("general.architecture");
        if(!(architecture is string) || ((string)architecture).Length==0) throw Bad("general.architecture string is required.");
        var names=new HashSet<string>(StringComparer.Ordinal);
        for(ulong i=0;i<tensorCount;i++) {
            string name=Text(1024); if(name.Length==0 || !names.Add(name)) throw Bad("empty or duplicate tensor name.");
            uint dimensions=U32(); if(dimensions<1 || dimensions>4) throw Bad("invalid tensor dimension count.");
            ulong n=1, first=0;
            for(uint d=0;d<dimensions;d++) { ulong size=U64(); if(size==0 || size>long.MaxValue) throw Bad("invalid tensor dimension."); if(d==0) first=size; n=checked(n*size); }
            uint type=U32(); Kind kind; if(!kinds.TryGetValue(type,out kind)) throw Bad("unknown/unsupported tensor type ID "+type+".");
            if(first%kind.Block!=0) throw Bad("tensor row is not divisible by quantization block size.");
            ulong bytes=checked(n/kind.Block*kind.Bytes), offset=U64();
            if(offset%alignment!=0) throw Bad("unaligned tensor offset.");
            tensors.Add(new Tensor{Name=name,Type=type,Offset=offset,Bytes=bytes,Elements=n});
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
        string arch=(string)architecture;
        ulong blocks=0, nextn=0;
        object blockValue=Lookup(arch+".block_count"), nextnValue=Lookup(arch+".nextn_predict_layers");
        if(blockValue!=null) blocks=Convert.ToUInt64(blockValue);
        if(nextnValue!=null) nextn=Convert.ToUInt64(nextnValue);
        if(nextn>blocks) throw Bad("MTP block count exceeds block count.");
        var mtp=new List<string>();
        foreach(Tensor t in tensors) {
            bool match=t.Name.StartsWith("nextn.",StringComparison.Ordinal) || t.Name.Contains(".nextn.",StringComparison.Ordinal);
            if(nextn>0 && t.Name.StartsWith("blk.",StringComparison.Ordinal)) {
                int end=t.Name.IndexOf('.',4); ulong block;
                if(end>4 && ulong.TryParse(t.Name.Substring(4,end-4),out block) && block>=blocks-nextn && block<blocks) match=true;
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
    public static Dictionary<string,object> Read(string path) {
        using(var parser=new GgufInspector(path)) {
            try { return parser.Inspect(path); }
            catch(OverflowException e) { throw new InvalidDataException("GGUF: descriptor/count arithmetic overflow.",e); }
        }
    }
    public void Dispose() { reader.Dispose(); file.Dispose(); }
}
}
