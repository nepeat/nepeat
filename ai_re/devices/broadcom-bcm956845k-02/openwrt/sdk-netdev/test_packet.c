/* SPDX-License-Identifier: GPL-2.0-only
 * Explicit hardware MAC-loopback test; requires loopback on the selected port.
 */
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/ioctl.h>
#include <net/if.h>
#include <netpacket/packet.h>
#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
int main(int argc,char **argv) {
    if(argc!=2) {fprintf(stderr,"usage: %s swpN (ASIC MAC loopback required)\n",argv[0]);return 2;}
    int s=socket(AF_PACKET,SOCK_RAW,htons(0x88b5));
    if(s<0) {perror("socket");return 1;}
    struct ifreq r={0};snprintf(r.ifr_name,IFNAMSIZ,"%s",argv[1]);
    if(ioctl(s,SIOCGIFHWADDR,&r)) {perror("get MAC");return 1;}
    unsigned char frame[128]={0},reply[2048];memcpy(frame,r.ifr_hwaddr.sa_data,6);
    unsigned char source[6]={2,0x10,0x18,0x56,0xfe,1};memcpy(frame+6,source,6);frame[12]=0x88;frame[13]=0xb5;
    memcpy(frame+14,"emma-sdk-netdev-loopback",23);
    struct sockaddr_ll address={.sll_family=AF_PACKET,.sll_protocol=htons(0x88b5),.sll_ifindex=if_nametoindex(argv[1])};
    if(bind(s,(void *)&address,sizeof(address))) {perror("bind");return 1;}
    struct timeval timeout={3,0};setsockopt(s,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout));
    int good=0;
    for(int n=0;n<20;n++) {
        frame[40]=n;
        if(send(s,frame,sizeof(frame),0)!=sizeof(frame)) {perror("send");break;}
        for(;;) {
            struct sockaddr_ll from;socklen_t len=sizeof(from);
            int bytes=recvfrom(s,reply,sizeof(reply),0,(void *)&from,&len);
            if(bytes<0) {perror("receive");goto done;}
            if(from.sll_pkttype==PACKET_OUTGOING) continue;
            if(bytes==(int)sizeof(frame)&&!memcmp(frame,reply,sizeof(frame))) {good++;break;}
            fprintf(stderr,"unexpected ingress: length %d, expected %zu, type %d\n",bytes,sizeof(frame),from.sll_pkttype);
            for(int i=0;i<bytes&&i<160;i++) fprintf(stderr,"%02x%s",reply[i],i%16==15?"\n":" ");
        }
    }
done:
    close(s);printf("%s: %d/20 exact Ethernet frames passed Linux TAP -> SDK DMA -> ASIC MAC loopback -> SDK RX -> Linux TAP\n",argv[1],good);return good==20?0:1;
}
