/* SPDX-License-Identifier: GPL-2.0-only
 * Verify a kernel-generated ARP reply after a request traverses ASIC loopback.
 * Requires 198.18.0.1/30 on swpN and MAC loopback on its ASIC port.
 */
#include <sys/socket.h>
#include <sys/time.h>
#include <net/if.h>
#include <netpacket/packet.h>
#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
int main(int argc,char **argv) {
    if(argc!=2)return 2;
    int s=socket(AF_PACKET,SOCK_RAW,htons(0x0003));if(s<0){perror("socket");return 1;}
    struct sockaddr_ll a={.sll_family=AF_PACKET,.sll_protocol=htons(0x0003),.sll_ifindex=if_nametoindex(argv[1])};
    if(bind(s,(void *)&a,sizeof(a))){perror("bind");return 1;}
    struct timeval timeout={5,0};setsockopt(s,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout));
    unsigned char f[60]={0},rx[2048],peer[6]={2,0x10,0x18,0x56,0xfe,2};
    memset(f,255,6);memcpy(f+6,peer,6);f[12]=8;f[13]=6;
    f[15]=1;f[16]=8;f[18]=6;f[19]=4;f[21]=1;memcpy(f+22,peer,6);
    unsigned char sip[4]={198,18,0,2},tip[4]={198,18,0,1};memcpy(f+28,sip,4);memcpy(f+38,tip,4);
    if(send(s,f,sizeof(f),0)!=sizeof(f)){perror("send");return 1;}
    for(int i=0;i<20;i++) {
        struct sockaddr_ll from;socklen_t len=sizeof(from);
        int n=recvfrom(s,rx,sizeof(rx),0,(void *)&from,&len);
        if(n<0){perror("ARP receive");return 1;}
        if(n>=42&&rx[12]==8&&rx[13]==6&&rx[20]==0&&rx[21]==2&&!memcmp(rx+28,tip,4)&&!memcmp(rx+38,sip,4)&&!memcmp(rx,peer,6)) {
            printf("%s: PASS Linux IPv4 stack generated ARP reply for 198.18.0.1 after request traversed SDK/ASIC MAC loopback (packet type %d)\n",argv[1],from.sll_pkttype);
            close(s);return 0;
        }
    }
    fprintf(stderr,"no matching ARP reply\n");return 1;
}
