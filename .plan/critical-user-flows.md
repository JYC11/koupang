Here are the detailed architectural flows for your E-commerce platform. I have designed these to leverage your Rust stack (`axum`, `sqlx`, `tokio`) and the specific patterns we discussed (Outbox, EDA, Resilience).

Each flow highlights **Synchronous** (HTTP/gRPC) vs. **Asynchronous** (Events) boundaries.

---

### 1. Ordering Flow (The Core Saga)

**Goal:** Create an order, reserve stock, and capture payment without distributed transactions.
**Pattern:** Choreography Saga + Transactional Outbox.

1.  **Client** sends `POST /orders` to **Order Service** (includes `Idempotency-Key`).
2.  **Order Service** (Sync):
    - Validates User Session (Identity Service).
    - Checks Cart validity.
    - **DB Transaction:** Inserts `Order` (Status: `Pending`) + Inserts `Outbox` Event (`OrderPlaced`).
    - Returns `202 Accepted` to Client.
3.  **Outbox Relay** (Async):
    - Polls DB, finds `OrderPlaced`, publishes to Kafka topic `orders.events`.
4.  **Inventory Service** (Async Consumer):
    - Consumes `OrderPlaced`.
    - Checks stock. If available: **DB Transaction:** Decrement Stock + Insert `Outbox` Event (`InventoryReserved`). If unavailable: Publish `InventoryReservationFailed`.
5.  **Payment Service** (Async Consumer):
    - Consumes `OrderPlaced`.
    - Calls External Payment Gateway (Stripe/PayPal).
    - **Resilience:** If Gateway times out, Retry with Backoff.
    - On Success: **DB Transaction:** Record Transaction + Insert `Outbox` Event (`PaymentAuthorized`).
6.  **Order Service** (Listening to Yourself):
    - Consumes `InventoryReserved` AND `PaymentAuthorized`.
    - **DB Transaction:** Update Order Status to `Confirmed` + Insert `Outbox` Event (`OrderConfirmed`).
    - _Compensation:_ If `InventoryReservationFailed` or `PaymentFailed` arrives first, update Order to `Cancelled` and publish `InventoryReleaseCommand`.

---

### 2. Shipping Flow

**Goal:** Generate labels and track logistics after payment.
**Pattern:** Event-Driven + External API Integration.

1.  **Trigger:** `OrderConfirmed` event (from Ordering Flow).
2.  **Shipping Service** (Async Consumer):
    - Consumes `OrderConfirmed`.
    - Selects best carrier based on weight/address rules.
    - Calls Carrier API (FedEx/UPS) to generate Label.
    - **Resilience:** Circuit Breaker on Carrier API. If down, queue for later retry.
    - **DB Transaction:** Save `Shipment` record + Insert `Outbox` Event (`ShipmentCreated`).
3.  **Notification Service** (Async Consumer):
    - Consumes `ShipmentCreated`.
    - Sends Email/SMS to Buyer with Tracking Link.
4.  **Order Service** (Listening to Yourself):
    - Consumes `ShipmentCreated`.
    - Updates Order Status to `Shipped`.

---

### 3. Uploading Products/Inventory (Seller Flow)

**Goal:** Handle bulk data ingestion without blocking the API.
**Pattern:** Async Processing + Blob Storage.

1.  **Seller** uploads CSV/JSON via **Catalog Service** (`POST /products/bulk`).
2.  **Catalog Service** (Sync):
    - Validates file type/size.
    - Uploads raw file to S3/Blob Storage.
    - **DB Transaction:** Create `ImportJob` (Status: `Processing`) + Insert `Outbox` Event (`ProductImportRequested`).
    - Returns `JobID` to Seller immediately.
3.  **Import Worker** (Separate Rust Binary or Background Task):
    - Consumes `ProductImportRequested`.
    - Downloads file from S3.
    - Parses rows (Stream processing to save memory).
    - **Batching:** Inserts products in batches of 1000.
    - **DB Transaction:** Update `ImportJob` (Status: `Completed`) + Insert `Outbox` Event (`ProductCreated`) for each valid item.
4.  **Search Service** (Async Consumer):
    - Consumes `ProductCreated`.
    - Indexes document into Elasticsearch/Meilisearch.
5.  **Seller UI** (Polling):
    - Polls `GET /import-jobs/{JobID}` to check status.

---

### 4. Resolving Payments (Webhook Handling)

**Goal:** Reconcile asynchronous payment gateway results.
**Pattern:** Idempotency + Signature Verification.

1.  **Payment Gateway** sends Webhook (`POST /webhooks/payment`) to **Payment Service**.
2.  **Payment Service** (Sync):
    - **Security:** Verifies HMAC Signature (ensure request is from Gateway).
    - **Idempotency:** Checks `EventID` from Gateway against DB `ProcessedWebhooks` table. If exists, return `200 OK` immediately.
    - **DB Transaction:** Record Webhook Event + Insert `Outbox` Event (`PaymentCaptured` or `PaymentFailed`).
    - Return `200 OK`.
3.  **Order Service** (Async Consumer):
    - Consumes `PaymentCaptured`.
    - Updates Order Status (if not already updated by direct flow).
    - _Note:_ This handles cases where the user closed the browser before the redirect returned.

---

### 5. Signing Up (Identity & Access)

