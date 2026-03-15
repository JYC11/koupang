use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::value_objects::{Currency, PriceSnapshot, Quantity};

/// A single item in a user's shopping cart.
#[derive(Debug, Clone)]
pub struct CartItem {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: Quantity,
    pub unit_price: PriceSnapshot,
    pub currency: Currency,
    pub product_name: String,
    pub image_url: Option<String>,
    pub added_at: DateTime<Utc>,
}

impl CartItem {
    /// Display-only line total: quantity * unit_price.
    pub fn line_total(&self) -> Decimal {
        self.unit_price.value() * Decimal::from(self.quantity.value())
    }
}

/// A user's shopping cart.
#[derive(Debug, Clone)]
pub struct Cart {
    pub user_id: Uuid,
    pub items: Vec<CartItem>,
}

impl Cart {
    pub fn new(user_id: Uuid, items: Vec<CartItem>) -> Self {
        Self { user_id, items }
    }

    /// Display-only total: sum of all line totals.
    pub fn total(&self) -> Decimal {
        self.items.iter().map(|i| i.line_total()).sum()
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(qty: u32, price: i64) -> CartItem {
        CartItem {
            product_id: Uuid::now_v7(),
            sku_id: Uuid::now_v7(),
            quantity: Quantity::new(qty).unwrap(),
            unit_price: PriceSnapshot::new(Decimal::new(price, 2)).unwrap(),
            currency: Currency::default(),
            product_name: "Test Product".to_string(),
            image_url: None,
            added_at: Utc::now(),
        }
    }

    #[test]
    fn line_total_computes_correctly() {
        let item = make_item(3, 1999); // 3 * 19.99 = 59.97
        assert_eq!(item.line_total(), Decimal::new(5997, 2));
    }

    #[test]
    fn cart_total_sums_line_totals() {
        let cart = Cart::new(Uuid::now_v7(), vec![make_item(2, 1000), make_item(1, 500)]);
        // 2*10.00 + 1*5.00 = 25.00
        assert_eq!(cart.total(), Decimal::new(2500, 2));
    }

    #[test]
    fn empty_cart_total_is_zero() {
        let cart = Cart::new(Uuid::now_v7(), vec![]);
        assert_eq!(cart.total(), Decimal::ZERO);
        assert_eq!(cart.item_count(), 0);
    }

    #[test]
    fn item_count_matches_items() {
        let cart = Cart::new(Uuid::now_v7(), vec![make_item(1, 100), make_item(1, 200)]);
        assert_eq!(cart.item_count(), 2);
    }
}
