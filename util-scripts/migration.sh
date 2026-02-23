#!/bin/bash
set -e

SERVICES=(identity catalog order payment shipping notification review moderation)

# Parse arguments or prompt for service
if [ $# -eq 0 ]; then
    echo "Available services:"
    for service in "${SERVICES[@]}"; do
        echo "  - $service"
    done
    echo ""
    read -p "Enter service name: " service_name
else
    service_name="$1"
fi

# Validate service exists
if [ ! -d "$service_name" ]; then
    echo "Error: Service '$service_name' not found"
    exit 1
fi

# Prompt for migration name
read -p "Enter migration name: " migration_name

# Generate timestamp
datetime=$(date '+%Y%m%d%H%M')

# Create filename
filename="${datetime}_${migration_name}.sql"

# Create migrations directory if it doesn't exist
migrations_dir="${service_name}/migrations"
mkdir -p "$migrations_dir"

# Create migration file
filepath="${migrations_dir}/${filename}"
touch "$filepath"

echo "Created migration file: $filepath"
echo ""
echo "Edit the file to add your migration SQL"