**Goal:** Register users with different roles (Buyer, Seller, Admin).
**Pattern:** CQRS + Event Notification.

1.  **Client** sends `POST /auth/register` to **Identity Service**.
2.  **Identity Service** (Sync):
    - Validates Email uniqueness.
    - Hashes Password (Argon2).
    - **DB Transaction:** Insert `User` + Insert `Outbox` Event (`UserRegistered`).
    - Returns JWT + Refresh Token.
3.  **Notification Service** (Async Consumer):
    - Consumes `UserRegistered`.
    - Sends "Welcome" Email.
4.  **Seller Onboarding (Special Case):**
    - If Role = `Seller`, Identity Service also publishes `SellerApplicationSubmitted`.
    - **Admin Service** consumes this, presents it in Admin Dashboard.
    - Admin approves -> Publishes `SellerApproved`.
    - **Catalog Service** consumes `SellerApproved` -> Enables product upload permissions for that User ID.

---

### 6. Browsing, Cart, & Wishlist

**Goal:** High performance reads and ephemeral writes.
**Pattern:** Caching + Redis.

1.  **Browsing (Read Heavy):**
    - **Client** requests `GET /products`.
    - **Catalog Service** checks **Redis Cache**.
    - If Hit: Return JSON.
    - If Miss: Query **PostgreSQL Read Replica** -> Populate Redis -> Return JSON.
    - _Pattern:_ Cache-Aside.
2.  **Cart (High Write/Transient):**
    - **Client** sends `POST /cart/items`.
    - **Cart Service** (can be part of Order or separate):
      - Stores data in **Redis Hash** (Key: `cart:{user_id}`).
      - Sets TTL (e.g., 30 days).
      - _Note:_ Do not write to SQL yet. SQL is for persistent Orders only.
    - **Checkout:** When user clicks Checkout, Cart Service validates stock (Sync API to Inventory) -> Moves data to Order Service -> Clears Redis Cart.
3.  **Wishlist (Persistent):**
    - **Client** sends `POST /wishlist`.
    - **Catalog Service**:
      - **DB Transaction:** Insert `WishlistItem` + Insert `Outbox` Event (`WishlistUpdated`).
      - _Why Event?_ To trigger "Price Drop Alerts" later.
    - **Notification Service** consumes `WishlistUpdated` + `ProductPriceChanged` -> Emails user if price drops.

---

### 7. Additional Critical Flows

#### A. Returns & Refunds (Reverse Logistics)

1.  **Buyer** requests Return via **Order Service**.
2.  **Order Service** publishes `ReturnRequested`.
3.  **Shipping Service** consumes -> Generates Return Label.
4.  **Warehouse** scans returned item -> **Inventory Service** publishes `ItemReturned`.
5.  **Payment Service** consumes `ItemReturned` -> Issues Refund via Gateway -> Publishes `RefundProcessed`.
6.  **Order Service** listens -> Updates Order Status to `Returned`.

#### B. Reviews & Moderation

1.  **Buyer** submits Review -> **Review Service**.
2.  **Review Service** saves as `Pending` -> Publishes `ReviewSubmitted`.
3.  **Moderation Service** (AI or Human) consumes -> Approves/Rejects.
4.  If Approved: Publishes `ReviewPublished`.
5.  **Catalog Service** consumes -> Updates Product Average Rating (Denormalization for fast read).

---

### 8. Auth Flows
- User Roles so far:
    - BUYER: end users
    - SELLER: users who can upload and sell products
    - ADMIN: users who can manage the entire platform
  - Could be up to change, so it's not enums right now
- User Registration
  - sends verification email after registration
  - needs to verify email before login
- User Password Reset
  - sends reset link to email
- User Password Change
  - requires current and new password

# Architecture Diagram Logic (Mental Model)

To visualize how these fit together in your Rust project:

1.  **The "Write" Path:**
    - Axum Handler -> SQLx Transaction (Business Data + Outbox) -> Return 202.
    - Background Task (Outbox) -> Kafka.
2.  **The "Read" Path:**
    - Axum Handler -> Redis (Cache) -> SQLx Read Replica.
3.  **The "Reaction" Path:**
    - Kafka Consumer -> Business Logic -> SQLx Transaction (State Update + New Outbox) -> Kafka.

# Implementation Checklist for These Flows

| Feature            | Priority | Rust Crate/Tool                 | Key Pattern          |
| :----------------- | :------- | :------------------------------ | :------------------- |
| **Auth/JWT**       | High     | `axum-extra`, `jsonwebtoken`    | Middleware           |
| **Outbox Polling** | High     | `tokio::time::interval`, `sqlx` | Transactional Outbox |
| **Kafka Producer** | High     | `rdkafka`                       | EDA                  |
| **Kafka Consumer** | High     | `rdkafka` + `tokio::spawn`      | Listen to Yourself   |
| **Redis Client**   | High     | `redis` (async)                 | Caching/Cart         |
| **Idempotency**    | High     | Custom Axum Middleware          | Resilience           |
| **Webhook Sig**    | Med      | `hmac`, `sha2`                  | Security             |
| **OpenAPI**        | Med      | `utoipa`, `utoipa-swagger-ui`   | API Evolution        |
| **Tracing**        | Med      | `tracing-opentelemetry`         | Observability        |
