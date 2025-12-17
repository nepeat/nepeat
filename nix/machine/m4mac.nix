{ pkgs, lib, ... }:
{
nix.buildMachines = [ {
    hostName = "10g.warc.zip";
    sshUser = "erin";
    system = "x86_64-linux";
    protocol = "ssh-ng";
    maxJobs = 8;
    supportedFeatures = [ "nixos-test" "benchmark" "big-parallel" ];
    mandatoryFeatures = [ ];
}] ;
}