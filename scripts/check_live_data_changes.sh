#!/usr/bin/env bash
set -euo pipefail

url="${1:-http://localhost:8080/data.json}"
interval_secs="${2:-60}"
windows="${3:-3}"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

if ! command -v sha256sum >/dev/null 2>&1; then
  echo "sha256sum is required" >&2
  exit 1
fi

fingerprint() {
  curl -fsS "$url" \
    | jq -c '[.. | objects | select(has("SensorId")) | {SensorId, Value, Min, Max, RawValue, RawMin, RawMax}] | sort_by(.SensorId)' \
    | sha256sum \
    | awk '{print $1}'
}

timestamp() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

previous="$(fingerprint)"
changed_windows=0

echo "$(timestamp) sample=0 fingerprint=${previous}"

for minute in $(seq 1 "$windows"); do
  sleep "$interval_secs"
  current="$(fingerprint)"
  if [[ "$current" != "$previous" ]]; then
    changed_windows=$((changed_windows + 1))
    status="changed"
  else
    status="unchanged"
  fi

  echo "$(timestamp) sample=${minute} status=${status} fingerprint=${current}"
  previous="$current"
done

if [[ "$changed_windows" -ne "$windows" ]]; then
  echo "FAIL: live data changed in ${changed_windows}/${windows} one-minute windows" >&2
  exit 1
fi

echo "PASS: live data changed in every sampled one-minute window"
