#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARTIFACT_DIR="${ROOT_DIR}/artifacts"
STAGE_DIR="${ROOT_DIR}/.artifact-stage/hvac_simulator"
PACKAGE_SRC="${ROOT_DIR}/ros2_ws/src/hack_to_the_future_hvac"
ARTIFACT_PATH="${ARTIFACT_DIR}/hvac_simulator.tar.gz"

rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/src" "${ARTIFACT_DIR}"

cp -R "${PACKAGE_SRC}" "${STAGE_DIR}/src/hack_to_the_future_hvac"
cp "${ROOT_DIR}/artifact-run.sh" "${STAGE_DIR}/run.sh"
chmod +x "${STAGE_DIR}/run.sh"

tar -C "${STAGE_DIR}" -czf "${ARTIFACT_PATH}" .
sha256sum "${ARTIFACT_PATH}" | awk '{print $1}'
