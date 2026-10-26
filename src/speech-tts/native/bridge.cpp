// The Rust boundary owns a single context and copies each returned audio buffer
// before releasing it with the same allocator that produced it.
#include "qwen.h"

#include <cstring>

namespace {
void discard_log(enum qt_log_level, const char *, void *) {}
}

extern "C" {

void * bonsai_tts_load(const char * model, const char * codec) noexcept {
    try {
        // A model, text, voice description or upstream exception must not leak
        // into diagnostics. The Rust caller reports value-free failures.
        qt_log_set(discard_log, nullptr);
        qt_init_params params{};
        qt_init_default_params(&params);
        params.talker_path = model;
        params.codec_path = codec;
        params.device = "CPU";
        params.use_fa = false;
        params.max_batch = 1;
        params.codec_chunk_sec = 8.0f;
        qt_context * context = qt_init(&params);
        if (context == nullptr) {
            return nullptr;
        }
        const char * type = qt_model_type(context);
        if (type == nullptr || std::strcmp(type, "voice_design") != 0) {
            qt_free(context);
            return nullptr;
        }
        return context;
    } catch (...) {
        return nullptr;
    }
}

void bonsai_tts_free(void * context) noexcept {
    qt_free(static_cast<qt_context *>(context));
}

int bonsai_tts_synthesize(void * context, const char * text, const char * voice,
                          qt_audio * audio) noexcept {
    try {
        qt_tts_params params{};
        qt_tts_default_params(&params);
        params.text = text;
        params.lang = "auto";
        params.instruct = voice;
        params.max_new_tokens = 1500;
        return qt_synthesize(static_cast<qt_context *>(context), &params, audio);
    } catch (...) {
        qt_audio_free(audio);
        return QT_STATUS_GENERATE_FAILED;
    }
}

void bonsai_tts_audio_free(qt_audio * audio) noexcept {
    qt_audio_free(audio);
}

}
