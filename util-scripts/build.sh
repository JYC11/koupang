#!/bin/bash
set -e

SERVICES=(shared identity catalog order payment shipping notification review moderation)

# Parse arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 <service-name|all> [--release]"
    echo ""
    echo "Available services:"
    for service in "${SERVICES[@]}"; do
        echo "  - $service"
    done
    echo "  - all (build all services)"
    echo ""
    echo "Options:"
    echo "  --release   Build in release mode"
    exit 1
fi

SERVICE="$1"
RELEASE=""

for arg in "$@"; do
    if [ "$arg" == "--release" ]; then
        RELEASE="--release"
    fi
done

# Determine which services to build
if [ "$SERVICE" == "all" ]; then
    services_to_build=("${SERVICES[@]}")
else
    services_to_build=("$SERVICE")
fi

failed=()

for service in "${services_to_build[@]}"; do
    if [ ! -d "$service" ]; then
        echo "Error: Service '$service' not found"
        exit 1
    fi

    if [ ! -f "$service/Cargo.toml" ]; then
        echo "Error: $service has no Cargo.toml"
        exit 1
    fi

    echo "========================================="
    echo "Building: $service"
    echo "========================================="

    # shared requires test-utils feature for full build
    if [ "$service" == "shared" ]; then
        FEATURES="--features test-utils"
    else
        FEATURES=""
    fi

    if cargo build -p "$service" $FEATURES $RELEASE; then
        echo "$service: OK"
    else
        echo "$service: FAILED"
        failed+=("$service")
    fi

    echo ""
done

echo "========================================="
if [ ${#failed[@]} -eq 0 ]; then
    echo "All builds passed."
else
    echo "Failed services: ${failed[*]}"
    exit 1
fi
