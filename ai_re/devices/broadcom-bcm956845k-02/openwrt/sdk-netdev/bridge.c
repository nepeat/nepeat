/* SPDX-License-Identifier: GPL-2.0-only
 * Experimental packet-API/TAP adapter for the exact recovered SDK 5.9.1.
 * This is NOT Broadcom KNET. See patch_sdk.py for the explicit bootstrap hook.
 * All SDK addresses/packet offsets are tied to a checked original image hash.
 */
#include <stdint.h>
#include <stddef.h>
#include <fcntl.h>
#include <sys/types.h>
#include <sys/ioctl.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <net/if.h>
#include <linux/if_tun.h>
#include <linux/sockios.h>
#include <linux/ethtool.h>

extern int sdk_open(const char *, int, ...);
extern int sdk_close(int);
extern ssize_t sdk_read(int, void *, size_t);
extern ssize_t sdk_write(int, const void *, size_t);
extern int sdk_ioctl(int, unsigned long, ...);
struct sdk_timeval { int32_t seconds, microseconds; }; /* factory glibc time32 */
extern int sdk_select(int, fd_set *, fd_set *, fd_set *, struct sdk_timeval *);
extern int sdk_socket(int, int, int);
extern int sdk_printf(const char *, ...);
extern int sdk_snprintf(char *, size_t, const char *, ...);
extern uint32_t sdk_time_usecs(void);
extern void sdk_usleep(unsigned int);
extern void *sdk_thread_create(char *, int, int, void (*)(void *), void *);
extern int sdk_pkt_alloc(int, int, uint32_t, void **);
extern int sdk_pkt_free(int, void *);
extern int sdk_tx(int, void *, void *);
extern int sdk_rx_active(int);
extern int sdk_rx_start(int, void *);
extern int sdk_rx_register(int, const char *, int (*)(int, void *, void *), uint8_t, void *, uint32_t);
extern int sdk_port_link(int, int, int *);
extern int sdk_port_speed(int, int, int *);
extern int sdk_port_enabled(int, int, int *);
extern int sdk_port_enable(int, int, int);
extern int sdk_port_pvid(int, int, uint16_t *);
extern void sdk_l2_init(void *, const uint8_t *, uint16_t);
extern int sdk_l2_add(int, void *);
extern int sdk_l2_delete(int, const uint8_t *, uint16_t);

#define MAX_PORTS 64
#define MAX_FRAME 2048
/* Offsets measured through CINT against this exact binary. */
#define U8(p,o) (*(uint8_t *)((uint8_t *)(p)+(o)))
#define U16(p,o) (*(uint16_t *)((uint8_t *)(p)+(o)))
#define U32(p,o) (*(uint32_t *)((uint8_t *)(p)+(o)))
#define PTR(p,o) (*(void **)((uint8_t *)(p)+(o)))
struct port {
    int fd, admin, sdk_admin, speed;
    uint16_t vid;
    uint8_t mac[6];
    char name[IFNAMSIZ];
    unsigned long tx, rx, errors;
};
static struct port ports[MAX_PORTS];
static int count, started, socket_fd, linux_managed;
static int exists(const char *path) { int f=sdk_open(path,O_RDONLY); if(f<0)return 0; sdk_close(f);return 1; }
static void zero(void *v, unsigned n) { uint8_t *p=v; while(n--) *p++=0; }
static void copy(void *v, const void *s, unsigned n) { uint8_t *p=v;const uint8_t *q=s;while(n--) *p++=*q++; }
static int equal(const void *a, const void *b, unsigned n) { const uint8_t *p=a,*q=b;while(n--) if(*p++!=*q++) return 0;return 1; }
static void name(char *s, int i) {
    s[0]='s';s[1]='w';s[2]='p';
    if(i>=10) { s[3]='0'+i/10;s[4]='0'+i%10;s[5]=0; }
    else { s[3]='0'+i;s[4]=0; }
}
static void ifreq_init(struct ifreq *r, const struct port *p) { zero(r,sizeof(*r));copy(r->ifr_name,p->name,IFNAMSIZ); }
/* snprintf returns the would-be length; keep the cursor inside the buffer so
 * the remaining-size argument can never wrap. A truncated report is dropped. */
