# Assisted by Claude Code.
#
# ARM GNU Toolchain cross-file for the AZ3166's STM32F412RG (Cortex-M4F) —
# same toolchain and CPU flags as threadx-temp-sensor's Renode path
# (STM32F407, also Cortex-M4F), just for this separate firmware project.

set(CMAKE_SYSTEM_NAME      Generic)
set(CMAKE_SYSTEM_PROCESSOR arm)

set(CMAKE_C_COMPILER   arm-none-eabi-gcc)
set(CMAKE_ASM_COMPILER arm-none-eabi-gcc)

set(CPU_FLAGS "-mcpu=cortex-m4 -mfpu=fpv4-sp-d16 -mfloat-abi=hard -mthumb")

set(CMAKE_C_FLAGS_INIT   "${CPU_FLAGS} -Os -ffunction-sections -fdata-sections")
set(CMAKE_ASM_FLAGS_INIT "${CPU_FLAGS}")

set(CMAKE_EXE_LINKER_FLAGS_INIT
    "-nostartfiles -Wl,--gc-sections -specs=nosys.specs")

# A bare -nostartfiles toolchain has no _start for CMake's own "does the
# compiler work" try_compile check to link against. Compiling a static
# library instead of an executable sidesteps that without needing a real
# linker script just to pass CMake's sanity check.
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

# Search headers/libs only in the sysroot, not on the build host
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
