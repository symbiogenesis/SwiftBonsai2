#nullable enable
using System;
using System.IO;
using System.Numerics;

namespace SwiftBonsai {
/// <summary>
/// Experimental reference codec for a single pinned Prism PQ2_0 block. This is
/// a symbol round-trip proof, not a model container, inference kernel, or runtime format.
/// </summary>
public static class Pq2BitcosCodec {
    // Source PQ2_0: 128 codes, little-endian raw FP16 scale, then 32 bytes of
    // low-first two-bit codes (-1,0,+1,+2). Pinned Prism source:
    // https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h
    // https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c
    // Reference BITCOS block: unchanged scale bytes [0..1], presence bitmap
    // [2..17] with logical position i in bit (i%8) of byte (2+i/8), then signs
    // in increasing logical-position order, little-bit packed, 0=-1 / 1=+1.
    // Absent positions represent original code 1. Unused high sign bits must
    // be zero. Length is exactly 18+ceil(nonzero/8), hence between 18 and 34.
    // There are no indices, offsets, alignment, framing, or runtime integration.
    private const int Elements = 128;
    private const int Pq2Bytes = 34;
    private const int PrefixBytes = 18;
    private static InvalidDataException Bad(string message) { return new InvalidDataException("PQ2/BITCOS reference block: " + message); }
    private static void ValidateScale(ReadOnlySpan<byte> source) {
        int scale=source[0] | (source[1]<<8);
        if((scale&0x7fff)==0) throw Bad("zero scale is outside conservative census eligibility.");
        if((scale&0x7c00)==0x7c00) throw Bad("nonfinite scale is outside conservative census eligibility.");
    }
    private static int Code(ReadOnlySpan<byte> source, int position) { return (source[2+position/4]>>(2*(position%4)))&3; }
    /// <summary>Repackages one eligible 34-byte PQ2_0 block into the experimental reference layout.</summary>
    /// <param name="source">Exactly one PQ2_0 block, with finite nonzero FP16 scale and no +2 codes.</param>
    /// <returns>A new canonical 18-to-34-byte reference block, preserving raw scale bits and all symbols.</returns>
    /// <exception cref="ArgumentNullException">The source is null.</exception>
    /// <exception cref="InvalidDataException">The block length, symbols, or scale are ineligible.</exception>
    public static byte[] EncodeBlock(byte[] source) {
        ArgumentNullException.ThrowIfNull(source);
        if(source.Length!=Pq2Bytes) throw Bad("PQ2 input must contain exactly 34 bytes.");
        ValidateScale(source);
        int nonzero=0;
        for(int position=0;position<Elements;position++) {
            int code=Code(source,position);
            if(code==3) throw Bad("+2 code cannot be represented by three symbols.");
            if(code!=1) nonzero++;
        }
        var encoded=new byte[PrefixBytes+(nonzero+7)/8];
        encoded[0]=source[0]; encoded[1]=source[1];
        int sign=0;
        for(int position=0;position<Elements;position++) {
            int code=Code(source,position);
            if(code==1) continue;
            encoded[2+position/8]|=(byte)(1<<(position%8));
            if(code==2) encoded[PrefixBytes+sign/8]|=(byte)(1<<(sign%8));
            sign++;
        }
        return encoded;
    }
    /// <summary>Restores exactly one original 34-byte PQ2_0 block from a canonical reference block.</summary>
    /// <param name="source">One reference block with exact sign length, finite nonzero scale, and zero unused sign bits.</param>
    /// <returns>A new 34-byte PQ2_0 block, byte-identical to the eligible block supplied to EncodeBlock.</returns>
    /// <exception cref="ArgumentNullException">The source is null.</exception>
    /// <exception cref="InvalidDataException">The block length, scale, or sign padding is invalid.</exception>
    public static byte[] DecodeBlock(byte[] source) {
        ArgumentNullException.ThrowIfNull(source);
        if(source.Length<PrefixBytes || source.Length>Pq2Bytes) throw Bad("reference input must contain 18 to 34 bytes.");
        ValidateScale(source);
        int nonzero=0;
        for(int index=2;index<PrefixBytes;index++) nonzero+=BitOperations.PopCount(source[index]);
        if(source.Length!=PrefixBytes+(nonzero+7)/8) throw Bad("sign byte count differs from the presence bitmap.");
        int remainder=nonzero%8;
        if(remainder!=0 && (source[^1]>>remainder)!=0) throw Bad("unused high sign bits must be zero.");
        var decoded=new byte[Pq2Bytes];
        decoded[0]=source[0]; decoded[1]=source[1];
        int sign=0;
        for(int position=0;position<Elements;position++) {
            int code=1;
            if((source[2+position/8]&(1<<(position%8)))!=0) {
                code=((source[PrefixBytes+sign/8]>>(sign%8))&1)*2;
                sign++;
            }
            decoded[2+position/4]|=(byte)(code<<(2*(position%4)));
        }
        return decoded;
    }
}
}
