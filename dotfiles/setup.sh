#!/usr/bin/env bash

set -x

# install homebrew if not found
if ! command -v brew &> /dev/null; then
  ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install)"
fi

# install brew packages
xargs brew -v install < brew/formula
xargs brew -v install --cask < brew/casks

# link code -> vscode
if ! command -v vscode &> /dev/null; then
    sudo ln -svf /Applications/Visual\ Studio\ Code\ -\ Insiders.app/Contents/Resources/app/bin/code /usr/local/bin/vscode
fi

# install rust stable + nightly
if ! command -v rustc &> /dev/null
then
    rustup-init -v --no-modify-path --default-toolchain nightly
    source "$HOME/.cargo/env"
    rustup install stable
fi

# create nvm folder and install nodejs lts
if ! command -v node &> /dev/null
then
mkdir ~/.nvm
export NVM_DIR="$HOME/.nvm"
  [ -s "/opt/homebrew/opt/nvm/nvm.sh" ] && \. "/opt/homebrew/opt/nvm/nvm.sh"  # This loads nvm
  [ -s "/opt/homebrew/opt/nvm/etc/bash_completion.d/nvm" ] && \. "/opt/homebrew/opt/nvm/etc/bash_completion.d/nvm"  # This loads nvm bash_completion
nvm install --lts
fi

# enable dark mode + ui tweaks
osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true'
defaults -currentHost write com.apple.controlcenter.plist BatteryShowPercentage -bool true
defaults -currentHost write com.apple.menuextra.clock.plist ShowSeconds -bool true
defaults write com.apple.dock "autohide" -bool "true"

# kill after settings updates
killall Dock

if [[ -d "${INTERNAL_DOTFILES}" ]]; then
    cd "${INTERNAL_DOTFILES}"
    make setup
fi