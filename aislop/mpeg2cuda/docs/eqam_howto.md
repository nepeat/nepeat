# EQAM-420B: add a channel in slot 2 (SPTS mode, fw 1.1.13)

Sources (D = documented, I = inference):
- [420A] EQAM-420A manual PDF, https://files.avprosupply.com/files/attachments/2664/blonder-tongue-eqam-420a-1-16p-manual.pdf (sections cited by number)
- [420B] manualslib EQAM-420B manual, https://www.manualslib.com/manual/2034059/Blonder-Tongue-Eqam-420b.html?page=19 (also page=23 Stream Config, page=22)
  The 420B pages return only partial text (screenshots/callouts missing), so 420A is the fuller source.

## What the docs say
- Entry format (D, 420A 6.2.1 p17; 420B p19): `xxx://yyy.yyy.yyy.yyy:zzzzz`, xxx = UDP or RTP only. "The address entered here MUST be identical to the address of the source stream, or else the unit cannot recognize the source." No duplicate entries allowed (highlighted yellow).
- Multicast = 224.0.0.0-239.255.255.255; anything else is treated as unicast (D, same). The unicast example is "provided from IP address 172.160.80.70, port 50000" i.e. the manual's unicast example uses the SENDER's address.
- 420B p19: "IGMP Version 3 adds support for source filtering ... Source-Specific Multicast [SSM]" appears under the Source IP field. The 420A manual has no IGMP or source-IP fields at all. Nothing found says what the destination IP of a unicast sender must be.
- Ping button (Diagnostics) works for unicast only (420A 6.5). Data port (1 GigE) and 10/100 control port must be on different subnets (420A s4, 6.7.1).
- Bitrate (D, 420A 6.2): sum of the 4 streams on one QAM RF channel must not exceed 38.8 Mbps or you get pixelation. 256QAM-B holds ~38.8 Mbps. Accepts CBR and VBR; does null-packet deletion/addition; MPEG-2 or H.264 (420A 2.2). AC-3 audio is what Stream Info examples and Pro:Idiom specs show. No PCR/PID rules documented for clear output. IP indicator: green = streaming, red = not.
- SPTS = one program per input stream (D). Stream Config (420B p23, SPTS only): Program Number "must enter a unique output program number for each program"; Short Name up to 7 chars; Major/Minor 1-999; VCT Type TVCT or CVCT; Out Of Band Enable/Disable just flags TS packets and "would not affect the input or output configuration".
- RF channel (D, 420A 6.4): shown on the TV "only if the source stream does not carry any virtual channel number". QAM Config: Output QAM Mode 256B recommended for USA, Map STD/IRC/HRC, module may take seconds to lock. Pick the map to match your TV/cable setting.
- Always press SAVE after edits (D, repeated on every screen). Admin.html has System Reboot, Config File/Automatic mode: in Automatic mode the unit rewrites its config from the Config file every 30 min, which can revert manual edits and zero minor channels (420A 6.7.1, 6.11).

## Steps (slot 2)
1. Check Admin.html Configuration Mode is Manual (or edit the Config file) so edits are not reverted.
2. Main > Status > Edit: slot 2 Stream = `UDP://<value>:<port>` (see inference below, use a port different from slot 1's 5000, e.g. 5001), Name <=10 chars, SAVE.
3. Start the sender; slot 2 IP indicator should turn green and Bitrate non-zero. (Diagnostics: TS Sync Error counter, Ping.)
4. Main > Status > Stream Config: set Program Number (unique, must match nothing else; ideally the PMT program in your stream or a unique number), Short Name, Major (e.g. 47) / Minor (e.g. 2), VCT type (CVCT for cable), SAVE.
5. QAM Config: confirm module/channel 47 On, 256B, matching map (STD/HRC/IRC), lock status Locked, SAVE.
6. On TV, rescan; the channel appears as Major.Minor (virtual number). Without a virtual number it shows the RF channel-slot.

## ffmpeg sender (inference)
```
ffmpeg -re -i in.mkv -c:v libx264 -b:v 6M -maxrate 6M -bufsize 6M -g 60 \
 -c:a ac3 -b:a 192k -f mpegts -muxrate 7M -pcr_period 20 \
 -mpegts_service_id 1 -mpegts_pmt_start_pid 0x100 -mpegts_start_pid 0x101 \
 "udp://<dest>:5001?pkt_size=1316"
```
Single program, PCR present, rate capped so four slots total < 38.8 Mbps (about 9 Mbps each). Constant rate preferred.

## Group address field (inference)
The docs only define it as the "address of the source stream". For multicast, group = the multicast group the EQAM joins via IGMP; sender sends to that group (not to the EQAM). For unicast, the working slot (UDP://10.60.30.231:5000, the sender PC) matches the manual's example 3, so the field is the sender's source IP plus the UDP destination port. Likely rule: sender sends unicast to the EQAM GigE IP (10.60.30.47) at dest port N; EQAM matches on src IP + port. Not stated in docs; verify by watching IP indicator. If slot 2 stays red, try the same sender IP with a different port, then try multicast (e.g. 239.x.x.x:5001 with IGMP on the switch). The 420B "Source IP A/B" fields are likely SSM include filters for multicast only (not documented).
