-- Enable ltree extension for hierarchical category paths
CREATE EXTENSION IF NOT EXISTS ltree;

-- Categories table (ltree-based hierarchy)
CREATE TABLE categories (
    id          UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ,
    name        VARCHAR(255) NOT NULL,
    slug        VARCHAR(255) NOT NULL UNIQUE,
    path        ltree NOT NULL UNIQUE,
    parent_id   UUID REFERENCES categories(id),
    depth       INTEGER NOT NULL DEFAULT 0,
    description TEXT
);

CREATE INDEX idx_categories_path_gist ON categories USING GIST (path);
CREATE INDEX idx_categories_parent_id ON categories(parent_id);

-- Brands table
CREATE TABLE brands (
    id          UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ,
    name        VARCHAR(255) NOT NULL UNIQUE,
    slug        VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    logo_url    TEXT
);

-- Brand-Category associations (many-to-many)
CREATE TABLE brand_categories (
    brand_id    UUID NOT NULL REFERENCES brands(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (brand_id, category_id)
);

CREATE INDEX idx_brand_categories_category_id ON brand_categories(category_id);

-- Alter products: replace string columns with UUID foreign keys
ALTER TABLE products DROP COLUMN category;
ALTER TABLE products DROP COLUMN brand;

ALTER TABLE products ADD COLUMN category_id UUID REFERENCES categories(id);
ALTER TABLE products ADD COLUMN brand_id UUID REFERENCES brands(id);

CREATE INDEX idx_products_category_id ON products(category_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_products_brand_id ON products(brand_id) WHERE deleted_at IS NULL;
