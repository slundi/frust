#!/bin/bash

#sudo apt-get install yui-compressor

yui-compressor static/main.js > static/main.min.js
