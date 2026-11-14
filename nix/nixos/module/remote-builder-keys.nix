# Shared by remote-builder-host.nix (server side) and remote-builders.nix
# (client side). Public material only -- the client private key lives at
# /root/.ssh/nix-remote-builder on each client and is never committed.
{
  # Generated 2026-11-14 on sea420-desktop (root). Copy the private half to a
  # client's /root/.ssh/nix-remote-builder to let it build on the desktop.
  clientKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFIjD6qaEUgs2DbnHLOJykr0z9+wo4dkI7/PaWHNtPIV nix-remote-builder@sea420-desktop";

  sea420-desktop = {
    hostName = "sea420-desktop.tail285d8f.ts.net";
    # base64 of /persist/etc/ssh/ssh_host_ed25519_key.pub, so clients need no
    # known_hosts entry (root's known_hosts is wiped with impermanence anyway).
    publicHostKey = "c3NoLWVkMjU1MTkgQUFBQUMzTnphQzFsWkRJMU5URTVBQUFBSUNUSjBnZlR0VFVIREJBOGFxSGJoS1JEZ1NSWTgyWEk4dDlqTWRxc21udEcgcm9vdEBzZWE0MjAtZGVza3RvcA==";
  };
}
