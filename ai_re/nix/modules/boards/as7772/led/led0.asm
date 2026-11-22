;
; AS7772-32X front-panel port LEDs, LED processor 0 (cages 17-32).
; Written for this repository (not the factory program); assembled with the
; OpenBCM SDK's tools/led/tools/ledasm.c. devices/edge-core-as7772-32x/leds.md.
;
; Output: 16 cages x 14 bits, in cage order. Per cage:
;   H1 H0            H1 = 1: the cage is one 4-lane port (40G/100G)
;   C1 C0 ON  x 4    one group per lane; C1 = 1: 25G/100G colour
; 100G cage, link up:  1 0  1 0 1  0 0 0  0 0 0  0 0 0   (the factory "blue")
; This program shows every cage as one 100G port: lane 0 carries the lamp.
;
; Inputs: link = bit 0 of data RAM (PORTDATA + index), written by the SDK
; diag shell's linkscan callback ("led auto on"); frames seen = the RX/TX
; port status bits of the same index. Index = the SDK's LED port number:
; processor 0: lane - 1 (lanes 1-32), lane - 65 (lanes 97-128);
; processor 1: lane - 33 (lanes 33-96). The SDK's own port-order remap
; (written by "init all") gives exactly this order: no remap of ours.
;
; Lamp: off without link, on with link, blinking (4 ticks on, 4 off) while
; frames pass.

PORTDATA        equ     0xa0    ; link bytes (SDK), one per index
TICK            equ     0xf0    ; frame counter (data RAM 0x80-0xff is zeroed by "led start")
BLINK_BIT       equ     2       ; TICK bit that gates the lamp while frames pass
NBITS           equ     224     ; 16 cages x 14 bits

RX              equ     0x0     ; port status: frame received
TX              equ     0x1     ; port status: frame sent
ZERO            equ     0xE     ; port status: constant 0
ONE             equ     0xF     ; port status: constant 1
COLOUR          equ     ONE     ; C1 of lane 0 (ONE: 25G/100G, ZERO: 10G/40G); after ONE: ledasm has no forward equ

update:
        ld      a,32
        call    cage            ; cage 17: lanes 97-100
        ld      a,36
        call    cage            ; cage 18: lanes 101-104
        ld      a,40
        call    cage            ; cage 19: lanes 105-108
        ld      a,44
        call    cage            ; cage 20: lanes 109-112
        ld      a,16
        call    cage            ; cage 21: lanes 17-20
        ld      a,20
        call    cage            ; cage 22: lanes 21-24
        ld      a,24
        call    cage            ; cage 23: lanes 25-28
        ld      a,28
        call    cage            ; cage 24: lanes 29-32
        ld      a,48
        call    cage            ; cage 25: lanes 113-116
        ld      a,52
        call    cage            ; cage 26: lanes 117-120
        ld      a,56
        call    cage            ; cage 27: lanes 121-124
        ld      a,60
        call    cage            ; cage 28: lanes 125-128
        ld      a,0
        call    cage            ; cage 29: lanes 1-4
        ld      a,4
        call    cage            ; cage 30: lanes 5-8
        ld      a,8
        call    cage            ; cage 31: lanes 9-12
        ld      a,12
        call    cage            ; cage 32: lanes 13-16
        inc     (TICK)
        send    NBITS

; one cage; a = index of its lane 0
cage:
        pushst  ONE             ; H1
        pack
        pushst  ZERO            ; H0
        pack
        pushst  COLOUR          ; C1
        pack
        pushst  ZERO            ; C0
        pack
        call    lamp            ; ON
        ld      b,9             ; lanes 1-3: nothing
pad:
        pushst  ZERO
        pack
        dec     b
        jnz     pad
        ret

; one lamp bit for index a
lamp:
        ld      b,PORTDATA
        add     b,a
        ld      b,(b)
        tst     b,0
        jnc     dark            ; no link
        port    a
        pushst  RX
        pushst  TX
        tor
        pop
        jnc     lit             ; link, no frames this tick
        ld      b,(TICK)
        tst     b,BLINK_BIT
        jc      dark
lit:
        pushst  ONE
        pack
        ret
dark:
        pushst  ZERO
        pack
        ret
