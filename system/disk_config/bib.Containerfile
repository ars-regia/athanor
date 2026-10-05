# The bootc-image-builder that forge/scripts/build_iso.sh runs, pinned by digest. Nothing
# builds this file: it exists so that forge/specs/azoth/bump.py (group "system") moves the
# pin the way it moves every other base image, with a pull request a person reviews.
FROM quay.io/centos-bootc/bootc-image-builder:latest@sha256:2b52843ea2bfda73b0a08d97e76b734393b1d3a804681b9fabb26723bd3a2f0b AS bootc-image-builder
