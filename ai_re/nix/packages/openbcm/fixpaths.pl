#!/usr/bin/env perl
# Replace hard-coded tool paths in the SDK's makefiles with PATH lookups.
use strict; use warnings;
for my $f (@ARGV) {
  local $/; open(my $h, '<', $f) or next; my $s = <$h>; close $h;
  my $o = $s;
  $s =~ s{(?<![\w/.\$\(\{-])/bin/(echo|rm|cp|mkdir|ln|touch|sed|printf|pwd|ls|cat|mv)\b}{$1}g;
  $s =~ s{(?<![\w/.\$\(\{-])/usr/bin/(test|gcc|gzip|compress)\b}{$1}g;
  if ($s ne $o) { open(my $w, '>', $f) or die; print $w $s; close $w; print "fixed $f\n"; }
}
