#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE DATABASE identity;
    CREATE DATABASE catalog;
    CREATE DATABASE "order";
    CREATE DATABASE payment;
    CREATE DATABASE shipping;
    CREATE DATABASE notification;
    CREATE DATABASE review;
    CREATE DATABASE moderation;
EOSQL
