# Front-panel cage -> SerDes lanes (list index = cage - 1), Celestica
# CS8210-32X ("Briggs", Teralynx 7 IVM77700): 32 QSFP-DD cages x 8 lanes.
# Lane = serdes_group * 8 + lane_in_group + 1 (1..256), the Innovium/Marvell
# SONiC numbering (sonic-buildimage Midstone-200i_32x400 port_config.ini:
# serdes group 31 -> lanes 249..).
# SerDes group per cage: the vendor SDK's isgMap (front-panel port order) in
# R3181-J0001-01_V0.1.7_Briggs_SDK/dynamic_config.sh
# (sha256 805eef454263006dc025697640e1b98cc0ecb49b1b328fbe4a5a0a8a2695d1d5),
# the same as the "front panel port N" comments of its cel_configs/isg.yaml
# (377e3b97...). Cage n = front-panel port n = the vendor's port n (fpp<n>):
# a module in cage 17 and a breakout in cage 2 came up on vendor ports 17 and
# 2 and read back through FPGA cage 17 / presence bit 1 (devices/cs8210-32x
# PROGRESS.md 2026-11-10 (o)/(p), 2026-11-11 (x)). Group 32 lane 0 is the
# 10G AUX/mgmt devport 129, not a cage. Cages 1-16 sit behind the Credo
# MACsec PHYs.
[
  [ 209 210 211 212 213 214 215 216 ]  # cage 1: serdes group 26
  [ 217 218 219 220 221 222 223 224 ]  # cage 2: serdes group 27
  [ 193 194 195 196 197 198 199 200 ]  # cage 3: serdes group 24
  [ 201 202 203 204 205 206 207 208 ]  # cage 4: serdes group 25
  [ 145 146 147 148 149 150 151 152 ]  # cage 5: serdes group 18
  [ 153 154 155 156 157 158 159 160 ]  # cage 6: serdes group 19
  [ 105 106 107 108 109 110 111 112 ]  # cage 7: serdes group 13
  [ 113 114 115 116 117 118 119 120 ]  # cage 8: serdes group 14
  [ 49 50 51 52 53 54 55 56 ]  # cage 9: serdes group 6
  [ 57 58 59 60 61 62 63 64 ]  # cage 10: serdes group 7
  [ 9 10 11 12 13 14 15 16 ]  # cage 11: serdes group 1
  [ 17 18 19 20 21 22 23 24 ]  # cage 12: serdes group 2
  [ 241 242 243 244 245 246 247 248 ]  # cage 13: serdes group 30
  [ 185 186 187 188 189 190 191 192 ]  # cage 14: serdes group 23
  [ 161 162 163 164 165 166 167 168 ]  # cage 15: serdes group 20
  [ 81 82 83 84 85 86 87 88 ]  # cage 16: serdes group 10
  [ 121 122 123 124 125 126 127 128 ]  # cage 17: serdes group 15
  [ 41 42 43 44 45 46 47 48 ]  # cage 18: serdes group 5
  [ 225 226 227 228 229 230 231 232 ]  # cage 19: serdes group 28
  [ 233 234 235 236 237 238 239 240 ]  # cage 20: serdes group 29
  [ 169 170 171 172 173 174 175 176 ]  # cage 21: serdes group 21
  [ 177 178 179 180 181 182 183 184 ]  # cage 22: serdes group 22
  [ 129 130 131 132 133 134 135 136 ]  # cage 23: serdes group 16
  [ 137 138 139 140 141 142 143 144 ]  # cage 24: serdes group 17
  [ 249 250 251 252 253 254 255 256 ]  # cage 25: serdes group 31
  [ 1 2 3 4 5 6 7 8 ]  # cage 26: serdes group 0
  [ 89 90 91 92 93 94 95 96 ]  # cage 27: serdes group 11
  [ 97 98 99 100 101 102 103 104 ]  # cage 28: serdes group 12
  [ 65 66 67 68 69 70 71 72 ]  # cage 29: serdes group 8
  [ 73 74 75 76 77 78 79 80 ]  # cage 30: serdes group 9
  [ 25 26 27 28 29 30 31 32 ]  # cage 31: serdes group 3
  [ 33 34 35 36 37 38 39 40 ]  # cage 32: serdes group 4
]
