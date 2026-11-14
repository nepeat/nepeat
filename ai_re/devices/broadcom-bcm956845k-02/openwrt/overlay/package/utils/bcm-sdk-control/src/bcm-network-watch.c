/* SPDX-License-Identifier: GPL-2.0-only
 * Wake the native state reconciler on rtnetlink changes. Keep a periodic
 * resync for SDK learning, sysctls, UCI changes and lost notifications.
 */
#define _GNU_SOURCE
#include <sys/socket.h>
#include <sys/wait.h>
#include <linux/netlink.h>
#include <linux/rtnetlink.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <time.h>
static volatile sig_atomic_t stopping;
static unsigned long netlink_runs;
static void stop(int sig) { (void)sig; stopping=1; }
static void wake(int sig) { (void)sig; }
static int64_t now_ms(void) {
    struct timespec t;
    if(clock_gettime(CLOCK_MONOTONIC,&t)) {perror("clock_gettime");exit(1);}
    return (int64_t)t.tv_sec*1000+t.tv_nsec/1000000;
}
static void status(unsigned long events,unsigned long overflows,unsigned long runs,
                   const char *reason,int running) {
    const char *tmp="/tmp/bcm-sdk/watcher-status.json.new";
    FILE *f=fopen(tmp,"w");if(!f)return;
    fprintf(f,"{\"pid\":%ld,\"events\":%lu,\"overflows\":%lu,\"reconciles\":%lu,\"netlink_reconciles\":%lu,\"last_trigger\":\"%s\",\"running_apply\":%s}\n",
            (long)getpid(),events,overflows,runs,netlink_runs,reason,running?"true":"false");
    if(fclose(f)==0)rename(tmp,"/tmp/bcm-sdk/watcher-status.json");
}
int main(void) {
    const int groups[]={RTNLGRP_LINK,RTNLGRP_NEIGH,RTNLGRP_IPV4_IFADDR,
        RTNLGRP_IPV4_ROUTE,RTNLGRP_IPV4_RULE,RTNLGRP_IPV4_NETCONF,RTNLGRP_BRVLAN};
    int fd=socket(AF_NETLINK,SOCK_RAW|SOCK_NONBLOCK|SOCK_CLOEXEC,NETLINK_ROUTE);
    if(fd<0){perror("rtnetlink socket");return 1;}
    struct sockaddr_nl address={.nl_family=AF_NETLINK};
    if(bind(fd,(void *)&address,sizeof(address))){perror("rtnetlink bind");return 1;}
    int size=1024*1024;
    if(setsockopt(fd,SOL_SOCKET,SO_RCVBUF,&size,sizeof(size)))perror("SO_RCVBUF");
    for(unsigned i=0;i<sizeof(groups)/sizeof(groups[0]);i++)
        if(setsockopt(fd,SOL_NETLINK,NETLINK_ADD_MEMBERSHIP,&groups[i],sizeof(groups[i]))) {
            perror("rtnetlink subscribe");return 1;
        }
    struct sigaction sa={.sa_handler=stop};sigemptyset(&sa.sa_mask);
    sigaction(SIGTERM,&sa,NULL);sigaction(SIGINT,&sa,NULL);
    sa.sa_handler=wake;sigaction(SIGCHLD,&sa,NULL);
    unsigned long events=0,overflows=0,runs=0;
    int dirty=1;int64_t first=now_ms(),last=first,periodic=first;
    pid_t child=0;const char *reason="startup";
    fprintf(stderr,"bcm-network: watching link, VLAN, neighbor, address, route, rule and forwarding events\n");
    while(!stopping) {
        int child_status;
        if(child && waitpid(child,&child_status,WNOHANG)==child) {
            child=0;periodic=now_ms()+2000;
            status(events,overflows,runs,reason,0);
        }
        int64_t now=now_ms();
        int64_t due=dirty?(last+100<first+500?last+100:first+500):periodic;
        if(!child && now>=due) {
            reason=dirty?(runs?"netlink":"startup"):"periodic";
            child=fork();
            if(child<0){perror("fork reconcile");return 1;}
            if(!child) {
                signal(SIGTERM,SIG_DFL);signal(SIGINT,SIG_DFL);
                int null=open("/dev/null",O_RDWR);
                if(null>=0){dup2(null,STDIN_FILENO);dup2(null,STDOUT_FILENO);if(null>2)close(null);}
                execl("/usr/bin/ucode","ucode","/usr/libexec/bcm-native.uc","apply",(char *)NULL);
                perror("exec native reconcile");_exit(127);
            }
            if(dirty&&runs)netlink_runs++;
            runs++;dirty=0;status(events,overflows,runs,reason,1);
        }
        now=now_ms();
        int timeout=child?100:(int)(due>now?due-now:0);
        struct pollfd pollfd={.fd=fd,.events=POLLIN};
        int ready=poll(&pollfd,1,timeout);
        if(ready<0){if(errno==EINTR)continue;perror("poll rtnetlink");break;}
        if(pollfd.revents&(POLLERR|POLLHUP|POLLNVAL)) {
            /* POLLERR can mean an overrun; recvmsg consumes ENOBUFS. */
            if(pollfd.revents&(POLLHUP|POLLNVAL)){fprintf(stderr,"rtnetlink closed\n");break;}
        }
        if(ready>0)for(int batch=0;batch<256;batch++) {
            char buffer[65536];struct sockaddr_nl from;
            struct iovec iov={buffer,sizeof(buffer)};
            struct msghdr msg={.msg_name=&from,.msg_namelen=sizeof(from),.msg_iov=&iov,.msg_iovlen=1};
            ssize_t n=recvmsg(fd,&msg,MSG_DONTWAIT);
            if(n<0) {
                if(errno==EAGAIN||errno==EWOULDBLOCK)break;
                if(errno==EINTR)continue;
                if(errno!=ENOBUFS){perror("recv rtnetlink");stopping=1;break;}
                overflows++;
            } else {
                if(n==0){stopping=1;break;}
                if(from.nl_pid!=0)continue; /* Only kernel notifications. */
                events++;
                if(msg.msg_flags&MSG_TRUNC)overflows++;
            }
            if(!dirty)first=now_ms();
            last=now_ms();dirty=1;
        }
    }
    close(fd);
    if(child) {
        kill(child,SIGTERM);
        while(waitpid(child,NULL,0)<0&&errno==EINTR){}
    }
    return stopping?0:1;
}
