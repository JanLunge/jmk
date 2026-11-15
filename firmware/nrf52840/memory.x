/* nRF52840 behind the Adafruit nRF52 bootloader, without a SoftDevice.
   firmware/nrf52840/README.md explains each boundary and gives sources.

   0x00000000  MBR (4 KiB), must stay
   0x00001000  this firmware
   0x000EA000  saved configuration (32 KiB), see flash.rs
   0x000F2000  Bluetooth bonds (8 KiB), see flash.rs
   0x000F4000  bootloader and its settings

   The MBR keeps the address it forwards interrupts to in the first 8 bytes
   of RAM. */
MEMORY {
    FLASH : ORIGIN = 0x00001000, LENGTH = 0x000EA000 - 0x00001000
    RAM   : ORIGIN = 0x20000008, LENGTH = 256K - 8
}
