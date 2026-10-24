cmake_minimum_required(VERSION 3.24)
project(BonsaiPrismUpstreamCommonAdapter LANGUAGES C CXX)

if(NOT DEFINED PRISM_SOURCE_DIR)
  message(FATAL_ERROR "PRISM_SOURCE_DIR must name the verified, unmodified pinned Prism source")
endif()
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON ON CACHE BOOL "" FORCE)
set(LLAMA_BUILD_MTMD ON CACHE BOOL "" FORCE)
foreach(option BUILD_TESTING LLAMA_BUILD_TESTS LLAMA_BUILD_TOOLS LLAMA_BUILD_SERVER
    LLAMA_BUILD_APP LLAMA_BUILD_EXAMPLES LLAMA_BUILD_UI MTMD_VIDEO
    LLAMA_USE_PREBUILT_UI LLAMA_OPENSSL LLAMA_LLGUIDANCE LLAMA_SUBPROCESS
    LLAMA_DSPARK_MARKOV_BLAS LLAMA_DSPARK_MARKOV_CUDA LLAMA_DSPARK_MARKOV_METAL
    GGML_CCACHE GGML_BACKEND_DL GGML_NATIVE GGML_CPU_ALL_VARIANTS GGML_OPENMP
    GGML_CUDA GGML_METAL GGML_ACCELERATE GGML_BLAS GGML_LLAMAFILE)
  set(${option} OFF CACHE BOOL "" FORCE)
endforeach()
# Configure only vendored dependencies; an adapter build must not fetch packages.
set(FETCHCONTENT_FULLY_DISCONNECTED ON CACHE BOOL "" FORCE)
set(FETCHCONTENT_UPDATES_DISCONNECTED ON CACHE BOOL "" FORCE)
add_subdirectory("${PRISM_SOURCE_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/prism")

add_executable(prism-upstream-common-bridge bridge.cpp jinja.hpp mtmd.hpp grammar_parser.hpp reasoning_budget.hpp tensor_graph.hpp grammar.hpp codec.hpp peg.hpp concurrency.hpp arena_graph.hpp batch_allocator.hpp allocation.hpp mtmd_chunks.hpp sampling.hpp gguf.hpp chat_templates.hpp chat_parser.hpp chat_auto.hpp vocab.hpp)
target_include_directories(prism-upstream-common-bridge PRIVATE "${PRISM_SOURCE_DIR}/src" "${PRISM_SOURCE_DIR}" "${PRISM_SOURCE_DIR}/ggml/src")
target_link_libraries(prism-upstream-common-bridge PRIVATE llama-common mtmd)
