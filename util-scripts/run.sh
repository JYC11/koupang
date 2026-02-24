#!/bin/bash
set -e

DB_USER="admin"
DB_PASS="1234"
DB_HOST="localhost"
DB_PORT="5432"
REDIS_URL="redis://localhost:6379"

ACCESS_TOKEN_SECRET="dev-access-secret-key-change-in-production"
REFRESH_TOKEN_SECRET="dev-refresh-secret-key-change-in-production"
ACCESS_TOKEN_EXPIRY="3600"
REFRESH_TOKEN_EXPIRY="604800"
DB_MAX_CONNECTIONS="5"

SERVICES=(identity catalog order payment shipping notification review moderation)

get_http_port() {
    case "$1" in
        identity)     echo 3001 ;;
        catalog)      echo 3002 ;;
        order)        echo 3003 ;;
        payment)      echo 3004 ;;
        shipping)     echo 3005 ;;
        notification) echo 3006 ;;
        review)       echo 3007 ;;
        moderation)   echo 3008 ;;
    esac
}

get_grpc_port() {
    case "$1" in
        identity) echo 50051 ;;
        *)        echo "" ;;
    esac
}

# Parse arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 <service-name>"
    echo ""
    echo "Available services:"
    for service in "${SERVICES[@]}"; do
        port=$(get_http_port "$service")
        echo "  - $service (port $port)"
    done
    exit 1
fi

SERVICE="$1"

# Validate service exists
if [ ! -d "$SERVICE" ]; then
    echo "Error: Service '$SERVICE' not found"
    exit 1
fi

if [ ! -f "$SERVICE/Cargo.toml" ]; then
    echo "Error: $SERVICE has no Cargo.toml"
    exit 1
fi

PORT=$(get_http_port "$SERVICE")
GRPC_PORT=$(get_grpc_port "$SERVICE")
SERVICE_UPPER=$(echo "$SERVICE" | tr '[:lower:]' '[:upper:]')
DB_URL="postgres://${DB_USER}:${DB_PASS}@${DB_HOST}:${DB_PORT}/${SERVICE}"

echo "========================================="
echo "Starting: $SERVICE"
echo "HTTP port: $PORT"
if [ -n "$GRPC_PORT" ]; then
    echo "gRPC port: $GRPC_PORT"
fi
echo "DB: $DB_URL"
echo "========================================="

export "${SERVICE_UPPER}_DB_URL=$DB_URL"
export "${SERVICE_UPPER}_PORT=$PORT"
export ACCESS_TOKEN_SECRET
export REFRESH_TOKEN_SECRET
export ACCESS_TOKEN_EXPIRY
export REFRESH_TOKEN_EXPIRY
export REDIS_URL
export DB_MAX_CONNECTIONS

# Set gRPC port if the service has one
if [ -n "$GRPC_PORT" ]; then
    export "${SERVICE_UPPER}_GRPC_PORT=$GRPC_PORT"
fi

cargo run -p "$SERVICE"
