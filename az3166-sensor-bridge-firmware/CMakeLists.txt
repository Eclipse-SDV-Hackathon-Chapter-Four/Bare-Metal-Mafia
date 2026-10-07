# AZ3166 (MXChip IoT DevKit, STM32F412RG) ThreadX sensor bridge firmware.
#
# Assisted by Claude Code.
#
# Reads the onboard LSM6DSL (accelerometer + die temperature) and writes
# one JSON line per reading to USART6 (the ST-Link virtual COM port). The
# host-side services/src/bin/az3166_serial_bridge.rs Rust binary reads
# that UART and republishes the data onto uProtocol — this firmware has
# no network/uProtocol code of its own, by design (see ARCHITECTURE.md's
# Golden Rule: guardian.rs and the transport layer stay hardware-agnostic).
#
# BSP (STM32CubeF4 HAL/CMSIS, LSM6DSL driver, startup/linker files) is
# vendored from chheis/challenge-threadx-playRemote's MXChip/AZ3166
# tree — the one repo found with a confirmed-working LSM6DSL driver for
# this exact board. Eclipse ThreadX itself is pulled fresh via
# FetchContent (cortex_m4/gnu port only — no NetX Duo, no SMP).

cmake_minimum_required(VERSION 3.20 FATAL_ERROR)

if(NOT CMAKE_TOOLCHAIN_FILE)
    set(CMAKE_TOOLCHAIN_FILE "${CMAKE_CURRENT_LIST_DIR}/cmake/arm-none-eabi-toolchain.cmake")
endif()

project(az3166_sensor_bridge C ASM)

set(CMAKE_C_STANDARD 99)

list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_LIST_DIR}/cmake)

# ── Eclipse ThreadX (cortex_m4/gnu port only) ───────────────────────────────
include(FetchContent)
set(FETCHCONTENT_QUIET OFF)

set(THREADX_ARCH      "cortex_m4" CACHE STRING "" FORCE)
set(THREADX_TOOLCHAIN "gnu"       CACHE STRING "" FORCE)
set(TX_USER_FILE       "${CMAKE_CURRENT_LIST_DIR}/tx_user.h" CACHE FILEPATH "" FORCE)

FetchContent_Declare(
    threadx
    GIT_REPOSITORY https://github.com/eclipse-threadx/threadx.git
    GIT_TAG        v6.5.2.202603_rel
    GIT_SHALLOW    TRUE
)
FetchContent_MakeAvailable(threadx)

# ── Vendored STM32CubeF4 HAL + CMSIS (board support) ────────────────────────
add_subdirectory(vendor/stm32cubef4)

# ── Firmware executable ─────────────────────────────────────────────────────
add_executable(${PROJECT_NAME}
    startup/system_stm32f4xx.c
    startup/startup_stm32f412rx.s
    startup/tx_initialize_low_level.S
    startup/syscall_stubs.c
    vendor/stm32cubef4/stm32f4xx_hal_msp.c
    vendor/mxchip_bsp/lsm6dsl/Src/lsm6dsl_reg.c
    vendor/mxchip_bsp/lsm6dsl/Src/lsm6dsl_read_data_polling.c
    vendor/mxchip_bsp/ssd1306/ssd1306.c
    vendor/mxchip_bsp/ssd1306/ssd1306_fonts.c
    bsp/board_init.c
    src/main.c
)

target_include_directories(${PROJECT_NAME} PRIVATE
    bsp
    vendor/mxchip_bsp/lsm6dsl/Inc
    vendor/mxchip_bsp/ssd1306
)

target_link_libraries(${PROJECT_NAME} PRIVATE
    azrtos::threadx
    stm32cubef4
)

target_link_options(${PROJECT_NAME} PRIVATE
    -T${CMAKE_CURRENT_LIST_DIR}/linker/stm32f412rg_flash.ld
    -Wl,-Map=$<TARGET_FILE_DIR:${PROJECT_NAME}>/${PROJECT_NAME}.map
)
set_target_properties(${PROJECT_NAME} PROPERTIES SUFFIX ".elf")

# Post-build: produce a flashable .bin — drag-and-drop it onto the
# AZ3166's ST-Link mass-storage drive to flash (no debugger session
# needed for that path; OpenOCD/STM32CubeProgrammer is the fallback for
# iterative debugging, see firmware README once written).
find_program(ARM_OBJCOPY arm-none-eabi-objcopy REQUIRED)
add_custom_command(TARGET ${PROJECT_NAME} POST_BUILD
    COMMAND ${ARM_OBJCOPY} -O binary
            $<TARGET_FILE:${PROJECT_NAME}>
            $<TARGET_FILE_DIR:${PROJECT_NAME}>/${PROJECT_NAME}.bin
    COMMENT "Generating ${PROJECT_NAME}.bin"
    VERBATIM
)
