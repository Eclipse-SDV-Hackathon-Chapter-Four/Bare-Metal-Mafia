/*
 * SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
 *
 * AI Disclosure: This file was largely AI-generated. The AI-generated
 * portions are made available under CC0-1.0 and not subject to the
 * project's licence. The human contributor has reviewed and verified
 * that the code is correct.
 *
 * Assisted-by: Anthropic Claude (Sonnet 5)
 *
 * AZ3166 ThreadX sensor bridge firmware — main.c
 *
 * Runs a single ThreadX thread that polls the onboard LSM6DSL
 * (accelerometer + die temperature) and HTS221 (humidity + ambient
 * temperature) and writes one JSON line per reading to USART6, the UART
 * behind the ST-Link's virtual COM port (same USB cable used to
 * flash/debug the board). Both sensors share the same I2C1 bus - no
 * extra wiring, just a second *_config()/*_data_read() pair.
 *
 * Wire format (consumed by services/src/bin/az3166_serial_bridge.rs on
 * the host side):
 *   {"seq":1042,"accel_mg":[12.0,-980.0,34.0],"die_temp_c":27.4,"humidity_pct":41.3,"uptime_ms":58213}
 *
 * humidity_pct comes from HTS221, a separate ambient-air sensor from
 * the LSM6DSL die temperature - this one is a genuine environmental
 * reading, not a chip-self-heating proxy. The Rust bridge currently
 * only forwards die_temp_c into uProtocol/Guardian; humidity_pct is
 * UART/OLED-only for now (not yet wired into the dashboard).
 *
 * This firmware does not talk to NetX Duo or uProtocol directly — it
 * only needs to get sensor bytes off the chip. The host-side Rust bridge
 * is what republishes them onto uProtocol, per the project's Golden Rule
 * (guardian.rs stays hardware-agnostic).
 *
 * The onboard SSD1306 OLED mirrors the same reading plus a few debug
 * values (sequence counter, board uptime, sensor health) directly on the
 * board - handy when there's no serial terminal open. It also mirrors
 * Guardian's own evaluate_state() thresholds (lib.rs: >=32C WARNING,
 * >=40C CRITICAL) as a local label purely for visual debugging; this
 * firmware makes no safety decision of its own - the host-side Guardian
 * is still the only real state machine.
 */

#include <tx_api.h>
#include <stdio.h>
#include <string.h>

#include "board_init.h"
#include "sensor.h"
#include "ssd1306.h"

#define SENSOR_THREAD_STACK_SIZE 4096
#define SENSOR_POLL_TICKS 100 /* ThreadX tick is 100 Hz -> ~1 reading/second */

#define LOCAL_WARNING_THRESHOLD_C 32.0f
#define LOCAL_CRITICAL_THRESHOLD_C 40.0f

static TX_THREAD sensor_thread;
static UCHAR sensor_thread_stack[SENSOR_THREAD_STACK_SIZE];

static void uart_write_line(const char *line)
{
    HAL_UART_Transmit(&UartHandle, (uint8_t *)line, (uint16_t)strlen(line), 1000);
}

static void update_display(const lsm6dsl_data_t *reading, int sensor_ok,
                            const hts221_data_t *humidity, int humidity_ok,
                            uint32_t seq, unsigned long long uptime_ms)
{
    char line[32];

    ssd1306_Fill(Black);

    ssd1306_SetCursor(0, 0);
    ssd1306_WriteString("AZ3166 Guardian", Font_6x8, White);

    if (sensor_ok)
    {
        snprintf(line, sizeof(line), "%.1fC", (double)reading->temperature_degC);
    }
    else
    {
        snprintf(line, sizeof(line), "ERR");
    }
    ssd1306_SetCursor(0, 9);
    ssd1306_WriteString(line, Font_11x18, White);

    if (humidity_ok)
    {
        snprintf(line, sizeof(line), "Hum: %.0f%%", (double)humidity->humidity_perc);
    }
    else
    {
        snprintf(line, sizeof(line), "Hum: ERR");
    }
    ssd1306_SetCursor(0, 28);
    ssd1306_WriteString(line, Font_7x10, White);

    char *state_label = "MONITORING";
    if (!sensor_ok)
    {
        state_label = "NO SENSOR";
    }
    else if (reading->temperature_degC >= LOCAL_CRITICAL_THRESHOLD_C)
    {
        state_label = "CRITICAL";
    }
    else if (reading->temperature_degC >= LOCAL_WARNING_THRESHOLD_C)
    {
        state_label = "WARNING";
    }
    ssd1306_SetCursor(0, 39);
    ssd1306_WriteString(state_label, Font_7x10, White);

    snprintf(line, sizeof(line), "seq=%lu up=%llus", (unsigned long)seq, uptime_ms / 1000ULL);
    ssd1306_SetCursor(0, 51);
    ssd1306_WriteString(line, Font_6x8, White);

    ssd1306_UpdateScreen();
}

static void sensor_thread_entry(ULONG arg)
{
    (void)arg;
    uint32_t seq = 0;
    int sensor_ok = (SENSOR_OK == lsm6dsl_config());
    int humidity_ok = (SENSOR_OK == hts221_config());

    if (!sensor_ok)
    {
        uart_write_line("{\"error\":\"lsm6dsl_config_failed\"}\r\n");
    }
    if (!humidity_ok)
    {
        uart_write_line("{\"error\":\"hts221_config_failed\"}\r\n");
    }

    for (;;)
    {
        lsm6dsl_data_t reading = lsm6dsl_data_read();
        hts221_data_t humidity = humidity_ok ? hts221_data_read() : (hts221_data_t){0};

        /* ThreadX has no RTC; this is board-relative uptime, matching
         * the SYSTICK_CYCLES 100 Hz tick configured in
         * tx_initialize_low_level.S (10 ms per tick). */
        unsigned long long uptime_ms = (unsigned long long)tx_time_get() * 10ULL;

        char line[192];
        snprintf(line, sizeof(line),
            "{\"seq\":%lu,\"accel_mg\":[%.1f,%.1f,%.1f],\"die_temp_c\":%.1f,"
            "\"humidity_pct\":%.1f,\"uptime_ms\":%llu}\r\n",
            (unsigned long)seq,
            (double)reading.acceleration_mg[0],
            (double)reading.acceleration_mg[1],
            (double)reading.acceleration_mg[2],
            (double)reading.temperature_degC,
            humidity_ok ? (double)humidity.humidity_perc : -1.0,
            uptime_ms);
        uart_write_line(line);

        update_display(&reading, sensor_ok, &humidity, humidity_ok, seq, uptime_ms);

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
