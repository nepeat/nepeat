#!/bin/sh

python3 -m virtualenv -p python3 virtualenv
source virtualenv/bin/activate
pip install -r requirements.txt
