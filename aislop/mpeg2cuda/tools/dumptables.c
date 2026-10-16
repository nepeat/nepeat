/* Print FFmpeg's MPEG-1/2 VLC tables as JSON for tools/m2vparse.py */
#include <stdio.h>
#include "libavcodec/mpeg12data.c"
#include "libavcodec/mpeg12vlc.h"

#define PAIRS(name, arr, n, codei, leni) do {                           \
        printf("\"%s\": [", name);                                      \
        for (int i = 0; i < (n); i++)                                   \
            printf("%s[%d,%d]", i ? "," : "", arr[i][codei], arr[i][leni]); \
        printf("],\n");                                                 \
    } while (0)

int main(void)
{
    printf("{\n");
    PAIRS("addr", ff_mpeg12_mbAddrIncrTable, 36, 0, 1);
    PAIRS("cbp", ff_mpeg12_mbPatTable, 64, 0, 1);
    PAIRS("mv", ff_mpeg12_mbMotionVectorTable, 17, 0, 1);
    PAIRS("b14", ff_mpeg1_vlc_table, MPEG12_RL_NB_ELEMS + 2, 0, 1);
    PAIRS("b15", ff_mpeg2_vlc_table, MPEG12_RL_NB_ELEMS + 2, 0, 1);
    printf("\"run\": [");
    for (int i = 0; i < MPEG12_RL_NB_ELEMS; i++) printf("%s%d", i ? "," : "", ff_mpeg12_run[i]);
    printf("],\n\"level\": [");
    for (int i = 0; i < MPEG12_RL_NB_ELEMS; i++) printf("%s%d", i ? "," : "", ff_mpeg12_level[i]);
    printf("],\n\"dcl\": [");
    for (int i = 0; i < 12; i++) printf("%s[%d,%d]", i ? "," : "", ff_mpeg12_vlc_dc_lum_code[i], ff_mpeg12_vlc_dc_lum_bits[i]);
    printf("],\n\"dcc\": [");
    for (int i = 0; i < 12; i++) printf("%s[%d,%d]", i ? "," : "", ff_mpeg12_vlc_dc_chroma_code[i], ff_mpeg12_vlc_dc_chroma_bits[i]);
    printf("]\n}\n");
    return 0;
}
