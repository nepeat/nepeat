# One run of a node-exporter textfile producer: run the command under a
# timeout, publish its stdout atomically as <directory>/<name>.prom with
# textfile_producer_* gauges appended, and delete the file on any failure so
# a broken or hung producer never leaves old values behind.
{ writeShellScript, coreutils }:
{ name, command, directory, timeout }:
writeShellScript "textfile-producer-${name}" ''
  set -u
  PATH=${coreutils}/bin:$PATH
  out=${directory}/${name}.prom
  tmp=${directory}/.${name}.prom.tmp
  rm -f -- "$tmp"
  start=$(date +%s%N)
  rc=0
  timeout --kill-after=5 ${toString timeout} ${command} > "$tmp" || rc=$?
  if [ "$rc" -eq 0 ] && [ ! -s "$tmp" ]; then rc=1; echo "${name}: no output" >&2; fi
  if [ "$rc" -ne 0 ]; then
    rm -f -- "$tmp" "$out"
    if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then echo "${name}: timed out after ${toString timeout}s" >&2; fi
    echo "${name}: failed (exit $rc), removed $out" >&2
    exit "$rc"
  fi
  end=$(date +%s%N)
  ns=$((end - start))
  if [ -n "$(tail -c 1 "$tmp")" ]; then echo >> "$tmp"; fi
  cat >> "$tmp" <<EOF
  # HELP textfile_producer_last_success_timestamp_seconds Unix time the textfile producer last completed successfully.
  # TYPE textfile_producer_last_success_timestamp_seconds gauge
  textfile_producer_last_success_timestamp_seconds{producer="${name}"} $((end / 1000000000)).$(printf %09d $((end % 1000000000)))
  # HELP textfile_producer_duration_seconds Run time of the textfile producer's last successful run.
  # TYPE textfile_producer_duration_seconds gauge
  textfile_producer_duration_seconds{producer="${name}"} $((ns / 1000000000)).$(printf %09d $((ns % 1000000000)))
  EOF
  chmod 0644 "$tmp"
  mv -f -- "$tmp" "$out"
''
