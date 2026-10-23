#nullable enable
using System;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace SwiftBonsai.Validation {
/// <summary>Model-free native consumer checks for the pinned Prism PQ2/PTQ1 formats.</summary>
[SupportedOSPlatform("windows10.0")]
public static class PrismCodecProbe {
    private const string RuntimeCommit = "73bfe6cc485554822f80c473a3c0c4f25e80f41b";
    private const int BlockValues = 128;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate IntPtr GetTraits(int type);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void ToFloat([In] byte[] source, [Out] float[] destination, long count);

    // Pinned ggml/include/ggml.h: ggml_type_traits on the Windows ARM64 ABI.
    // The C bool occupies one byte; alignment places to_float at offset 40.
    [StructLayout(LayoutKind.Sequential)]
    private record struct TypeTraits {
        public IntPtr Name;
        public long BlockSize;
        public long Interleave;
        public UIntPtr TypeSize;
        [MarshalAs(UnmanagedType.I1)] public bool Quantized;
        public IntPtr ToFloat;
        public IntPtr FromFloat;
    }

    /// <summary>Checks synthetic blocks against both actual native dequantizers.</summary>
    /// <param name="runtimeDirectory">Fully verified pinned runtime directory; use only inside an owned child.</param>
    /// <returns>The number of synthetic blocks whose FP32 bits agree.</returns>
    /// <remarks>This proves neither activation quantization, logits, nor performance equivalence.</remarks>
    public static int Run(string runtimeDirectory) {
        ArgumentException.ThrowIfNullOrWhiteSpace(runtimeDirectory);
        if (!OperatingSystem.IsWindowsVersionAtLeast(10) || RuntimeInformation.ProcessArchitecture != Architecture.Arm64 || IntPtr.Size != 8)
            throw new PlatformNotSupportedException("The pinned native codec probe requires Windows ARM64.");
        if (!Path.IsPathFullyQualified(runtimeDirectory) || runtimeDirectory.Contains('%', StringComparison.Ordinal) ||
            !string.Equals(Path.GetFileName(Path.TrimEndingDirectorySeparator(runtimeDirectory)), RuntimeCommit, StringComparison.Ordinal))
            throw new ArgumentException("The runtime must be the expanded, pinned commit directory.", nameof(runtimeDirectory));
        if (Marshal.SizeOf<TypeTraits>() != 56 || Marshal.OffsetOf<TypeTraits>(nameof(TypeTraits.ToFloat)).ToInt64() != 40)
            throw new InvalidDataException("The ggml_type_traits ABI is not the reviewed 56-byte layout with to_float at offset 40.");
        string libraryPath = Path.Combine(runtimeDirectory, "ggml-base.dll");
        if (!File.Exists(libraryPath)) throw new FileNotFoundException("The verified native codec library is missing.", libraryPath);
        IntPtr library = NativeLibrary.Load(libraryPath, typeof(PrismCodecProbe).Assembly,
            DllImportSearchPath.UseDllDirectoryForDependencies | DllImportSearchPath.System32);
        try {
            GetTraits getTraits = Marshal.GetDelegateForFunctionPointer<GetTraits>(NativeLibrary.GetExport(library, "ggml_get_type_traits"));
            ToFloat pq2 = Decoder(getTraits, 142, "pq2_0", 34);
            ToFloat ptq1 = Decoder(getTraits, 143, "ptq1_0", 28);
            return CheckVectors(pq2, ptq1);
        } finally { NativeLibrary.Free(library); }
    }

    private static ToFloat Decoder(GetTraits getTraits, int type, string expectedName, ulong expectedBytes) {
        ArgumentNullException.ThrowIfNull(getTraits);
        IntPtr pointer = getTraits(type);
        if (pointer == IntPtr.Zero) throw new InvalidDataException("The pinned native type traits are missing.");
        TypeTraits traits = Marshal.PtrToStructure<TypeTraits>(pointer);
        if (traits.Name == IntPtr.Zero || !string.Equals(Marshal.PtrToStringUTF8(traits.Name), expectedName, StringComparison.Ordinal) ||
            traits.BlockSize != BlockValues || traits.Interleave != 0 || traits.TypeSize.ToUInt64() != expectedBytes || !traits.Quantized || traits.ToFloat == IntPtr.Zero)
            throw new InvalidDataException("The pinned native codec type contract changed.");
        return Marshal.GetDelegateForFunctionPointer<ToFloat>(traits.ToFloat);
    }

    private static byte[] Block(ushort scale) {
        byte[] block = new byte[34];
        Array.Fill<byte>(block, 0x55); // Code one represents zero in every position.
        block[0] = (byte)(scale & 255);
        block[1] = (byte)(scale >> 8);
        return block;
    }

    private static void Set(byte[] block, int index, int code) {
        ArgumentNullException.ThrowIfNull(block);
        if (block.Length != 34) throw new InvalidDataException("The synthetic PQ2 block is out of bounds.");
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        ArgumentOutOfRangeException.ThrowIfGreaterThanOrEqual(index, BlockValues);
        ArgumentOutOfRangeException.ThrowIfNegative(code);
        ArgumentOutOfRangeException.ThrowIfGreaterThan(code, 2);
        int offset = 2 + index / 4;
        int shift = 2 * (index % 4);
        block[offset] = (byte)((block[offset] & ~(3 << shift)) | (code << shift));
    }

    private static void Compare(byte[] block, ToFloat pq2, ToFloat ptq1) {
        ArgumentNullException.ThrowIfNull(block);
        ArgumentNullException.ThrowIfNull(pq2);
        ArgumentNullException.ThrowIfNull(ptq1);
        if (block.Length != 34) throw new InvalidDataException("The synthetic PQ2 block is out of bounds.");
        byte[] packed = Pq2Ptq1Codec.EncodeBlock(block);
        if (packed.Length != 28 || !block.AsSpan().SequenceEqual(Pq2Ptq1Codec.DecodeBlock(packed)))
            throw new InvalidDataException("The codec changed a synthetic PQ2 symbol or scale bit.");
        float[] original = new float[BlockValues];
        float[] repacked = new float[BlockValues];
        pq2(block, original, BlockValues);
        ptq1(packed, repacked, BlockValues);
        float scale = (float)BitConverter.UInt16BitsToHalf((ushort)(block[0] | (block[1] << 8)));
        for (int index = 0; index < BlockValues; index++) {
            int code = (block[2 + index / 4] >> (2 * (index % 4))) & 3;
            int expected = BitConverter.SingleToInt32Bits(scale * (code - 1));
            if (BitConverter.SingleToInt32Bits(original[index]) != expected || BitConverter.SingleToInt32Bits(repacked[index]) != expected)
                throw new InvalidDataException("Native PQ2/PTQ1 dequantization changed an expected FP32 bit, including signed zero.");
        }
    }

    [SuppressMessage("Security", "CA5394:Do not use insecure randomness", Justification = "A fixed seed generates reproducible synthetic codec vectors, never secrets.")]
    private static int CheckVectors(ToFloat pq2, ToFloat ptq1) {
        ArgumentNullException.ThrowIfNull(pq2);
        ArgumentNullException.ThrowIfNull(ptq1);
        int count = 0;
        ushort[] scales = [0x3c00, 0xbc00, 0x0001, 0x8001, 0x03ff, 0x83ff, 0x0400, 0x8400, 0x7bff, 0xfbff, 0x3555, 0xb555];
        foreach (ushort scale in scales) {
            for (int code = 0; code < 3; code++) {
                byte[] block = Block(scale);
                for (int index = 0; index < BlockValues; index++) Set(block, index, code);
                Compare(block, pq2, ptq1); count++;
            }
        }
        for (int index = 0; index < BlockValues; index++) {
            for (int code = 0; code < 3; code++) {
                byte[] block = Block(0x3555);
                Set(block, index, code);
                Compare(block, pq2, ptq1); count++;
            }
        }
        for (int lane = 0; lane < 24; lane++) {
            for (int word = 0; word < 243; word++) {
                byte[] block = Block(0xbc00);
                int first = lane < 16 ? lane : 80 + lane - 16;
                int stride = lane < 16 ? 16 : 8;
                int remaining = word;
                for (int trit = 4; trit >= 0; trit--) {
                    Set(block, first + trit * stride, remaining % 3); remaining /= 3;
                }
                Compare(block, pq2, ptq1); count++;
            }
        }
        for (int lane = 0; lane < 2; lane++) {
            for (int word = 0; word < 81; word++) {
                byte[] block = Block(0x0001);
                int remaining = word;
                for (int trit = 3; trit >= 0; trit--) {
                    Set(block, 120 + lane + trit * 2, remaining % 3); remaining /= 3;
                }
                Compare(block, pq2, ptq1); count++;
            }
        }
        var random = new Random(7310787);
        for (int sample = 0; sample < 256; sample++) {
            byte[] block = Block((ushort)(sample % 2 == 0 ? 0x3c00 : 0xb555));
            for (int index = 0; index < BlockValues; index++) Set(block, index, random.Next(3));
            Compare(block, pq2, ptq1); count++;
        }
        if (count != 6670) throw new InvalidDataException("The fixed native codec vector count changed.");
        return count;
    }
}
}
