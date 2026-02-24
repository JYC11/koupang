-- Products table
CREATE TABLE products (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ,
    seller_id       UUID NOT NULL,
    name            VARCHAR(500) NOT NULL,
    slug            VARCHAR(500) NOT NULL UNIQUE,
    description     TEXT,
    base_price      NUMERIC(19, 4) NOT NULL,
    currency        VARCHAR(3) NOT NULL DEFAULT 'USD',
    category        VARCHAR(255),
    brand           VARCHAR(255),
    status          VARCHAR(50) NOT NULL DEFAULT 'draft',
    CONSTRAINT chk_products_base_price CHECK (base_price >= 0),
    CONSTRAINT chk_products_status CHECK (status IN ('draft', 'active', 'inactive', 'archived'))
);

CREATE INDEX idx_products_seller_id ON products(seller_id);
CREATE INDEX idx_products_status ON products(status) WHERE deleted_at IS NULL;
CREATE INDEX idx_products_category ON products(category) WHERE deleted_at IS NULL;
CREATE INDEX idx_products_slug ON products(slug);

-- SKUs table
CREATE TABLE skus (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ,
    product_id      UUID NOT NULL REFERENCES products(id),
    sku_code        VARCHAR(100) NOT NULL UNIQUE,
    price           NUMERIC(19, 4) NOT NULL,
    stock_quantity  INTEGER NOT NULL DEFAULT 0,
    attributes      JSONB NOT NULL DEFAULT '{}',
    status          VARCHAR(50) NOT NULL DEFAULT 'active',
    CONSTRAINT chk_skus_price CHECK (price >= 0),
    CONSTRAINT chk_skus_stock CHECK (stock_quantity >= 0),
    CONSTRAINT chk_skus_status CHECK (status IN ('active', 'inactive', 'out_of_stock'))
);

CREATE INDEX idx_skus_product_id ON skus(product_id);
CREATE INDEX idx_skus_sku_code ON skus(sku_code);
CREATE INDEX idx_skus_status ON skus(status) WHERE deleted_at IS NULL;

-- Product images table (no soft delete)
CREATE TABLE product_images (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    product_id      UUID NOT NULL REFERENCES products(id),
    url             TEXT NOT NULL,
    alt_text        VARCHAR(500),
    sort_order      INTEGER NOT NULL DEFAULT 0,
    is_primary      BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_product_images_product_id ON product_images(product_id);
