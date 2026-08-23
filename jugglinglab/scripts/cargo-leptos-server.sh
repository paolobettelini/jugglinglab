#!/usr/bin/env bash

set -euo pipefail

release_build=false
for argument in "$@"; do
    if [[ "$argument" == "--release" ]]; then
        release_build=true
        break
    fi
done

if [[ "$release_build" != "true" ]]; then
    exec cargo "$@"
fi

site_root="${LEPTOS_SITE_ROOT:-dist}"
pkg_dir="${LEPTOS_SITE_PKG_DIR:-pkg}"
output_name="${LEPTOS_OUTPUT_NAME:-juggling_web}"
bundle_dir="$site_root/$pkg_dir"

# cargo-leptos starts frontend and server jobs concurrently. A release server
# must only compile after rust-embed can see the complete, settled dist tree.
deadline=$((SECONDS + 600))
previous_snapshot=""
stable_snapshots=0
first_js_signature=""
first_wasm_signature=""
optimized_wasm_observed=false
minified_js_observed=false

while (( SECONDS < deadline )); do
    if [[ -s "$bundle_dir/$output_name.js" \
        && -s "$bundle_dir/$output_name.wasm" \
        && -s "$bundle_dir/$output_name.css" ]]; then
        js_signature="$(stat --printf='%s\t%y' "$bundle_dir/$output_name.js")"
        wasm_signature="$(stat --printf='%s\t%y' "$bundle_dir/$output_name.wasm")"
        snapshot="$({
            find "$site_root" -type f -printf '%P\t%s\t%T@\n'
        } | sort | sha256sum)"

        if [[ -z "$first_js_signature" ]]; then
            first_js_signature="$js_signature"
            first_wasm_signature="$wasm_signature"
            previous_snapshot="$snapshot"
            sleep 0.1
            continue
        fi

        # wasm-bindgen writes the first bundle, then release processing rewrites
        # the WASM with wasm-opt and the JS with SWC. Both final writes must be
        # observed before rust-embed is allowed to inspect dist.
        if [[ "$wasm_signature" != "$first_wasm_signature" ]]; then
            optimized_wasm_observed=true
        fi
        if [[ "${LEPTOS_JS_MINIFY:-true}" != "true" \
            || "$js_signature" != "$first_js_signature" ]]; then
            minified_js_observed=true
        fi

        if [[ "$optimized_wasm_observed" == "true" \
            && "$minified_js_observed" == "true" \
            && "$snapshot" == "$previous_snapshot" ]]; then
            ((stable_snapshots += 1))
        else
            previous_snapshot="$snapshot"
            stable_snapshots=0
        fi

        if (( stable_snapshots >= 10 )); then
            exec cargo "$@"
        fi
    fi

    sleep 0.1
done

printf 'Timed out waiting for cargo-leptos frontend assets in %s\n' "$site_root" >&2
exit 1
