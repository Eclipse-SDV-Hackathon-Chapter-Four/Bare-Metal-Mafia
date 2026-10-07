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
 * board_init.h — trimmed AZ3166 (STM32F412RG) board bring-up for the
 * Guardian Loop sensor bridge firmware.
 *
 * Derived from chheis/challenge-threadx-playRemote's MXChip/AZ3166 BSP
 * (app/board_init.c/.h). OLED (ssd1306), RGB LED, and push-button handling
 * were removed on purpose — this firmware only needs clocks, the USART6
 * console UART (ST-Link virtual COM port), and the I2C1 bus the LSM6DSL
 * sits on.
 */

#ifndef _BOARD_INIT_H
#define _BOARD_INIT_H

#include "stm32f4xx_hal.h"

extern UART_HandleTypeDef UartHandle;
extern I2C_HandleTypeDef I2cHandle;

void board_init(void);

#endif /* _BOARD_INIT_H */
