#!/bin/bash
set -e

SERVICES=(shared identity catalog order payment shipping notification review moderation)

# Parse arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 <service-name|all> [--clippy]"
    echo ""
    echo "Available services:"
    for service in "${SERVICES[@]}"; do
        echo "  - $service"
    done
    echo "  - all (check all services)"
    echo ""
    echo "Options:"
    echo "  --clippy    Also run clippy lints (with -D warnings)"
    exit 1
fi

SERVICE="$1"
RUN_CLIPPY=false

for arg in "$@"; do
    if [ "$arg" == "--clippy" ]; then
        RUN_CLIPPY=true
    fi
done

# Determine which services to check
if [ "$SERVICE" == "all" ]; then
    services_to_check=("${SERVICES[@]}")
else
    services_to_check=("$SERVICE")
fi

failed=()

for service in "${services_to_check[@]}"; do
    if [ ! -d "$service" ]; then
        echo "Error: Service '$service' not found"
        exit 1
    fi

    if [ ! -f "$service/Cargo.toml" ]; then
        echo "Error: $service has no Cargo.toml"
        exit 1
    fi

    echo "========================================="
    echo "Checking: $service"
    echo "========================================="

    # shared requires test-utils feature for full checking
    if [ "$service" == "shared" ]; then
        FEATURES="--features test-utils"
    else
        FEATURES=""
    fi

    if ! cargo check -p "$service" $FEATURES; then
        echo "$service: CHECK FAILED"
        failed+=("$service")
        continue
    fi

    if [ "$RUN_CLIPPY" == true ]; then
        echo "Running clippy for $service..."
        if ! cargo clippy -p "$service" $FEATURES -- -D warnings; then
            echo "$service: CLIPPY FAILED"
            failed+=("$service")
            continue
        fi
    fi

    echo "$service: OK"
    echo ""
done

echo "========================================="
if [ ${#failed[@]} -eq 0 ]; then
    echo "All checks passed."
else
    echo "Failed services: ${failed[*]}"
    exit 1
fi
