#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(realpath "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)")"
cd "${ROOT_DIR}"

section() {
  printf "\n== %s ==\n" "$1"
}

note() {
  printf "%s\n" "$1"
}

show_target_lock_owners() {
  section "Build Directory Lock"

  if [[ ! -f target/debug/.cargo-lock ]]; then
    note "target/debug/.cargo-lock not found yet."
    return
  fi

  if ! command -v lsof >/dev/null 2>&1; then
    note "lsof not available; skipping lock owner detection."
    return
  fi

  local owners
  owners="$(
    {
      lsof target/debug/.cargo-lock 2>/dev/null || true
    } | awk 'NR == 1 || /cargo|clippy|rustc/'
  )"

  if [[ -z "${owners}" ]]; then
    note "No Cargo-related processes currently hold target/debug/.cargo-lock."
    return
  fi

  printf "%s\n" "${owners}"
}

show_background_cargo() {
  section "Background Cargo Processes"

  local matches
  matches="$(ps -Ao pid,ppid,etime,pcpu,pmem,command | awk '/cargo |cargo-clippy|clippy-driver|rustc/ && $0 !~ /awk/ && $0 !~ /doctor-compile/ { print }')"

  if [[ -z "${matches}" ]]; then
    note "No background Cargo processes detected."
    return
  fi

  printf "%s\n" "${matches}" | sed -n '1,40p'
}

show_sccache_stats() {
  section "sccache"

  if ! command -v sccache >/dev/null 2>&1; then
    note "sccache not installed."
    return
  fi

  sccache --show-stats \
    | awk '
        /^Compile requests/ ||
        /^Cache hits/ ||
        /^Cache misses/ ||
        /^Cache hits rate/ ||
        /^Cache hit rate/ ||
        /^Non-cacheable/ ||
        /^Average cache write/ ||
        /^Average compiler/ ||
        /^Failed distributed compilations/ ||
        /^Failed compilations/ ||
        /^Cache location/
      '
}

measure_exec_seconds() {
  python3 - "$1" <<'PY'
import subprocess
import sys
import time

binary = sys.argv[1]
start = time.perf_counter()
completed = subprocess.run([binary], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
elapsed = time.perf_counter() - start
print(f"{elapsed:.3f}")
sys.exit(completed.returncode)
PY
}

show_macos_gatekeeper_probe() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    return
  fi

  section "macOS Execution Probe"

  if ! command -v cc >/dev/null 2>&1; then
    note "cc not available; skipping macOS probe."
    return
  fi

  local tmpdir
  tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/openfang-compile-doctor.XXXXXX")"

  cat >"${tmpdir}/probe.c" <<'EOF'
int main(void) {
  return 0;
}
EOF

  cc -x c "${tmpdir}/probe.c" -o "${tmpdir}/probe"

  local provenance="absent"
  if xattr -l "${tmpdir}/probe" 2>/dev/null | awk '/com.apple.provenance/ { found = 1 } END { exit !found }'; then
    provenance="present"
  fi

  local first_run second_run syspolicyd_cpu
  first_run="$(measure_exec_seconds "${tmpdir}/probe")"
  second_run="$(measure_exec_seconds "${tmpdir}/probe")"
  syspolicyd_cpu="$(
    ps -Ao pcpu=,command= | awk '
      BEGIN { cpu = "0.0" }
      /syspolicyd/ && $0 !~ /awk/ && cpu == "0.0" { cpu = $1 }
      END { print cpu }
    '
  )"
  syspolicyd_cpu="${syspolicyd_cpu:-0.0}"

  printf "Probe xattr com.apple.provenance: %s\n" "${provenance}"
  printf "First launch: %ss\n" "${first_run}"
  printf "Second launch: %ss\n" "${second_run}"
  printf "syspolicyd CPU snapshot: %s%%\n" "${syspolicyd_cpu}"

  python3 - "${first_run}" "${second_run}" "${syspolicyd_cpu}" <<'PY'
import sys

first = float(sys.argv[1])
second = float(sys.argv[2])
syspolicyd = float(sys.argv[3])

if first >= 0.05 and (second <= first * 0.5 or syspolicyd >= 10.0):
    print()
    print("Diagnosis: macOS execution policy is adding first-launch latency to freshly built binaries.")
    print("If Cargo appears to hang on `build-script-build` and `syspolicyd` stays hot,")
    print("add your terminal or editor to System Settings > Privacy & Security > Developer Tools.")
    print("For Terminal.app specifically, `spctl developer-mode enable-terminal` is available on this host.")
else:
    print()
    print("Diagnosis: the quick probe did not detect a strong first-launch Gatekeeper penalty.")
PY

  rm -rf "${tmpdir}"
}

main() {
  note "OpenFang compile doctor"
  note "Repository: ${ROOT_DIR}"

  show_target_lock_owners
  show_background_cargo
  show_sccache_stats
  show_macos_gatekeeper_probe
}

main "$@"
