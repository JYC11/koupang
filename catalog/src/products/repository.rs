use crate::brands::value_objects::BrandId;
use crate::categories::value_objects::CategoryId;
use crate::products::dtos::{
    ProductFilter, ValidAddProductImageReq, ValidCreateProductReq, ValidCreateSkuReq,
    ValidUpdateProductReq, ValidUpdateSkuReq,
};
use crate::products::entities::{
    ProductDetailRow, ProductEntity, ProductImageEntity, ProductListEntity, SkuEntity,
};
use crate::products::value_objects::{ProductId, ProductImageId, SkuId};
use shared::db::pagination_support::{PaginationParams, keyset_paginate};
use shared::db::{PgExec, PgPool};
use shared::errors::AppError;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use uuid::Uuid;
// ── Product queries ─────────────────────────────────────────

const PRODUCT_SELECT: &str = "\
    SELECT p.*, \
           c.name AS category_name, c.slug AS category_slug, \
           b.name AS brand_name, b.slug AS brand_slug \
    FROM products p \
    LEFT JOIN categories c ON p.category_id = c.id \
    LEFT JOIN brands b ON p.brand_id = b.id \
    WHERE 1=1";

const PRODUCT_LIST_SELECT: &str = "\
    SELECT p.id, p.created_at, p.seller_id, p.name, p.slug, \
           p.base_price, p.currency, p.category_id, p.brand_id, p.status, \
           c.name AS category_name, b.name AS brand_name, \
           pi.url AS image_url \
    FROM products p \
    LEFT JOIN categories c ON p.category_id = c.id \
    LEFT JOIN brands b ON p.brand_id = b.id \
    LEFT JOIN LATERAL ( \
        SELECT url FROM product_images WHERE product_id = p.id ORDER BY sort_order ASC LIMIT 1 \
    ) pi ON true \
    WHERE 1=1";

pub async fn get_product_by_id<'e>(
    executor: impl PgExec<'e>,
    id: ProductId,
) -> Result<ProductEntity, AppError> {
    let sql = format!("{} AND p.id = $1 AND p.deleted_at IS NULL", PRODUCT_SELECT);
    sqlx::query_as::<_, ProductEntity>(&sql)
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Product not found: {}", e)))
}

pub async fn get_product_by_slug<'e>(
    executor: impl PgExec<'e>,
    slug: &str,
) -> Result<ProductEntity, AppError> {
    let sql = format!(
        "{} AND p.slug = $1 AND p.deleted_at IS NULL",
        PRODUCT_SELECT
    );
    sqlx::query_as::<_, ProductEntity>(&sql)
        .bind(slug)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Product not found: {}", e)))
}

/// Fetch product with SKUs and images in a single query using LATERAL + json_agg.
pub async fn get_product_detail<'e>(
    executor: impl PgExec<'e>,
    id: ProductId,
) -> Result<ProductDetailRow, AppError> {
    sqlx::query_as::<_, ProductDetailRow>(
        "SELECT p.*, \
                c.name AS category_name, c.slug AS category_slug, \
                b.name AS brand_name, b.slug AS brand_slug, \
                COALESCE(s.skus_json, '[]'::json) AS skus_json, \
                COALESCE(i.images_json, '[]'::json) AS images_json \
         FROM products p \
         LEFT JOIN categories c ON p.category_id = c.id \
         LEFT JOIN brands b ON p.brand_id = b.id \
         LEFT JOIN LATERAL ( \
             SELECT json_agg(row_to_json(sr)) AS skus_json \
             FROM (SELECT id, created_at, updated_at, deleted_at, product_id, \
                          sku_code, price, stock_quantity, attributes, status \
                   FROM skus WHERE product_id = p.id AND deleted_at IS NULL \
                   ORDER BY created_at ASC LIMIT 100) sr \
         ) s ON true \
         LEFT JOIN LATERAL ( \
             SELECT json_agg(row_to_json(ir)) AS images_json \
             FROM (SELECT id, created_at, product_id, url, alt_text, sort_order, is_primary \
                   FROM product_images WHERE product_id = p.id \
                   ORDER BY sort_order ASC LIMIT 50) ir \
         ) i ON true \
         WHERE p.id = $1 AND p.deleted_at IS NULL",
    )
    .bind(id.value())
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::NotFound(format!("Product not found: {}", e)))
}

