/*
 * AZ3166 ThreadX sensor bridge firmware — main.c
 *
 * Runs a single ThreadX thread that polls the onboard LSM6DSL
 * (accelerometer + die temperature) and writes one JSON line per
 * reading to USART6, the UART behind the ST-Link's virtual COM port
 * (same USB cable used to flash/debug the board).
 *
 * Wire format (consumed by services/src/bin/az3166_serial_bridge.rs on
 * the host side):
 *   {"seq":1042,"accel_mg":[12.0,-980.0,34.0],"die_temp_c":27.4,"uptime_ms":58213}
 *
 * This firmware does not talk to NetX Duo or uProtocol directly — it
 * only needs to get sensor bytes off the chip. The host-side Rust bridge
 * is what republishes them onto uProtocol, per the project's Golden Rule
 * (guardian.rs stays hardware-agnostic).
 */

#include <tx_api.h>
#include <stdio.h>
#include <string.h>

#include "board_init.h"
#include "sensor.h"

#define SENSOR_THREAD_STACK_SIZE 4096
#define SENSOR_POLL_TICKS 100 /* ThreadX tick is 100 Hz -> ~1 reading/second */

static TX_THREAD sensor_thread;
static UCHAR sensor_thread_stack[SENSOR_THREAD_STACK_SIZE];

static void uart_write_line(const char *line)
{
    HAL_UART_Transmit(&UartHandle, (uint8_t *)line, (uint16_t)strlen(line), 1000);
}

static void sensor_thread_entry(ULONG arg)
{
    (void)arg;
    uint32_t seq = 0;

    if (SENSOR_OK != lsm6dsl_config())
    {
        uart_write_line("{\"error\":\"lsm6dsl_config_failed\"}\r\n");
    }

    for (;;)
    {
        lsm6dsl_data_t reading = lsm6dsl_data_read();

        /* ThreadX has no RTC; this is board-relative uptime, matching
         * the SYSTICK_CYCLES 100 Hz tick configured in
         * tx_initialize_low_level.S (10 ms per tick). */
        unsigned long long uptime_ms = (unsigned long long)tx_time_get() * 10ULL;

        char line[160];
        snprintf(line, sizeof(line),
            "{\"seq\":%lu,\"accel_mg\":[%.1f,%.1f,%.1f],\"die_temp_c\":%.1f,\"uptime_ms\":%llu}\r\n",
            (unsigned long)seq,
            (double)reading.acceleration_mg[0],
            (double)reading.acceleration_mg[1],
            (double)reading.acceleration_mg[2],
            (double)reading.temperature_degC,
            uptime_ms);
        uart_write_line(line);

        seq++;
        tx_thread_sleep(SENSOR_POLL_TICKS);
    }
}

/* Called by ThreadX after tx_kernel_enter(); this is where application
 * threads get created. */
void tx_application_define(void *first_unused_memory)
{
    (void)first_unused_memory;

    UINT status = tx_thread_create(
        &sensor_thread, "az3166-sensor", sensor_thread_entry, 0,
        sensor_thread_stack, SENSOR_THREAD_STACK_SIZE,
        16, 16, TX_NO_TIME_SLICE, TX_AUTO_START);

    if (status != TX_SUCCESS)
    {
        uart_write_line("{\"error\":\"tx_thread_create_failed\"}\r\n");
    }
}

int main(void)
{
    board_init();
    uart_write_line("=== AZ3166 ThreadX sensor bridge ===\r\n");

    /* Never returns: ThreadX takes over the CPU from here. */
    tx_kernel_enter();
    return 0;
}
