# whisper-rs-sys forwards CMAKE_* environment variables to its CMake build.
# This project-scoped hook leaves other native dependencies unchanged.
if(CMAKE_SYSTEM_NAME STREQUAL "Darwin" AND CMAKE_OSX_ARCHITECTURES STREQUAL "arm64")
    # GGML forces +sve in a runtime probe. On affected Apple Silicon/macOS
    # systems its first ADDVL instruction traps indefinitely rather than
    # terminating with SIGILL. Only run that probe if the native compiler
    # actually enables SVE. DotProd, I8MM and SME detection remain unchanged.
    # https://github.com/ggml-org/llama.cpp/pull/20064
    include(CMakePushCheckState)
    include(CheckCXXSourceCompiles)
    cmake_push_check_state(RESET)
    set(CMAKE_REQUIRED_FLAGS "-mcpu=native")
    check_cxx_source_compiles("
        #ifndef __ARM_FEATURE_SVE
        #error Native target does not enable SVE
        #endif
        int main() { return 0; }
    " FFPLAYOUT_WHISPER_NATIVE_SVE)
    cmake_pop_check_state()

    if(NOT FFPLAYOUT_WHISPER_NATIVE_SVE)
        set(GGML_MACHINE_SUPPORTS_sve FALSE CACHE INTERNAL
            "Skip unsafe forced-SVE runtime probe on Apple Silicon" FORCE)
    endif()
endif()