fn apply_product_filters(qb: &mut QueryBuilder<Postgres>, filter: &ProductFilter) {
    if let Some(category_id) = filter.category_id {
        qb.push(" AND p.category_id = ");
        qb.push_bind(category_id);
    }
    if let Some(brand_id) = filter.brand_id {
        qb.push(" AND p.brand_id = ");
        qb.push_bind(brand_id);
    }
    if let Some(min_price) = filter.min_price {
        qb.push(" AND p.base_price >= ");
        qb.push_bind(min_price);
    }
    if let Some(max_price) = filter.max_price {
        qb.push(" AND p.base_price <= ");
        qb.push_bind(max_price);
    }
    if let Some(ref search) = filter.search {
        qb.push(" AND p.name ILIKE '%' || ");
        qb.push_bind(search.as_str().to_string());
        qb.push(" || '%'");
    }
    if let Some(ref status) = filter.status {
        qb.push(" AND p.status = ");
        qb.push_bind(status.as_str().to_string());
    }
}

pub async fn list_products_by_seller<'e>(
    executor: impl PgExec<'e>,
    seller_id: Uuid,
    params: &PaginationParams,
    filter: &ProductFilter,
) -> Result<Vec<ProductListEntity>, AppError> {
    let mut qb = QueryBuilder::new(PRODUCT_LIST_SELECT);
    qb.push(" AND p.seller_id = ");
    qb.push_bind(seller_id);
    qb.push(" AND p.deleted_at IS NULL");
    apply_product_filters(&mut qb, filter);
    keyset_paginate(params, Some("p"), &mut qb);
    qb.build_query_as::<ProductListEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list products: {}", e)))
}

pub async fn list_active_products<'e>(
    executor: impl PgExec<'e>,
    params: &PaginationParams,
    filter: &ProductFilter,
) -> Result<Vec<ProductListEntity>, AppError> {
    let mut qb = QueryBuilder::new(PRODUCT_LIST_SELECT);
    qb.push(" AND p.status = 'active'");
    qb.push(" AND p.deleted_at IS NULL");
    apply_product_filters(&mut qb, filter);
    keyset_paginate(params, Some("p"), &mut qb);
    qb.build_query_as::<ProductListEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list products: {}", e)))
}

// ── FK validation helpers ───────────────────────────────────

pub async fn category_exists<'e>(
    executor: impl PgExec<'e>,
    id: CategoryId,
) -> Result<bool, AppError> {
    let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM categories WHERE id = $1)")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to check category: {}", e)))?;
    Ok(row.0)
}

pub async fn brand_exists<'e>(executor: impl PgExec<'e>, id: BrandId) -> Result<bool, AppError> {
    let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM brands WHERE id = $1)")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to check brand: {}", e)))?;
    Ok(row.0)
}

pub async fn is_brand_in_category<'e>(
    executor: impl PgExec<'e>,
    brand_id: BrandId,
    category_id: CategoryId,
) -> Result<bool, AppError> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM brand_categories WHERE brand_id = $1 AND category_id = $2)",
    )
    .bind(brand_id.value())
    .bind(category_id.value())
    .fetch_one(executor)
    .await
    .map_err(|e| {
        AppError::InternalServerError(format!("Failed to check brand-category association: {}", e))
    })?;
    Ok(row.0)
}

/// Core FK validation: existence checks + brand-category association.
pub async fn validate_fk_references(
    pool: &PgPool,
    category_id: Option<CategoryId>,
    brand_id: Option<BrandId>,
) -> Result<(), AppError> {
    if let Some(cat_id) = category_id {
        if !category_exists(pool, cat_id).await? {
            return Err(AppError::BadRequest("Category does not exist".to_string()));
        }
    }
    if let Some(br_id) = brand_id {
        if !brand_exists(pool, br_id).await? {
            return Err(AppError::BadRequest("Brand does not exist".to_string()));
        }
    }
    if let (Some(cat_id), Some(br_id)) = (category_id, brand_id) {
        if !is_brand_in_category(pool, br_id, cat_id).await? {
            return Err(AppError::BadRequest(
                "Brand is not associated with the specified category".to_string(),
            ));
        }
    }
    Ok(())
}

