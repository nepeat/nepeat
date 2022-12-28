#!/usr/bin/env bash

set -x

# enable dark mode + ui tweaks
osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true'
defaults -currentHost write com.apple.controlcenter.plist BatteryShowPercentage -bool true
defaults -currentHost write com.apple.menuextra.clock.plist ShowSeconds -bool true
defaults write com.apple.dock "autohide" -bool "true"

# kill after settings updates
killall Dock

# install homebrew if not found
if ! command -v brew &> /dev/null; then
	ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install)"
fi

# install brew packages
brew bundle --file=brew/Brewfile

# link code -> vscode
if ! command -v vscode &> /dev/null; then
	sudo ln -svf /Applications/Visual\ Studio\ Code\ -\ Insiders.app/Contents/Resources/app/bin/code /usr/local/bin/vscode
fi

# install rust stable + nightly
if ! command -v rustc &> /dev/null; then
	rustup-init -v --no-modify-path --default-toolchain nightly
	source "$HOME/.cargo/env"
	rustup install stable
fi

# create nvm folder and install nodejs lts
if ! command -v node &> /dev/null; then
	mkdir ~/.nvm
	export NVM_DIR="$HOME/.nvm"
	[ -s "/opt/homebrew/opt/nvm/nvm.sh" ] && \. "/opt/homebrew/opt/nvm/nvm.sh"  # This loads nvm
	[ -s "/opt/homebrew/opt/nvm/etc/bash_completion.d/nvm" ] && \. "/opt/homebrew/opt/nvm/etc/bash_completion.d/nvm"  # This loads nvm bash_completion
	nvm install --lts
fi

# install rvm + latest ruby
gpg --recv-keys 409B6B1796C275462A1703113804BB82D39DC0E3 7D2BAF1CF37B13E2069D6956105BD0E739499BDB
if ! command -v rvm &> /dev/null; then
	curl -sSL https://get.rvm.io | bash -s stable
	[[ -s "$HOME/.rvm/scripts/rvm" ]] && source "$HOME/.rvm/scripts/rvm"
	rvm install ruby
fi

if [[ -d "${INTERNAL_DOTFILES}" ]]; then
	cd "${INTERNAL_DOTFILES}"
	make setup
fi
