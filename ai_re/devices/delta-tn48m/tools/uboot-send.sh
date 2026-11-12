#!/usr/bin/env bash
# usage: uboot-send.sh <file-of-uboot-lines>  -- types each line at the Marvell>> prompt, waiting for it
T=serial-tn48m
while IFS= read -r line; do
  [ -z "$line" ] && continue
  tmux send-keys -t $T -l "$line"; tmux send-keys -t $T Enter
  for i in $(seq 1 100); do sleep 0.1; tmux capture-pane -t $T -p | grep -v '^\s*$' | tail -1 | grep -q '^Marvell>> *$' && break; done
done < "$1"