// ── Product mutations ───────────────────────────────────────

pub async fn create_product(
    tx: &mut PgConnection,
    seller_id: Uuid,
    req: ValidCreateProductReq,
) -> Result<ProductId, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO products (seller_id, name, slug, description, base_price, currency, category_id, brand_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
    )
        .bind(seller_id)
        .bind(req.name.as_str())
        .bind(req.slug.as_str())
        .bind(&req.description)
        .bind(req.base_price.value())
        .bind(req.currency.as_str())
        .bind(req.category_id.map(|id| id.value()))
        .bind(req.brand_id.map(|id| id.value()))
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create product: {}", e)))?;

    Ok(ProductId::new(row.0))
}

pub async fn update_product(
    tx: &mut PgConnection,
    id: ProductId,
    req: ValidUpdateProductReq,
) -> Result<(), AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE products SET ");
    let mut fields = 0usize;
    {
        let mut sep = qb.separated(", ");
        if let Some(ref name) = req.name {
            sep.push("name = ").push_bind_unseparated(name.as_str());
            fields += 1;
        }
        if let Some(ref slug) = req.slug {
            sep.push("slug = ").push_bind_unseparated(slug.as_str());
            fields += 1;
        }
        if let Some(ref description) = req.description {
            sep.push("description = ")
                .push_bind_unseparated(description.clone());
            fields += 1;
        }
        if let Some(ref base_price) = req.base_price {
            sep.push("base_price = ")
                .push_bind_unseparated(base_price.value());
            fields += 1;
        }
        if let Some(ref currency) = req.currency {
            sep.push("currency = ")
                .push_bind_unseparated(currency.as_str());
            fields += 1;
        }
        if let Some(ref category_id) = req.category_id {
            sep.push("category_id = ")
                .push_bind_unseparated(category_id.value());
            fields += 1;
        }
        if let Some(ref brand_id) = req.brand_id {
            sep.push("brand_id = ")
                .push_bind_unseparated(brand_id.value());
            fields += 1;
        }
        if let Some(ref status) = req.status {
            sep.push("status = ")
                .push_bind_unseparated(status.as_str().to_string());
            fields += 1;
        }
        if fields == 0 {
            return Ok(());
        }
        sep.push("updated_at = NOW()");
    }
    qb.push(" WHERE id = ").push_bind(id.value());
    qb.push(" AND deleted_at IS NULL");

    let result =
        qb.build().execute(&mut *tx).await.map_err(|e| {
            AppError::InternalServerError(format!("Failed to update product: {}", e))
        })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Product not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );

    Ok(())
}

pub async fn delete_product(tx: &mut PgConnection, id: ProductId) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE products SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id.value())
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to delete product: {}", e))
            })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Product not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "DELETE must affect exactly 1 row"
    );

    Ok(())
}

// ── SKU queries ─────────────────────────────────────────────

pub async fn get_sku_by_id<'e>(
    executor: impl PgExec<'e>,
    id: SkuId,
) -> Result<SkuEntity, AppError> {
    sqlx::query_as::<_, SkuEntity>("SELECT * FROM skus WHERE id = $1 AND deleted_at IS NULL")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("SKU not found: {}", e)))
}

pub async fn list_skus_by_product<'e>(
    executor: impl PgExec<'e>,
    product_id: ProductId,
) -> Result<Vec<SkuEntity>, AppError> {
    sqlx::query_as::<_, SkuEntity>(
        "SELECT * FROM skus WHERE product_id = $1 AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 100",
    )
    .bind(product_id.value())
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list SKUs: {}", e)))
}

