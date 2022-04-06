# nepeat (erin) [she/her/they/them]

i curse this site with my bad code and disaster of an infrastructure.

## cool messes i've done
* archive team infra (2019 - present)
    * split across many repos!
    * many [terraform + packer](https://github.com/general-programming/megarepo/tree/mainline/automation/deploy_server) playbooks to bulk provision servers automagically instead of doing them all by hand! 1000+ servers have been made and axed off from this as a starting point...
    * [pyinfra](https://github.com/general-programming/megarepo/tree/mainline/infrastructure/pyinfra) playbooks that were used to onboard the core nomad servers and kickstart worker nodes with consul/vault
    * [nomad](https://github.com/general-programming/megarepo/tree/mainline/infrastructure/nomad) jobs to run log egress + updates + warrior tasks on all the nodes
    * [beatbeatlarge](https://github.com/nepeat/beatbeatlarge), a set of scripts that ingests logs from a stream and outposts compressed log files. includes a rust prototype to process compressed log files in bulk!
* [barf](https://github.com/general-programming/megarepo/tree/mainline/projects/barf) (2022) - config generation and basic management prototype for my vpn backbone and soon the rest of my lab networks
* [ansible playbooks] (2018 - present)(https://github.com/general-programming/megarepo/tree/mainline/infrastructure/ansible)
    * evolving mess that manages my current colo infra and cloud infra
* [r/place (2022) archiver](https://github.com/Snakeroom/bad-nep-archive-code) (2022)
    * mass downloading of every single websocket message and frame reddit sends? fun stuff! 
* [twitch drives](https://github.com/general-programming/twitch-drives) (2021)
    * i let a discord channel and twitch drive my car.
    * results are very silly during peak testing https://www.youtube.com/watch?v=x4c6seYUhLs
* [MSPARP (old)](https://github.com/MSPARP/MSPARP) (2014 - 2016) + MSPARP (new) (2014 - 2019)
    * that one dreaded homestuck roleplay website as described by others
    * i somehow contributed a decent amount of features and is the reason I Know Python And Linux Enough To Be Dangerous

## minor but still cool
* [cfworker-dhcp-discord](https://github.com/general-programming/megarepo/tree/mainline/serverless/cfworker-dhcp-discord) - strapping a discord webhook to a dhcp server is unwise, yet i do it either way
* [cfworker-netbox-discord](https://github.com/general-programming/megarepo/tree/mainline/serverless/cfworker-netbox-discord) - however, having a discord webhook for netbox ipam update notifications is more useful than doing it for dhcp.
* [twilio-discordhook](https://github.com/general-programming/megarepo/tree/mainline/serverless/twilio-discordhook) - call notifications and SMS messages with MMS in discord is cool

## bad prototypes that sorta work
* https://github.com/general-programming/torspider (2018)
    * AGPL warning
    * ancient code i do not touch but it was a functional test project that scraped a decent part of the tor network
* https://github.com/nepeat/dischain (2018)
    * dare to store data in blockchains gone wrong
    * managed to store a few images and i recall video files in a niche coin
* https://github.com/nepeat/chatforesnics (2018)
    * attempt to reparse imessage databases and other chat databases into a single database
    * ended up getting bored after a month, might revisit this from scratch eventually
