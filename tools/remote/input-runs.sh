#!/usr/bin/env bash
# The real-input runs of the VMware disk on a builder, some of them several times (a hang that shows only
# now and then): bash tools/remote/input-runs.sh <repeat> <run>...   e.g. input-runs.sh 4 usb-xga bt
# Through tools/remote/rb.sh; prints every run's verdict and the lines of the failed ones.
set -uo pipefail
rep=${1:?repeat}; shift
INPUT_RUNS="$*" REPEAT=$rep bash tools/vm/vmware-input-e2e.sh
echo "INPUT-RUNS-RC $?"