static int bounded(int used, unsigned cap) { return used>(int)cap-1?(int)cap-1:used; }
static void cpu_mac(struct port *p, const uint8_t *mac, uint16_t vid) {
    uint32_t l2[16];
    if(p->vid) sdk_l2_delete(0,p->mac,p->vid);
    sdk_l2_init(l2,mac,vid);
    l2[0]=0x20; /* BCM_L2_STATIC; destination CPU port zero. */
    l2[3]=0;l2[4]=0;
    int rc=sdk_l2_add(0,l2);
    if(rc) { p->errors++;sdk_printf("SDK TAP: L2 add %s VLAN %u failed %d\n",p->name,vid,rc); }
    else { copy(p->mac,mac,6);p->vid=vid; }
}
static void update_ports(void) {
    for(int i=0;i<count;i++) {
        struct port *p=&ports[i];struct ifreq r;int enabled=0,link=0;
        if(p->fd<0) continue;
        ifreq_init(&r,p);
        if(sdk_ioctl(socket_fd,SIOCGIFFLAGS,&r)<0) continue;
        int admin=!!(r.ifr_flags&IFF_UP);
        if(sdk_port_enabled(0,i+1,&enabled)) continue;
        if(linux_managed) {
            if(admin!=enabled && !exists("/var/lock/bcm-switch-apply") && !exists("/tmp/bcm-sdk/native-failed")) {
                if(sdk_port_enable(0,i+1,admin)) p->errors++;
                else enabled=admin;
            }
        } else if(p->admin>=0 && admin!=p->admin) {
            if(sdk_port_enable(0,i+1,admin)) p->errors++;
            else enabled=admin;
        } else if(enabled!=admin) {
            if(enabled) r.ifr_flags|=IFF_UP;else r.ifr_flags&=~IFF_UP;
            if(sdk_ioctl(socket_fd,SIOCSIFFLAGS,&r)<0) p->errors++;
            else admin=enabled;
        }
        p->admin=admin;p->sdk_admin=enabled;
        if(!sdk_port_link(0,i+1,&link)) { int carrier=link&&enabled; sdk_ioctl(p->fd,TUNSETCARRIER,&carrier); }
        int speed=0;
        if(!sdk_port_speed(0,i+1,&speed) && speed!=p->speed) {
            struct ethtool_cmd settings;zero(&settings,sizeof(settings));settings.cmd=ETHTOOL_GSET;
            ifreq_init(&r,p);r.ifr_data=(void *)&settings;
            if(!sdk_ioctl(socket_fd,SIOCETHTOOL,&r)) {
                settings.cmd=ETHTOOL_SSET;ethtool_cmd_speed_set(&settings,speed);
                settings.duplex=DUPLEX_FULL;settings.autoneg=AUTONEG_DISABLE;
                if(!sdk_ioctl(socket_fd,SIOCETHTOOL,&r)) p->speed=speed;
            }
        }
        uint16_t vid=0;
        if(!linux_managed && !sdk_port_pvid(0,i+1,&vid)) {
            ifreq_init(&r,p);
            if(!sdk_ioctl(socket_fd,SIOCGIFHWADDR,&r) && (vid!=p->vid || !equal(p->mac,r.ifr_hwaddr.sa_data,6)))
                cpu_mac(p,(uint8_t *)r.ifr_hwaddr.sa_data,vid);
        }
    }
}
static int receive(int unit, void *pkt, void *cookie) {
    (void)unit;(void)cookie;
    int src=U8(pkt,0x85);
    if(src<1||src>count||ports[src-1].fd<0) return 1; /* BCM_RX_NOT_HANDLED */
    void *block=PTR(pkt,0);uint8_t *data=PTR(block,0);
    unsigned len=U16(pkt,0x44);uint32_t flags=U32(pkt,0xa4);
    if(len<14||len>MAX_FRAME||len>U32(block,4)) {ports[src-1].errors++;return 2;}
    if(!(flags&0x1000000) && len>=18) len-=4; /* CRC not already stripped */
    uint8_t frame[MAX_FRAME];
    if((U8(pkt,0x87)&1) && len>=18 && data[12]==0x81 && data[13]==0) {
        copy(frame,data,12);copy(frame+12,data+16,len-16);len-=4;data=frame;
    }
    if(sdk_write(ports[src-1].fd,data,len)==(ssize_t)len) ports[src-1].rx++;
    else ports[src-1].errors++;
    return 2; /* BCM_RX_HANDLED; SDK retains ownership */
}
static void transmit(struct port *p, int number, uint8_t *data, unsigned len) {
    if(len<14||len>MAX_FRAME-4) {p->errors++;return;}
    int tagged=data[12]==0x81 && data[13]==0;
    unsigned size=(len<60?60:len); /* SDK allocates CRC space itself. */
    void *pkt=0;
    if(sdk_pkt_alloc(0,size,0x830|(tagged?0:4),&pkt)) {p->errors++;return;}
    void *block=PTR(pkt,0);uint8_t *out=PTR(block,0);
    zero(out,size);copy(out,data,len);
    U32(pkt,0x48+(number/32)*4)=1u<<(number%32);
    if(!tagged) U32(pkt,0x54+(number/32)*4)=1u<<(number%32);
    int rc=sdk_tx(0,pkt,0);
    if(rc) {p->errors++; if(p->errors<6) sdk_printf("SDK TAP: TX %s error %d\n",p->name,rc);}
    else p->tx++;
    sdk_pkt_free(0,pkt);
}
static void worker(void *arg) {
    (void)arg;
    uint8_t frame[MAX_FRAME];uint32_t last_update=0;
    /* Main SDK startup and the first UCI apply must finish before RX begins. */
    for(int n=0;n<180;n++) {
        int f=sdk_open("/tmp/bcm-sdk/last-apply.json",O_RDONLY);
        if(f>=0) {sdk_close(f);break;}
        sdk_usleep(1000000);
    }
    update_ports();
    int rc=sdk_rx_register(0,"openwrt-tap",receive,200,0,0x40000000);
    if(!rc && !sdk_rx_active(0)) rc=sdk_rx_start(0,0);
    sdk_printf("SDK TAP: RX registration/start %d; %d Linux interfaces\n",rc,count);
    for(;;) {
        fd_set readfds;FD_ZERO(&readfds);int max=-1;
        for(int i=0;i<count;i++) if(ports[i].fd>=0) {FD_SET(ports[i].fd,&readfds);if(ports[i].fd>max)max=ports[i].fd;}
        struct sdk_timeval timeout={0,100000};
        int ready=sdk_select(max+1,&readfds,0,0,&timeout);
        if(ready>0) for(int i=0;i<count;i++) if(ports[i].fd>=0&&FD_ISSET(ports[i].fd,&readfds)) {
            ssize_t len=sdk_read(ports[i].fd,frame,sizeof(frame));
            if(len>0) transmit(&ports[i],i+1,frame,(unsigned)len);
        }
        if((uint32_t)(sdk_time_usecs()-last_update)>=1000000) {
            update_ports();last_update=sdk_time_usecs();
            char report[16384];int used=bounded(sdk_snprintf(report,sizeof(report),"{\"backend\":\"sdk-tap\",\"knet\":false,\"ports\":["),sizeof(report));
            for(int i=0;i<count;i++) used=bounded(used+sdk_snprintf(report+used,sizeof(report)-used,"%s{\"device\":\"%s\",\"sdk_port\":%d,\"tx\":%lu,\"rx\":%lu,\"errors\":%lu}",i?",":"",ports[i].name,i+1,ports[i].tx,ports[i].rx,ports[i].errors),sizeof(report));
            used=bounded(used+sdk_snprintf(report+used,sizeof(report)-used,"]}\n"),sizeof(report));
            if(used>=(int)sizeof(report)-1) used=0; /* incomplete JSON: write nothing */
            int log=sdk_open("/tmp/bcm-sdk/netdev-status.json",O_WRONLY|O_CREAT|O_TRUNC,0600);
            if(log>=0) {sdk_write(log,report,used);sdk_close(log);}
        }
    }
}
/* Invoked explicitly through the otherwise-disabled KNET init entrypoint.
 * No KNET ioctl/filter ABI is claimed or implemented by this adapter.
 */
