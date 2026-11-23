# CMake toolchain for 64-bit rs2 programs in C and C++ (docs/CXX64.md):
#
#   cmake -DCMAKE_TOOLCHAIN_FILE=.../userland64/cmake/rs2.cmake ...
#
# Sources are compiled by clang with cc64.sh's flags; cc64.sh links. The
# libraries have to be built first (cc64.sh --libcxx).
set(CMAKE_SYSTEM_NAME RS2)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_LIST_DIR}")
get_filename_component(RS2_USERLAND "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(RS2_CC "${RS2_USERLAND}/c/cc64.sh")

if(DEFINED ENV{CLANG})
  set(RS2_CLANG "$ENV{CLANG}")
else()
  set(RS2_CLANG clang)
endif()
find_program(RS2_CLANG_PATH "${RS2_CLANG}" REQUIRED)
set(CMAKE_C_COMPILER "${RS2_CLANG_PATH}")
set(CMAKE_CXX_COMPILER "${RS2_CLANG_PATH}")
set(CMAKE_ASM_COMPILER "${RS2_CLANG_PATH}")
foreach(lang C CXX ASM)
  set(CMAKE_${lang}_COMPILER_TARGET x86_64-unknown-none-elf)
  set(CMAKE_${lang}_COMPILER_FORCED TRUE)
endforeach()

# clang's own headers (immintrin.h) after ours.
execute_process(COMMAND "${RS2_CLANG_PATH}" -print-resource-dir OUTPUT_VARIABLE resource OUTPUT_STRIP_TRAILING_WHITESPACE)

# cc64.sh --flags prints what it compiles with.
foreach(lang C CXX)
  execute_process(COMMAND "${RS2_CC}" --flags ${lang} OUTPUT_VARIABLE flags OUTPUT_STRIP_TRAILING_WHITESPACE
                  COMMAND_ERROR_IS_FATAL ANY)
  set(CMAKE_${lang}_FLAGS_INIT "${flags} -idirafter ${resource}/include")
endforeach()
set(CMAKE_CXX_STANDARD_LIBRARIES "")
execute_process(COMMAND rustc --print sysroot OUTPUT_VARIABLE sysroot OUTPUT_STRIP_TRAILING_WHITESPACE)
file(GLOB CMAKE_AR "${sysroot}/lib/rustlib/*/bin/llvm-ar")
file(GLOB RS2_NM "${sysroot}/lib/rustlib/*/bin/llvm-nm")
set(CMAKE_NM "${RS2_NM}")
set(CMAKE_RANLIB "${CMAKE_COMMAND};-E;true" CACHE STRING "")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
