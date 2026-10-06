#!/bin/bash

PUNKTFUNK_LOG_PREFIX=setup
source "$(dirname -- "${BASH_SOURCE[0]}")/punktfunk/launcher-common.sh"

# Bare --browse is upstream's controller-driven host list with discovery,
# on-screen PIN pairing and settings. A terminal dialog cannot render from
# Spruce's graphical port launcher and is not installed on the handheld.
run_session --browse --fullscreen
exit $?
