
#!/bin/bash
set -e

DB_USER="admin"
DB_PASS="1234"
DB_HOST="localhost"
DB_PORT="5432"

SERVICES=(identity catalog order payment shipping notification review moderation)

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

    export DATABASE_URL="postgres://${DB_USER}:${DB_PASS}@${DB_HOST}:${DB_PORT}/${service}"

    if cargo test -p "$service" -- --test-threads=1; then
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