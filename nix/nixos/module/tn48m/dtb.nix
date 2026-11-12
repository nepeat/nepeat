{
  runCommand,
  dtc,
  gcc,
  kernelSrc,
  dts ? ./delta-tn48m.dts,
  name ? "delta-tn48m",
}:

let
  raw =
    runCommand "${name}.dtb"
      {
        nativeBuildInputs = [ dtc ];
        inherit dts kernelSrc;
      }
      ''
        set -euo pipefail
        mkdir -p kernel-includes kernel-dts
        tar xf "$kernelSrc" --strip-components=1 -C kernel-includes \
          --wildcards '*/include/dt-bindings'
        tar xf "$kernelSrc" --strip-components=6 -C kernel-dts \
          --wildcards '*/arch/arm64/boot/dts/marvell/*'
        ${gcc}/bin/cpp -nostdinc -undef -x assembler-with-cpp \
          -I kernel-dts -I kernel-includes/include \
          -o preprocessed.dts "$dts"
        dtc -I dts -O dtb -o "$out" preprocessed.dts
      '';
in
runCommand "${name}-dtbs" { } ''
  mkdir -p "$out/marvell"
  cp ${raw} "$out/marvell/${name}.dtb"
''
