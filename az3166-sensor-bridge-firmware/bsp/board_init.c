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
 * board_init.c — trimmed AZ3166 (STM32F412RG) board bring-up for the
 * Guardian Loop sensor bridge firmware.
 *
 * Derived from chheis/challenge-threadx-playRemote's MXChip/AZ3166 BSP
 * (app/board_init.c). The original also brought up an RGB LED and two
 * push buttons; those are still dropped (not needed here), but the
 * onboard SSD1306 OLED is brought up - it shares I2C1 with the LSM6DSL,
 * so no extra wiring/bus setup is needed, just ssd1306_Init() after
 * I2C1_Init(). SystemClock_Config() is copied byte-for-byte from the
 * upstream BSP (26 MHz HSE -> 96 MHz SYSCLK via PLL) since it is the one
 * part of bring-up that must exactly match this specific board's crystal.
 */

#include "board_init.h"
#include "ssd1306.h"

I2C_HandleTypeDef I2cHandle;
UART_HandleTypeDef UartHandle;

#define I2C_ADDRESS 0x30F
#define I2C_SPEEDCLOCK 400000
#define I2C_DUTYCYCLE  I2C_DUTYCYCLE_2
#define I2Cx I2C1

static void SystemClock_Config(void);
static void STM32_Error_Handler(void);
static void I2C1_Init(void);
static void UART_Console_Init(void);

void board_init(void)
{
    HAL_Init();
    SystemClock_Config();
    UART_Console_Init();
    I2C1_Init();
    ssd1306_Init();
}

/* System Clock Configuration: HSE (26 MHz) -> PLL -> SYSCLK 96 MHz. */
static void SystemClock_Config(void)
{
    RCC_ClkInitTypeDef RCC_ClkInitStruct;
    RCC_OscInitTypeDef RCC_OscInitStruct;

    __HAL_RCC_PWR_CLK_ENABLE();
    __HAL_PWR_VOLTAGESCALING_CONFIG(PWR_REGULATOR_VOLTAGE_SCALE1);

    RCC_OscInitStruct.OscillatorType = RCC_OSCILLATORTYPE_HSE | RCC_OSCILLATORTYPE_LSE;
    RCC_OscInitStruct.HSEState       = RCC_HSE_ON;
    RCC_OscInitStruct.LSEState       = RCC_LSE_ON;
    RCC_OscInitStruct.PLL.PLLState   = RCC_PLL_ON;
    RCC_OscInitStruct.PLL.PLLSource  = RCC_PLLSOURCE_HSE;
    RCC_OscInitStruct.PLL.PLLM       = 13;
    RCC_OscInitStruct.PLL.PLLN       = 96;
    RCC_OscInitStruct.PLL.PLLP       = RCC_PLLP_DIV2;
    RCC_OscInitStruct.PLL.PLLQ       = 4;
    RCC_OscInitStruct.PLL.PLLR       = 2;
    if (HAL_RCC_OscConfig(&RCC_OscInitStruct) != HAL_OK)
    {
        STM32_Error_Handler();
    }

    RCC_ClkInitStruct.ClockType =
        (RCC_CLOCKTYPE_SYSCLK | RCC_CLOCKTYPE_HCLK | RCC_CLOCKTYPE_PCLK1 | RCC_CLOCKTYPE_PCLK2);
    RCC_ClkInitStruct.SYSCLKSource   = RCC_SYSCLKSOURCE_PLLCLK;
    RCC_ClkInitStruct.AHBCLKDivider  = RCC_SYSCLK_DIV1;
    RCC_ClkInitStruct.APB1CLKDivider = RCC_HCLK_DIV2;
    RCC_ClkInitStruct.APB2CLKDivider = RCC_HCLK_DIV1;
    if (HAL_RCC_ClockConfig(&RCC_ClkInitStruct, FLASH_LATENCY_3) != HAL_OK)
    {
        STM32_Error_Handler();
    }
}

static void STM32_Error_Handler(void)
{
    while (1)
    {
    }
}

static void I2C1_Init(void)
{
    I2cHandle.Instance             = I2Cx;
    I2cHandle.Init.ClockSpeed      = I2C_SPEEDCLOCK;
    I2cHandle.Init.DutyCycle       = I2C_DUTYCYCLE;
    I2cHandle.Init.OwnAddress1     = I2C_ADDRESS;
    I2cHandle.Init.AddressingMode  = I2C_ADDRESSINGMODE_10BIT;
    I2cHandle.Init.DualAddressMode = I2C_DUALADDRESS_DISABLE;
    I2cHandle.Init.OwnAddress2     = 0xFF;
    I2cHandle.Init.GeneralCallMode = I2C_GENERALCALL_DISABLE;
    I2cHandle.Init.NoStretchMode   = I2C_NOSTRETCH_DISABLE;

    if (HAL_I2C_Init(&I2cHandle) != HAL_OK)
    {
        STM32_Error_Handler();
    }
}

/* USART6 is the ST-Link virtual COM port (the same USB cable used to
 * flash/debug the board carries this UART). */
static void UART_Console_Init(void)
{
    UartHandle.Instance          = USART6;
    UartHandle.Init.BaudRate     = 115200;
    UartHandle.Init.WordLength   = UART_WORDLENGTH_8B;
    UartHandle.Init.StopBits     = UART_STOPBITS_1;
    UartHandle.Init.Parity       = UART_PARITY_NONE;
    UartHandle.Init.HwFlowCtl    = UART_HWCONTROL_NONE;
    UartHandle.Init.Mode         = UART_MODE_TX_RX;
    UartHandle.Init.OverSampling = UART_OVERSAMPLING_16;
    if (HAL_UART_Init(&UartHandle) != HAL_OK)
    {
        STM32_Error_Handler();
    }
}
