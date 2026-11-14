/* SPDX-License-Identifier: GPL-2.0-only
 * Native bridge proof: the ASIC forwards a broadcast once. Linux must not
 * reflood its CPU copy to other ports in the same hardware forwarding domain.
 * Requires isolated VLAN 100 on swp1/swp16 and egress MAC-loopback capture.
 */
#include <sys/socket.h>
#include <sys/time.h>
#include <net/if.h>
#include <linux/if_packet.h>
#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
static int sock(const char *name) {
    int fd=socket(AF_PACKET,SOCK_RAW,htons(3));if(fd<0)return -1;
    struct sockaddr_ll a={.sll_family=AF_PACKET,.sll_protocol=htons(3),.sll_ifindex=if_nametoindex(name)};
    if(!a.sll_ifindex||bind(fd,(void *)&a,sizeof(a)))return -1;
    struct timeval t={0,400000};setsockopt(fd,SOL_SOCKET,SO_RCVTIMEO,&t,sizeof(t));
    int one=1;setsockopt(fd,SOL_PACKET,PACKET_AUXDATA,&one,sizeof(one));return fd;
}
int main(int argc,char **argv) {
    int tagged=0,multicast=0;
    for(int i=1;i<argc;i++) {
        if(!strcmp(argv[i],"--tagged"))tagged=1;
        else if(!strcmp(argv[i],"--multicast"))multicast=1;
        else return 2;
    }
    int tx=sock("swp1"),rx=sock("swp16");
    if(tx<0||rx<0){perror("socket");return 1;}
    unsigned char frame[96]={0},data[2048];
    union { struct cmsghdr align; unsigned char bytes[256]; } control;
    memset(frame,255,6);unsigned char mac[6]={2,0x10,0x18,0xdd,1,2};memcpy(frame+6,mac,6);
    if(multicast) {unsigned char group[6]={1,0,0x5e,1,2,3};memcpy(frame,group,6);}
    frame[12]=0x88;frame[13]=0xb5;memcpy(frame+14,"emma-native-bridge-proof",24);
    int got=0,duplicate=0,outgoing=0;
    for(int seq=0;seq<20;seq++) {
        frame[95]=seq;
        if(send(tx,frame,sizeof(frame),0)!=(ssize_t)sizeof(frame)){perror("send");return 1;}
        int seen=0;
        struct timeval timeout={2,0};setsockopt(rx,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout));
        for(;;) {
            struct sockaddr_ll from;struct iovec iov={data,sizeof(data)};
            struct msghdr msg={.msg_name=&from,.msg_namelen=sizeof(from),.msg_iov=&iov,.msg_iovlen=1,.msg_control=control.bytes,.msg_controllen=sizeof(control.bytes)};
            int len=recvmsg(rx,&msg,0),vid=-1;
            if(len<0) {if(errno==EAGAIN||errno==EWOULDBLOCK)break;perror("receive");return 1;}
            for(struct cmsghdr *c=CMSG_FIRSTHDR(&msg);c;c=CMSG_NXTHDR(&msg,c)) if(c->cmsg_level==SOL_PACKET&&c->cmsg_type==PACKET_AUXDATA) {
                struct tpacket_auxdata *a=(void *)CMSG_DATA(c);if(a->tp_status&TP_STATUS_VLAN_VALID)vid=a->tp_vlan_tci&4095;
            }
            if(len>=18&&data[12]==0x81&&data[13]==0) {vid=((data[14]<<8)|data[15])&4095;memmove(data+12,data+16,len-16);len-=4;}
            if(len!=(int)sizeof(frame)||memcmp(data+14,frame+14,24)||data[95]!=seq)continue;
            if(memcmp(data,frame,sizeof(frame))||(tagged?vid!=100:vid!=-1)) {
                fprintf(stderr,"FAIL frame/tag mismatch: VID %d expected %s\n",vid,tagged?"100":"untagged");return 1;
            }
            if(from.sll_pkttype==PACKET_OUTGOING)outgoing++;
            else if(seen++)duplicate++;
            else {got++;timeout.tv_sec=0;timeout.tv_usec=400000;setsockopt(rx,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout));}
        }
        if(!seen){fprintf(stderr,"missing sequence %d\n",seq);break;}
    }
    printf("%d/20 native ASIC bridge frames, %s, %s; duplicate ingress=%d software re-forward=%d\n",got,tagged?"802.1Q VID 100":"untagged",multicast?"multicast":"broadcast",duplicate,outgoing);
    close(tx);close(rx);return got==20&&!duplicate&&!outgoing?0:1;
}
