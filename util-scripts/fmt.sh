#!/bin/bash
set -e

SERVICES=(shared identity catalog order payment shipping notification review moderation)

# Parse arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 <service-name|all> [--check]"
    echo ""
    echo "Available services:"
    for service in "${SERVICES[@]}"; do
        echo "  - $service"
    done
    echo "  - all (format all services)"
    echo ""
    echo "Options:"
    echo "  --check    Check formatting without modifying files (for CI)"
    exit 1
fi

SERVICE="$1"
CHECK=""

for arg in "$@"; do
    if [ "$arg" == "--check" ]; then
        CHECK="--check"
    fi
done

# Determine which services to format
if [ "$SERVICE" == "all" ]; then
    services_to_fmt=("${SERVICES[@]}")
else
    services_to_fmt=("$SERVICE")
fi

failed=()

for service in "${services_to_fmt[@]}"; do
    if [ ! -d "$service" ]; then
        echo "Error: Service '$service' not found"
        exit 1
    fi

    if [ ! -f "$service/Cargo.toml" ]; then
        echo "Error: $service has no Cargo.toml"
        exit 1
    fi

    echo "========================================="
    if [ -n "$CHECK" ]; then
        echo "Checking format: $service"
    else
        echo "Formatting: $service"
    fi
    echo "========================================="

    if cargo fmt -p "$service" $CHECK; then
        echo "$service: OK"
    else
        echo "$service: FAILED"
        failed+=("$service")
    fi

    echo ""
done

echo "========================================="
if [ ${#failed[@]} -eq 0 ]; then
    if [ -n "$CHECK" ]; then
        echo "All format checks passed."
    else
        echo "All services formatted."
    fi
else
    echo "Failed services: ${failed[*]}"
    exit 1
fi
