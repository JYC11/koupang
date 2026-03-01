
#!/bin/bash
set -e

SERVICES=(shared identity catalog order payment shipping notification review moderation)

# Parse arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 <service-name|all>"
    echo ""
    echo "Available services:"
    for service in "${SERVICES[@]}"; do
        echo "  - $service"
    done
    echo "  - all (run tests for all services)"
    exit 1
fi

# Determine which services to test
if [ "$1" == "all" ]; then
    services_to_test=("${SERVICES[@]}")
else
    services_to_test=("$1")
fi

failed=()

for service in "${services_to_test[@]}"; do
    if [ ! -d "$service" ]; then
        echo "Error: Service '$service' not found"
        exit 1
    fi

    if [ ! -f "$service/Cargo.toml" ]; then
        echo "Error: $service has no Cargo.toml"
        exit 1
    fi

    echo "========================================="
    echo "Running tests for: $service"
    echo "========================================="

    # shared requires test-utils feature for integration tests
    if [ "$service" == "shared" ]; then
        CARGO_CMD="cargo test -p shared --features test-utils -- --test-threads=1"
    else
        CARGO_CMD="cargo test -p $service -- --test-threads=1"
    fi

    if eval "$CARGO_CMD"; then
        echo "$service: PASSED"
    else
        echo "$service: FAILED"
        failed+=("$service")
    fi

    echo ""
done

echo "========================================="
if [ ${#failed[@]} -eq 0 ]; then
    echo "All tests passed."
else
    echo "Failed services: ${failed[*]}"
    exit 1
fi