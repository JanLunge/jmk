/* Assumes 2 MB of flash, the smallest fitted to common RP2040 boards.
   The last 32 KiB hold the saved configuration (see STORAGE_OFFSET). */
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100 - 32K
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
