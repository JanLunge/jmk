/* Assumes 2 MB of flash, the smallest an RP2350 comes with (the RP2354 has
   2 MB built in; boards with a separate flash chip carry 4 MB or more, of
   which only the first 2 MB are used). The last 32 KiB of those 2 MB hold
   the saved configuration (see STORAGE_OFFSET). */
MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K - 32K
    RAM   : ORIGIN = 0x20000000, LENGTH = 512K
}

SECTIONS {
    /* What tells the boot ROM that this is a program for the Arm cores. It
       must lie in the first 4 KiB of flash, so it follows the vector table. */
    .start_block : ALIGN(4)
    {
        KEEP(*(.start_block));
    } > FLASH
} INSERT AFTER .vector_table;

/* The code starts after that block. */
_stext = ADDR(.start_block) + SIZEOF(.start_block);