int bridge_start(int unit) {
    if(unit!=0) return -3;
    if(started) return 0;
    linux_managed=exists("/tmp/bcm-sdk/linux-managed");
    char profile[4]={0};int f=sdk_open("/tmp/bcm-sdk/active-profile",O_RDONLY);
    if(f<0) return -1;
    sdk_read(f,profile,3);sdk_close(f);count=profile[0]=='4'?16:64;
    socket_fd=sdk_socket(AF_INET,SOCK_DGRAM,0);
    if(socket_fd<0) return -1;
    for(int i=0;i<count;i++) ports[i].fd=-1;
    for(int i=0;i<count;i++) {
        struct port *p=&ports[i];name(p->name,i+1);p->admin=-1;
        int fd=sdk_open("/dev/net/tun",O_RDWR|O_NONBLOCK);
        struct ifreq r;ifreq_init(&r,p);r.ifr_flags=IFF_TAP|IFF_NO_PI;
        if(fd<0||sdk_ioctl(fd,TUNSETIFF,&r)<0) {
            if(fd>=0)sdk_close(fd);
            for(int j=0;j<i;j++) sdk_close(ports[j].fd);
            sdk_close(socket_fd);sdk_printf("SDK TAP: interface creation failed at %s\n",p->name);return -1;
        }
        p->fd=fd;
        ifreq_init(&r,p);r.ifr_hwaddr.sa_family=1;
        uint8_t mac[6]={2,0x10,0x18,0x56,0,i+1};copy(r.ifr_hwaddr.sa_data,mac,6);
        sdk_ioctl(socket_fd,SIOCSIFHWADDR,&r);
        int carrier=0;sdk_ioctl(fd,TUNSETCARRIER,&carrier);
    }
    if(sdk_thread_create("sdk-tap",65536,100,worker,0)==(void *)-1) return -1;
    started=1;sdk_printf("SDK TAP: started %d swp interfaces (not KNET)\n",count);
    return 0;
}
