;
; AS7772-32X port LED test pattern (by-eye check of colours and wiring):
; the same 14 bits for all 16 cages of one LED processor, no inputs.
; Ours, not the factory LED_*.hex; build.sh assembles one variant per colour
; by replacing the @...@ values. devices/edge-core-as7772-32x/leds.md.

ZERO            equ     0xE
ONE             equ     0xF
H1              equ     @H1@    ; 1 = cage is one 4-lane port
C1              equ     @C1@    ; colour bit of lane 0 (and of lanes 1-3 when H1 = ZERO)
ON              equ     @ON@    ; lamp bit of lane 0 (and of lanes 1-3 when H1 = ZERO)
REST_C1         equ     @RC1@
REST_ON         equ     @RON@

update:
        ld      a,16
cage:
        pushst  H1
        pack
        pushst  ZERO
        pack
        pushst  C1
        pack
        pushst  ZERO
        pack
        pushst  ON
        pack
        ld      b,3
rest:
        pushst  REST_C1
        pack
        pushst  ZERO
        pack
        pushst  REST_ON
        pack
        dec     b
        jnz     rest
        dec     a
        jnz     cage
        send    224
