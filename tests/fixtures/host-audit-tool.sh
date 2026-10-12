#!/bin/sh
printf '%s\n' 'host stdout'
printf '%s\n' 'host stderr' >&2
exit "$1"
