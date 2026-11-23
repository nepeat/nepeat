/* SPDX-License-Identifier: GPL-2.0 */
/*
 * Register bank + access recorder for the difftest. Trace format (one event
 * per line; "I" lines are inputs the Rust side replays, all others are outputs
 * both sides must produce identically):
 *   S <name>                       scenario start
 *   I FILL <seed>                  every register word := mock_hash(seed, bar, offset)
 *   I PIN <bar> <off> <val>        reads of that word always return val
 *   I SET <bar> <off> <val>        the hardware changes a word
 *   I CDESC <ring> <idx> w0 w1 w2 w3   the hardware writes a completion descriptor
 *   I CALL <op> [args]             the driver operation that follows
 *   R|W <bar> <off> <val>          32-bit MMIO read / write (bar u=UDMA e=EC m=MAC)
 *   R16|W16 <bar> <off> <val>      16-bit MMIO
 *   CR|CW <off> <val>              PCI config space read / write (bar p)
 *   D <us>                         busy wait
 *   MB                             barrier before a doorbell
 *   DESC <ring> <idx> w0 w1 w2 w3  descriptor ring entry changed during the call
 *   RET <op> ...                   what the operation returned / decided
 */
#ifndef DIFFTEST_MOCK_H
#define DIFFTEST_MOCK_H
#include <stdint.h>
#include <stdio.h>

enum mock_bar { BAR_U, BAR_E, BAR_M, BAR_P, NBARS };

extern FILE *mock_out;
extern uint8_t *mock_bar_mem[NBARS];

void mock_init(void);
void mock_scenario(const char *name);
void mock_fill(uint32_t seed);
void mock_pin(int bar, uint32_t off, uint32_t val);
void mock_set(int bar, uint32_t off, uint32_t val);
uint32_t mock_peek(int bar, uint32_t off);
uint32_t mock_hash(uint32_t seed, int bar, uint32_t off);

/* descriptor rings (normal memory written by the HAL directly) */
int mock_ring_add(const char *name, void *mem, unsigned int entries);
void mock_ring_dump(void);
void mock_rings_clear(void);   /* the driver freed its rings (down); up allocates new ones */
void mock_cdesc(int ring, unsigned int idx, const uint32_t w[4]);
const char *mock_ring_name(int ring);

int mock_pci_read(void *handle, int where, uint32_t *val);
int mock_pci_write(void *handle, int where, uint32_t val);

/* xorshift32 for the scenario generator (inputs only) */
uint32_t rnd(void);
void rnd_seed(uint32_t s);
#endif