pub async fn create_sku(
    tx: &mut PgConnection,
    product_id: ProductId,
    req: ValidCreateSkuReq,
) -> Result<SkuId, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO skus (product_id, sku_code, price, stock_quantity, attributes)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
    )
    .bind(product_id.value())
    .bind(req.sku_code.as_str())
    .bind(req.price.value())
    .bind(req.stock_quantity.value())
    .bind(&req.attributes)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create SKU: {}", e)))?;

    Ok(SkuId::new(row.0))
}

pub async fn update_sku(
    tx: &mut PgConnection,
    id: SkuId,
    req: ValidUpdateSkuReq,
) -> Result<(), AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE skus SET ");
    let mut fields = 0usize;
    {
        let mut sep = qb.separated(", ");
        if let Some(ref price) = req.price {
            sep.push("price = ").push_bind_unseparated(price.value());
            fields += 1;
        }
        if let Some(ref stock_quantity) = req.stock_quantity {
            sep.push("stock_quantity = ")
                .push_bind_unseparated(stock_quantity.value());
            fields += 1;
        }
        if let Some(ref attributes) = req.attributes {
            sep.push("attributes = ")
                .push_bind_unseparated(attributes.clone());
            fields += 1;
        }
        if let Some(ref status) = req.status {
            sep.push("status = ")
                .push_bind_unseparated(status.as_str().to_string());
            fields += 1;
        }
        if fields == 0 {
            return Ok(());
        }
        sep.push("updated_at = NOW()");
    }
    qb.push(" WHERE id = ").push_bind(id.value());
    qb.push(" AND deleted_at IS NULL");

    let result = qb
        .build()
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update SKU: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("SKU not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );

    Ok(())
}

pub async fn delete_sku(tx: &mut PgConnection, id: SkuId) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE skus SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id.value())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to delete SKU: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("SKU not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "DELETE must affect exactly 1 row"
    );

    Ok(())
}

// ── Stock operations ────────────────────────────────────────

pub async fn adjust_stock(
    tx: &mut PgConnection,
    sku_id: SkuId,
    delta: i32,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE skus SET stock_quantity = stock_quantity + $1, updated_at = NOW()
         WHERE id = $2 AND deleted_at IS NULL AND stock_quantity + $1 >= 0",
    )
    .bind(delta)
    .bind(sku_id.value())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to adjust stock: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "SKU not found or insufficient stock".to_string(),
        ));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );

    Ok(())
}

// ── Image queries ───────────────────────────────────────────

pub async fn get_image_by_id<'e>(
    executor: impl PgExec<'e>,
    id: ProductImageId,
) -> Result<ProductImageEntity, AppError> {
    sqlx::query_as::<_, ProductImageEntity>("SELECT * FROM product_images WHERE id = $1")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Image not found: {}", e)))
}

pub async fn list_images_by_product<'e>(
    executor: impl PgExec<'e>,
    product_id: ProductId,
) -> Result<Vec<ProductImageEntity>, AppError> {
    sqlx::query_as::<_, ProductImageEntity>(
        "SELECT * FROM product_images WHERE product_id = $1 ORDER BY sort_order ASC LIMIT 50",
    )
    .bind(product_id.value())
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list images: {}", e)))
}

pub async fn add_product_image(
    tx: &mut PgConnection,
    product_id: ProductId,
    req: ValidAddProductImageReq,
) -> Result<ProductImageId, AppError> {
    // If this image is primary, unset any existing primary
    if req.is_primary {
        sqlx::query("UPDATE product_images SET is_primary = FALSE WHERE product_id = $1 AND is_primary = TRUE")
            .bind(product_id.value())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to unset primary image: {}", e)))?;
    }

    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO product_images (product_id, url, alt_text, sort_order, is_primary)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
    )
    .bind(product_id.value())
    .bind(req.url.as_str())
    .bind(&req.alt_text)
    .bind(req.sort_order)
    .bind(req.is_primary)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to add image: {}", e)))?;

    Ok(ProductImageId::new(row.0))
}

pub async fn delete_product_image(
    tx: &mut PgConnection,
    id: ProductImageId,
) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM product_images WHERE id = $1")
        .bind(id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete image: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Image not found".to_string()));
    }

    Ok(())
}
