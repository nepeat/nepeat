# What every system in the lab shares, imported by each configs/<host>/configuration.nix:
# who may log in, the time zone, and a management port that takes DHCP. Identity
# (host name, MACs, ports) and hardware stay with the host and its board module.
{ config, lib, ... }:
{
  options.lab.managementInterface = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = "eth0";
    description = ''
      Interface that is the management port: DHCP (v4 by MAC, v6 by RA), never
      waited for. null: the host defines its own systemd.network unit.
    '';
  };
  config = {
    time.timeZone = "America/Los_Angeles";
    # Root keys: https://github.com/nepeat.keys (fetched 2026-11-09) plus root on
    # sea420-desktop, the build/deploy host. The same list as tn48m/cs8210 (nepeat/nix).
    users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
    ];
    # The serial console is physical access (it already means U-Boot access).
    services.getty.autologinUser = "root";

    networking.useDHCP = false;
    networking.useNetworkd = true;
    networking.usePredictableInterfaceNames = false;
    systemd.network = {
      enable = true;
      wait-online.enable = false;
      networks."10-management" = lib.mkIf (config.lab.managementInterface != null) {
        matchConfig.Name = config.lab.managementInterface;
        networkConfig = { DHCP = "yes"; IPv6AcceptRA = true; };
        dhcpV4Config.ClientIdentifier = "mac";
        linkConfig.RequiredForOnline = "no";
      };
    };
    networking.firewall.allowedTCPPorts = [ 22 ];
    networking.nftables.enable = true;
  };
}
