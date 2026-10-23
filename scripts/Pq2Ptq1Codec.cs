#nullable enable
using System;
using System.Buffers.Binary;
using System.IO;

namespace SwiftBonsai {
/// <summary>
/// Exact symbol/scale repacking between one current Prism PQ2_0 block and one
/// canonical PTQ1_0 block. This does not requantize weights or attest equal
/// runtime logits: the pinned CPU types use different activation quantizers.
/// </summary>
public static class Pq2Ptq1Codec {
    // Pinned Prism 73bfe6cc485554822f80c473a3c0c4f25e80f41b:
    // ggml/src/ggml-common.h block_ptq1_0, ggml/src/ggml-quants.c
    // quantize_row_ptq1_0_ref/dequantize_row_ptq1_0. This implements the byte
    // layout only, bypassing the floating-point quantizer completely.
    // PQ2: raw FP16[2], then 32 low-first two-bit code bytes (128 symbols).
    // PTQ: qs[24], qh[2], then the same raw FP16[2]. qs stages are 16 then 8
    // lanes, each byte encoding five high-first base-3 digits. qh has two
    // lanes of four digits, followed by a mandatory zero fifth digit.
    // Stored byte = ceil(base3_value*256/243), NOT the base-3 value itself.
    private const int Pq2Bytes = 34;
    private const int Ptq1Bytes = 28;
    private static InvalidDataException Bad(string message) { return new InvalidDataException("PQ2/PTQ1 block: " + message); }
    private static void ValidateScale(ReadOnlySpan<byte> scale) {
        int bits=BinaryPrimitives.ReadUInt16LittleEndian(scale);
        if((bits&0x7fff)==0 || (bits&0x7c00)==0x7c00) throw Bad("scale must be finite and nonzero under conservative census eligibility.");
    }
    private static int Code(ReadOnlySpan<byte> source, int position) { return (source[2+position/4]>>(2*(position%4)))&3; }
    private static byte CeilingByte(int value) { return (byte)((value*256+242)/243); }
    private static byte EncodeGroup(ReadOnlySpan<byte> source, int first, int stride, int count) {
        int value=0;
        for(int trit=0;trit<count;trit++) {
            int code=Code(source,first+trit*stride);
            if(code==3) throw Bad("+2 code cannot be represented by PTQ1_0.");
            value=value*3+code;
        }
        if(count==4) value*=3;
        return CeilingByte(value);
    }
    private static void DecodeGroup(byte packed, Span<byte> target, int first, int stride, int count) {
        int multiplier=1, canonical=0;
        for(int trit=0;trit<count;trit++) {
            // uint8 wrapping is part of the pinned native decoder. An explicit
            // mask preserves it even when the project enables checked arithmetic.
            int code=(((packed*multiplier)&255)*3)>>8;
            int position=first+trit*stride;
            target[2+position/4]|=(byte)(code<<(2*(position%4)));
            canonical=canonical*3+code;
            multiplier*=3;
        }
        if(count==4) canonical*=3;
        if(CeilingByte(canonical)!=packed) throw Bad("noncanonical PTQ1 trit byte or nonzero unused tail trit.");
    }
    /// <summary>Repackages an eligible 34-byte PQ2_0 block as canonical 28-byte PTQ1_0.</summary>
    /// <param name="source">One PQ2 block containing only codes 0/1/2 and a finite nonzero raw FP16 scale.</param>
    /// <returns>A new PTQ1 block with unchanged symbols and raw scale bits.</returns>
    /// <exception cref="ArgumentNullException">The source is null.</exception>
    /// <exception cref="InvalidDataException">The length, fourth symbol, or scale is ineligible.</exception>
    public static byte[] EncodeBlock(byte[] source) {
        ArgumentNullException.ThrowIfNull(source);
        if(source.Length!=Pq2Bytes) throw Bad("PQ2 input must contain exactly 34 bytes.");
        var target=new byte[Ptq1Bytes];
        EncodeBlocks(source,target);
        return target;
    }
    private static void EncodeOne(ReadOnlySpan<byte> source, Span<byte> target) {
        ValidateScale(source[..2]);
        for(int lane=0;lane<16;lane++) target[lane]=EncodeGroup(source,lane,16,5);
        for(int lane=0;lane<8;lane++) target[16+lane]=EncodeGroup(source,80+lane,8,5);
        for(int lane=0;lane<2;lane++) target[24+lane]=EncodeGroup(source,120+lane,2,4);
        target[26]=source[0]; target[27]=source[1];
    }
    /// <summary>Restores the exact original 34-byte PQ2 block from one canonical PTQ1 block.</summary>
    /// <param name="source">One canonical 28-byte PTQ1 block with a finite nonzero raw FP16 scale.</param>
    /// <returns>A new PQ2 block with byte-identical symbols and scale bits.</returns>
    /// <exception cref="ArgumentNullException">The source is null.</exception>
    /// <exception cref="InvalidDataException">The length, scale, or canonical trit representation is invalid.</exception>
    public static byte[] DecodeBlock(byte[] source) {
        ArgumentNullException.ThrowIfNull(source);
        if(source.Length!=Ptq1Bytes) throw Bad("PTQ1 input must contain exactly 28 bytes.");
        var target=new byte[Pq2Bytes];
        DecodeBlocks(source,target);
        return target;
    }
    private static void DecodeOne(ReadOnlySpan<byte> source, Span<byte> target) {
        ValidateScale(source.Slice(26,2));
        target.Clear();
        for(int lane=0;lane<16;lane++) DecodeGroup(source[lane],target,lane,16,5);
        for(int lane=0;lane<8;lane++) DecodeGroup(source[16+lane],target,80+lane,8,5);
        for(int lane=0;lane<2;lane++) DecodeGroup(source[24+lane],target,120+lane,2,4);
        target[0]=source[26]; target[1]=source[27];
    }
    /// <summary>Repackages complete PQ2 blocks into an exactly sized caller-owned PTQ1 buffer without allocation.</summary>
    /// <param name="source">A whole number of 34-byte PQ2 blocks; an empty source is allowed.</param>
    /// <param name="destination">Exactly 28 bytes per source block; must not overlap the source.</param>
    /// <exception cref="InvalidDataException">Lengths, overlap, a fourth symbol, or a scale are invalid.</exception>
    /// <remarks>On an invalid later block, earlier destination blocks may already have been written.</remarks>
    public static void EncodeBlocks(ReadOnlySpan<byte> source, Span<byte> destination) {
        if(source.Length%Pq2Bytes!=0 || destination.Length!=(long)source.Length/Pq2Bytes*Ptq1Bytes) throw Bad("batch sizes must match complete 34-to-28-byte blocks.");
        if(source.Overlaps(destination)) throw Bad("source and destination must not overlap.");
        int blocks=source.Length/Pq2Bytes;
        for(int block=0;block<blocks;block++) EncodeOne(source.Slice(block*Pq2Bytes,Pq2Bytes),destination.Slice(block*Ptq1Bytes,Ptq1Bytes));
    }
    /// <summary>Restores complete canonical PTQ1 blocks into an exactly sized caller-owned PQ2 buffer without allocation.</summary>
    /// <param name="source">A whole number of 28-byte canonical PTQ1 blocks; an empty source is allowed.</param>
    /// <param name="destination">Exactly 34 bytes per source block; must not overlap the source.</param>
    /// <exception cref="InvalidDataException">Lengths, overlap, a scale, or canonical trit bytes are invalid.</exception>
    /// <remarks>On an invalid later block, earlier destination blocks may already have been written.</remarks>
    public static void DecodeBlocks(ReadOnlySpan<byte> source, Span<byte> destination) {
        if(source.Length%Ptq1Bytes!=0 || destination.Length!=(long)source.Length/Ptq1Bytes*Pq2Bytes) throw Bad("batch sizes must match complete 28-to-34-byte blocks.");
        if(source.Overlaps(destination)) throw Bad("source and destination must not overlap.");
        int blocks=source.Length/Ptq1Bytes;
        for(int block=0;block<blocks;block++) DecodeOne(source.Slice(block*Ptq1Bytes,Ptq1Bytes),destination.Slice(block*Pq2Bytes,Pq2Bytes));
    }
}
}
