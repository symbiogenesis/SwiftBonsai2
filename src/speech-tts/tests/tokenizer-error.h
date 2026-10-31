// Request-free model-free logging stand-in; no tokenizer behavior lives here.
#pragma once
#include <stdexcept>
enum qt_log_level { QT_LOG_ERROR, QT_LOG_INFO, QT_LOG_WARN };
inline void qt_log(qt_log_level, const char *, ...) {}
[[noreturn]] inline void qt_throw(const char * message) { throw std::runtime_error(message); }
