{
  # Big endian Book E, NOT POWER8 / ppc64le. ELFv2 is the maintained BE
  # userspace ABI; it does not itself require little endian or VSX.
  config = "powerpc64-unknown-linux-gnuabielfv2";
  gcc.cpu = "e6500";
  gcc.tune = "e6500";
}
