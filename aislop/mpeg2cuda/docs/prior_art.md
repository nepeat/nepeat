# Prior art: GPU MPEG-2 encoding (web research, 2026-10-16)

Caveat: the search tool was shallow. Several claims below could not be verified and are labelled so. No throughput figures for any GPU MPEG-2 encoder were found.

## Verified (with sources)
- Montero, Taibo, Gulias, Rivas (Univ. A Coruna), "Parallel zigzag scanning and Huffman coding for a GPU-based MPEG-2 encoder", IEEE ISM 2010 pp.97-104. Hybrid parallel/serial entropy coding beats both pure serial and fully parallel once CPU transfer cost counts. Abstract gave no speedup numbers, nothing on interlace/HD/ME in the page I saw. https://portalinvestigacion.udc.gal/documentos/609c27351aec1f036bb26913
- Same group: "Optimising lossless stages in a GPU-based MPEG encoder", Multimedia Tools & Applications 65(3), 2013, pp.495-520, doi 10.1007/s11042-012-1053-9. Also "GPU-based fast motion estimation for on-the-fly encoding of computer-generated video streams", NOSSDAV 2011 (titles only; not read). https://katalog.hcu-hamburg.de/vufind/Search2Record/OLC2035007747
- NVIDIA NVCUVENC / MainConcept CUDA SDK: H.264 only as output (MPEG-2 only as an input format for transcode). MainConcept's first GPU encoder was 2010. https://developer.nvidia.com/video-processing-software-solutions-nvidia-developers
- Elemental (2008-2011): GPU (CUDA, Quadro/Tesla) appliances. Elemental Server lists MPEG-2/VC-1/H.264 output and CableLabs-compliant MPEG-2 TS; Elemental Accelerator (Premiere plugin) v2.2 advertised GPU MPEG-2 encode. 1080i was not confirmed in any source I got; internals (how much of MPEG-2 ran on GPU) not public. https://www.streamingmediaglobal.com/Articles/News/Featured-News/Elemental-Introduces-GPU-Based-Video-Processing-System-65479.aspx ; https://tvtechnology.com/equipment/elemental-technologies-releases-transcoding-appliance
- x262 (x264 fork): per-macroblock frame/field adaptation listed; repo dormant since 2015; FFmpeg removed libx262 wrapper in 2022; 2020 report says 4:2:2 interlaced missing. https://ffmpeg.org/pipermail/ffmpeg-devel/2022-May/296910.html ; https://www-test.videolan.org/projects/x262/
- Intel QSV: FFmpeg wiki lists MPEG-2 encode starting Ivy Bridge (gen7). Debian/Ubuntu intel-media-va-driver (iHD) lists only JPEG/AVC/HEVC/VP9 encode, i.e. no MPEG-2 encode in the modern driver. https://trac.ffmpeg.org/wiki/Hardware/QuickSync ; https://packages.debian.org/sid/intel-media-va-driver
- Patents (not papers) on GPU motion estimation naming MPEG-2: US 8,275,049 / 9,319,708 / 8,861,591 ("Software video encoder with GPU acceleration").
- 2008 NVIDIA forum thread on CUDA ME for MPEG-2/H.264 encoders (only discussion). https://forums.developer.nvidia.com/t/motion-estimation-on-gpu/1579
- Fixed-function/embedded figures: Bluefish444 RAGE N|Code (2006) claims real-time 1080i60 (card); TI DM368 ~27 fps 1440x1080i, ~17 fps 1080 (vendor forum). https://www.cgw.com/Press-Center/News/2006/Bluefish444-Unveils-HD-MPEG-2-Video-Compression-.aspx ; https://e2e.ti.com/support/processors-group/processors/f/processors-forum/139796/dm368-mpeg2-encoder-performs-1080i30
- GitHub/SourceForge search: no CUDA/OpenCL MPEG-2 encoder repo found. A forum mention of "gpeg2" (GPU VFW MPEG-2 encoder), site gone. https://forum.videohelp.com/showthread.php?p=2088871

## Table
| Name | Year | Type | MPEG-2 enc? | 1080i? | Notes |
|---|---|---|---|---|---|
| Montero et al. (A Coruna) | 2010-13 | GPU (CUDA-era) | yes (research) | unknown | DCT, zigzag, VLC on GPU; closest academic work |
| Elemental Server/Accelerator | 2008-11 | GPU + CPU | yes | unconfirmed | commercial; CableLabs MPEG-2 TS |
| NVCUVENC / MainConcept CUDA | 2008-10 | GPU | no (H.264) | n/a | |
| Intel QSV/VAAPI | 2012+ | ASIC | yes on old gens, dropped in iHD | unverified | |
| x262 | 2011-15 | CPU SIMD | yes | partial | dead |
| Bluefish444 RAGE N/Code | 2006 | HW card | yes | yes, claimed RT | |
| TI DM36x | ~2011 | DSP | yes | ~27 fps | |
| AMD VCN / NVENC | - | ASIC | no (inference from codec tables) | - | not directly verified |

## Not verified (training-memory inference, treat with care)
- Avivo Video Converter: GPU-assisted H.264/WMV/MPEG-4; MPEG-2 encode not confirmed.
- Intel Gen6-9 MPEG-2 encode in legacy i965 VAAPI driver, interlace support unknown.
- Harmonic/Envivio/Ateme/Thor/ZeeVee/Blonder Tongue: mostly ASIC/FPGA/DSP or x86 CPU MPEG-2; I found no source confirming any GPU MPEG-2 for QAM headends.
- Numbers for Cinema Craft, MainConcept, HC, IPP: none found.

## Assessment (mine, not sourced)
- Novelty: high as a combination. Prior GPU MPEG-2 work found is 2008-2013, partial (DCT/ME/VLC pieces) or closed (Elemental). A fully on-GPU, decoder-bit-exact-recon, field-ME, B-frame, VBV-safe CBR encoder integrated in FFmpeg at 1080i with 2200-3100 fps is, as far as I can find, unpublished. Absence of evidence from a shallow search; say "no prior art we found" not "first".
- Comparable CPU numbers: none found for SIMD 1080i beyond your own FFmpeg baseline (300-480 fps/stream; 700-1260 aggregate on 7950X), which is likely at or near the best public figure. x262 is dead and no benchmark was found.
- Worth borrowing: Montero's hybrid VLC (parallel per-block code lengths plus prefix-sum bit placement, serial only for slice headers); GPU full-search/hierarchical ME from the H.264 CUDA literature (Monteiro SBAC-PAD 2011, https://sol.sbc.org.br/index.php/sbac-pad/article/view/34707); x262-style per-MB frame/field decision for rate-distortion; statmux-style lookahead CBR as in Elemental.
- Suggested next searches: IEEE/ACM for Montero NOSSDAV 2011 numbers; Intel media-driver docs for MPEG-2 encode gens; Elemental datasheets for 1080i.
