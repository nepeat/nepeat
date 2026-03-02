{ lib, ... }:
{
    # Set SSH signer to be macOS 1Password
    programs.git.settings = {
        "gpg \"ssh\"" = lib.mkDefault {
            program = "/Applications/1Password.app/Contents/MacOS/op-ssh-sign";
        };
    };
}
