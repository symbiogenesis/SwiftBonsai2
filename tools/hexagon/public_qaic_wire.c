/* Host-only public QAIC wire capture. No FastRPC or SDK library is linked. */
#include <inttypes.h>
#include <limits.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "htp_iface.h"
#include "HAP_debug.h"

enum { CAPTURE_CALLS = 2, CAPTURE_BYTES = 64 };

struct capture {
    remote_handle64 handle;
    uint32_t scalars;
    int args_null;
    size_t primary_bytes;
    unsigned char primary[CAPTURE_BYTES];
};

static struct capture captures[CAPTURE_CALLS];
static unsigned int call_count;
static unsigned int unexpected_calls;

int remote_handle64_open(const char *name, remote_handle64 *handle) {
    (void)name;
    (void)handle;
    ++unexpected_calls;
    return -77;
}

int remote_handle64_close(remote_handle64 handle) {
    (void)handle;
    ++unexpected_calls;
    return -77;
}

int remote_handle64_invoke(remote_handle64 handle, uint32_t scalars, remote_arg *args) {
    unsigned int index = call_count++;
    if (index >= CAPTURE_CALLS) {
        ++unexpected_calls;
        return -77;
    }
    struct capture *capture = &captures[index];
    capture->handle = handle;
    capture->scalars = scalars;
    capture->args_null = args == NULL;
    if (index == 0) {
        if (REMOTE_SCALARS_METHOD(scalars) != 2 ||
            REMOTE_SCALARS_INBUFS(scalars) != 1 ||
            REMOTE_SCALARS_OUTBUFS(scalars) != 0 ||
            REMOTE_SCALARS_INHANDLES(scalars) != 0 ||
            REMOTE_SCALARS_OUTHANDLES(scalars) != 0 || args == NULL) {
            ++unexpected_calls;
            return -77;
        }
        capture->primary_bytes = args[0].buf.nLen;
        if (capture->primary_bytes > CAPTURE_BYTES || args[0].buf.pv == NULL) {
            ++unexpected_calls;
            return -77;
        }
        memcpy(capture->primary, args[0].buf.pv, capture->primary_bytes);
    } else if (REMOTE_SCALARS_METHOD(scalars) != 3 ||
               REMOTE_SCALARS_LENGTH(scalars) != 0 || args != NULL) {
        ++unexpected_calls;
        return -77;
    }
    return 0;
}

/* Generated error logging must never be needed for this successful mock cycle. */
void HAP_debug_v2(int level, const char *file, int line, const char *format, ...) {
    (void)level; (void)file; (void)line; (void)format;
    ++unexpected_calls;
}

void HAP_debug_runtime(int level, const char *file, int line, const char *format, ...) {
    (void)level; (void)file; (void)line; (void)format;
    ++unexpected_calls;
}

void HAP_debug(const char *message, int level, const char *file, int line) {
    (void)message; (void)level; (void)file; (void)line;
    ++unexpected_calls;
}

static void print_capture(const struct capture *capture) {
    printf("{\"scalars\":%" PRIu32 ",\"method\":%" PRIu32
           ",\"inputBuffers\":%" PRIu32 ",\"outputBuffers\":%" PRIu32
           ",\"inputHandles\":%" PRIu32 ",\"outputHandles\":%" PRIu32
           ",\"argsNull\":%s,\"primaryBytes\":%zu,\"primaryHex\":\"",
           capture->scalars, (uint32_t)REMOTE_SCALARS_METHOD(capture->scalars),
           (uint32_t)REMOTE_SCALARS_INBUFS(capture->scalars),
           (uint32_t)REMOTE_SCALARS_OUTBUFS(capture->scalars),
           (uint32_t)REMOTE_SCALARS_INHANDLES(capture->scalars),
           (uint32_t)REMOTE_SCALARS_OUTHANDLES(capture->scalars),
           capture->args_null ? "true" : "false", capture->primary_bytes);
    if (capture->primary_bytes <= CAPTURE_BYTES) {
        for (size_t index = 0; index < capture->primary_bytes; ++index) {
            printf("%02x", (unsigned int)capture->primary[index]);
        }
    }
    printf("\"}");
}

int main(void) {
    _Static_assert(sizeof(uint32_t) == 4, "uint32 width");
    _Static_assert(sizeof(uint64_t) == 8, "uint64 width");
    const remote_handle64 handle = UINT64_C(0x1020304050607080);
    int start_result = htp_iface_start(handle,
        UINT32_C(0x11223344), UINT64_C(0x2132435465760718),
        UINT32_C(0x31425364), UINT32_C(0x41526374),
        UINT64_C(0x5162730415263748), UINT64_C(0x6172031425364758),
        UINT32_C(0x71820314));
    int stop_result = htp_iface_stop(handle);
    const uint32_t endian = 1;
    const int little_endian = *(const unsigned char *)&endian == 1;
    if (captures[0].handle != handle || captures[1].handle != handle) {
        ++unexpected_calls;
    }
    printf("{\"schemaVersion\":1,\"kind\":\"public-qaic-mock-wire-capture\","
           "\"testVectorId\":\"htp-start-distinct-positive-v1\","
           "\"handleHex\":\"%016" PRIx64 "\",\"start\":", (uint64_t)captures[0].handle);
    print_capture(&captures[0]);
    printf(",\"stop\":");
    print_capture(&captures[1]);
    printf(",\"callCount\":%u,\"unexpectedCalls\":%u,"
           "\"returnCodes\":{\"start\":%d,\"stop\":%d},"
           "\"layout\":{\"charBits\":%d,\"pointerBytes\":%zu,\"sizeTBytes\":%zu,"
           "\"uint32Bytes\":%zu,\"uint64Bytes\":%zu,\"remoteHandle64Bytes\":%zu,"
           "\"remoteBufferBytes\":%zu,\"remoteBufferAlignment\":%zu,"
           "\"remoteBufferPointerOffset\":%zu,\"remoteBufferLengthOffset\":%zu,"
           "\"remoteArgBytes\":%zu,\"remoteArgAlignment\":%zu,\"littleEndian\":%s}}\n",
           call_count, unexpected_calls, start_result, stop_result,
           CHAR_BIT, sizeof(void *), sizeof(size_t), sizeof(uint32_t), sizeof(uint64_t),
           sizeof(remote_handle64), sizeof(remote_buf), _Alignof(remote_buf),
           offsetof(remote_buf, pv), offsetof(remote_buf, nLen),
           sizeof(remote_arg), _Alignof(remote_arg), little_endian ? "true" : "false");
    return call_count == CAPTURE_CALLS && unexpected_calls == 0 &&
           start_result == 0 && stop_result == 0 ? 0 : 1;
}
