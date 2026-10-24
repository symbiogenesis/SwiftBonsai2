cmake_minimum_required(VERSION 3.21)
project(bonsai_prism_upstream_reference LANGUAGES C CXX)

if(NOT IS_DIRECTORY "${PRISM_SOURCE_DIR}/tests" OR NOT EXISTS "${PRISM_SOURCE_DIR}/ggml/CMakeLists.txt")
    message(FATAL_ERROR "PRISM_SOURCE_DIR must contain the verified pinned Prism source tree")
endif()

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Keep the original test translation units and their assertions in Release.
# Some upstream files undefine NDEBUG themselves; others rely on build policy.
if(MSVC)
    add_compile_options(/UNDEBUG /permissive- /sdl)
else()
    add_compile_options(-UNDEBUG)
endif()

set(BUILD_SHARED_LIBS OFF CACHE BOOL "Static reference libraries" FORCE)
set(FETCHCONTENT_FULLY_DISCONNECTED ON CACHE BOOL "No configure-time downloads" FORCE)
set(LLAMA_BUILD_COMMON ON CACHE BOOL "Original test dependencies" FORCE)
set(LLAMA_BUILD_TESTS ON CACHE BOOL "Import the unmodified upstream test definitions" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "No example programs" FORCE)
set(LLAMA_BUILD_TOOLS ON CACHE BOOL "Define dependencies referenced by upstream tests" FORCE)
set(LLAMA_BUILD_SERVER ON CACHE BOOL "Define server-context referenced by upstream test-chat" FORCE)
set(LLAMA_BUILD_APP OFF CACHE BOOL "No desktop application" FORCE)
set(LLAMA_BUILD_UI OFF CACHE BOOL "No UI build" FORCE)
set(LLAMA_USE_PREBUILT_UI OFF CACHE BOOL "No UI download" FORCE)
set(LLAMA_OPENSSL OFF CACHE BOOL "No TLS dependency" FORCE)
set(LLAMA_BUILD_BORINGSSL OFF CACHE BOOL "No TLS download" FORCE)
set(LLAMA_BUILD_LIBRESSL OFF CACHE BOOL "No TLS download" FORCE)
set(LLAMA_SUBPROCESS OFF CACHE BOOL "No subprocess feature" FORCE)
set(LLAMA_LLGUIDANCE OFF CACHE BOOL "No external grammar dependency" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "Portable CPU baseline" FORCE)
set(GGML_BACKEND_DL OFF CACHE BOOL "Static CPU backend" FORCE)
set(GGML_CPU_ALL_VARIANTS OFF CACHE BOOL "One CPU backend" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "No OpenMP dependency" FORCE)
set(GGML_CUDA OFF CACHE BOOL "CPU-only reference" FORCE)
set(GGML_METAL OFF CACHE BOOL "CPU-only reference" FORCE)
set(GGML_VULKAN OFF CACHE BOOL "CPU-only reference" FORCE)
set(GGML_OPENCL OFF CACHE BOOL "CPU-only reference" FORCE)
set(GGML_BLAS OFF CACHE BOOL "CPU-only reference" FORCE)
set(GGML_RPC OFF CACHE BOOL "No RPC backend" FORCE)
set(GGML_CCACHE OFF CACHE BOOL "Only an explicit compiler launcher is permitted" FORCE)

# CTest imports every upstream registration, including performance and model
# tests. The PowerShell runner builds and executes only its audited allowlist;
# never run an unfiltered ctest against this tree.
enable_testing()
add_subdirectory("${PRISM_SOURCE_DIR}" prism)
