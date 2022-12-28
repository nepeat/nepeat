#!/usr/bin/env bash

set -x

# setup script inspired from https://github.com/jpalardy/dotfiles/blob/main/.setup.bash
DOTFILES=$(pwd)
first=1

relink() {
	if [ "$(readlink "$BASE_DIR/$1")" = "$2" ]; then
		return
	fi
	if [[ -e "$BASE_DIR/$1" || -L "$BASE_DIR/$1" ]]; then
		rm -fv $BASE_DIR/$1
	fi
	ln -sv $2 $BASE_DIR/$1
}

BASE_DIR="$HOME"

## shell stuff
relink	.profile 		"$DOTFILES/profile"
relink	.bash_profile	"$DOTFILES/bash_profile"
relink	.zshrc			"$DOTFILES/zshrc"

## git
relink	.gitconfig	"$DOTFILES/git/gitconfig"
relink	.gitignore	"$DOTFILES/git/gitignore"
