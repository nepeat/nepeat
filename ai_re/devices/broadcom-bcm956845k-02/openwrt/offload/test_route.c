/* SPDX-License-Identifier: GPL-2.0-only
 * Isolated hardware route proof: swp1/xe0 MAC loopback -> L3 -> xe15/swp16
 * MAC loopback -> CPU capture. Linux forwarding must be disabled separately.
 */
#include <sys/socket.h>
#include <sys/time.h>
#include <net/if.h>
#include <netpacket/packet.h>
#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
static unsigned checksum(const unsigned char *p, unsigned n) {
    unsigned sum=0;while(n>1){sum+=(p[0]<<8)|p[1];p+=2;n-=2;}
    if(n)sum+=p[0]<<8;
    while(sum>>16)sum=(sum&65535)+(sum>>16);
    return (~sum)&65535;
}
static int packet_socket(const char *name) {
    int s=socket(AF_PACKET,SOCK_RAW,htons(3));if(s<0)return -1;
    struct sockaddr_ll a={.sll_family=AF_PACKET,.sll_protocol=htons(3),.sll_ifindex=if_nametoindex(name)};
    if(!a.sll_ifindex||bind(s,(void *)&a,sizeof(a))){close(s);return -1;}
    struct timeval t={2,0};setsockopt(s,SOL_SOCKET,SO_RCVTIMEO,&t,sizeof(t));return s;
}
int main(int argc,char **argv) {
    int absent=argc==2&&!strcmp(argv[1],"--withdrawn");
    if(argc>2||(argc==2&&!absent))return 2;
    int tx=packet_socket("swp1"),rx=packet_socket("swp16");
    if(tx<0||rx<0){perror("packet sockets");return 1;}
    unsigned char f[74]={0},expected[74],r[2048];
    unsigned char inmac[6]={2,0x10,0x18,0xaa,1,1},src[6]={2,0x10,0x18,0xcc,1,2};
    unsigned char outmac[6]={2,0x10,0x18,0xaa,2,1},peer[6]={2,0x10,0x18,0xbb,2,2};
    memcpy(f,inmac,6);memcpy(f+6,src,6);f[12]=8;
    f[14]=0x45;f[17]=60;f[18]=0x12;f[19]=0x34;f[22]=64;f[23]=17;
    f[26]=198;f[27]=19;f[28]=1;f[29]=2;
    f[30]=198;f[31]=19;f[32]=2;f[33]=2;
    f[34]=0x9c;f[35]=0x40;f[36]=0x9c;f[37]=0x41;f[39]=40;
    memcpy(f+42,"emma-hardware-route-proof",25);
    unsigned c=checksum(f+14,20);f[24]=c>>8;f[25]=c;
    memcpy(expected,f,sizeof(f));memcpy(expected,peer,6);memcpy(expected+6,outmac,6);
    expected[22]=63;expected[24]=expected[25]=0;c=checksum(expected+14,20);expected[24]=c>>8;expected[25]=c;
    int passed=0,count=absent?1:20;
    for(int n=0;n<count;n++) {
        f[73]=expected[73]=n;
        if(send(tx,f,sizeof(f),0)!=(ssize_t)sizeof(f)){perror("send");return 1;}
        for(;;) {
            struct sockaddr_ll from;socklen_t alen=sizeof(from);
            int len=recvfrom(rx,r,sizeof(r),0,(void *)&from,&alen);
            if(len<0) {
                if(absent&&(errno==EAGAIN||errno==EWOULDBLOCK)) {
                    puts("PASS route withdrawal: no routed packet reached swp16");return 0;
                }
                perror("receive routed frame");goto done;
            }
            if(from.sll_pkttype==PACKET_OUTGOING||len<74||memcmp(r+42,f+42,25)||r[73]!=n)continue;
            if(absent){fputs("FAIL routed packet arrived after route withdrawal\n",stderr);return 1;}
            if(len==(int)sizeof(expected)&&!memcmp(r,expected,sizeof(expected))&&!checksum(r+14,20)) {passed++;break;}
            fprintf(stderr,"FAIL routed frame: length=%d TTL=%u checksum=%04x\n",len,r[22],checksum(r+14,20));
            for(int i=0;i<len&&i<100;i++) { fprintf(stderr,"%02x%s",r[i],i%16==15?"\n":" "); }
            goto done;
        }
    }
done:
    close(tx);close(rx);
    printf("%d/20 exact ASIC-routed IPv4 frames: next-hop MAC rewrite, egress RIF source MAC, TTL 64->63, valid IPv4 checksum\n",passed);
    return passed==20?0:1;
}
