# patchelfdd

A utility to set `DT_RUNPATH` and the interpreter of an elf binary.
In contrast to [patchelf](https://github.com/NixOS/patchelf), it does not try to move or resize existing sections.

Instead it searches for a symbol in `.dynstr`, that is likely to be unused. Currently that can be either
- `__gmon_start__`
- `_ITM_deregisterTMCloneTable`

It will corrupt this symbol and replace it with a new `DT_RUNPATH`.

## Motivation

When using [patchelf](https://github.com/NixOS/patchelf), the elf will be modified quite a bit.
Existing sections like `.dynstr` and `.dynamic` get appended as a whole to the end of the new elf.
Most of the time this is the best solution, since you can append arbitrary entries to the new section.

However, I frequently had the following problems with patchelf:
1. Symbols are not detected by gdb after patching
2. The patched sections of the elf have to be mapped in a new section after the last section of the original elf.
   This offsets the heap and could be unwanted.
3. It increases the size of the binary, which can cause confusion.

I made patchelfdd, to work around those problems.

