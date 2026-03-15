# Hyperswitch Architecture Research

Research on [juspay/hyperswitch](https://github.com/juspay/hyperswitch) — a production Rust payment orchestration platform.
Extracted patterns relevant to building our payment service.

## 1. Crate Structure

Hyperswitch splits into ~35 crates in a Cargo workspace. Key ones:

| Crate | Responsibility |
|-------|---------------|
| `common_enums` | All shared enums (statuses, payment methods, currencies) |
| `common_utils` | Utilities, `MinorUnit` type, ID types, crypto |
| `hyperswitch_domain_models` | Domain types: `PaymentIntent`, `PaymentAttempt`, `RouterData`, `ErrorResponse` |
| `hyperswitch_interfaces` | Trait definitions: `Connector`, `ConnectorIntegration`, `ConnectorCommon` |
| `hyperswitch_connectors` | 70+ concrete connector adapters (Stripe, Adyen, etc.) |
| `router` | Main application: routes, core business logic, operations |
| `diesel_models` | Database models (Diesel ORM) |
| `storage_impl` | Storage layer implementations |

**Takeaway for us**: Our simpler setup (single service) doesn't need this many crates, but separating domain models from connector interfaces from concrete adapters is sound.

## 2. Payment State Machine

### Two-level status model

Hyperswitch separates **intent-level** status from **attempt-level** status:

**`IntentStatus`** (the overall payment):
```rust
pub enum IntentStatus {
    Succeeded,
    Failed,
    Cancelled,
    CancelledPostCapture,
    Processing,
    RequiresCustomerAction,
    RequiresMerchantAction,
    RequiresPaymentMethod,
    RequiresConfirmation,  // default
    RequiresCapture,
    PartiallyCaptured,
    PartiallyCapturedAndCapturable,
    PartiallyAuthorizedAndRequiresCapture,
    PartiallyCapturedAndProcessing,
    Conflicted,
    Expired,
}
```

**`AttemptStatus`** (each payment attempt):
```rust
pub enum AttemptStatus {
    Started,
    AuthenticationFailed,
    RouterDeclined,
    AuthenticationPending,
    AuthenticationSuccessful,
    Authorized,
    AuthorizationFailed,
    Charged,
    Authorizing,
    CodInitiated,
    Voided,
    VoidedPostCharge,
    VoidInitiated,
    CaptureInitiated,
    CaptureFailed,
    VoidFailed,
    AutoRefunded,
    PartialCharged,
    PartiallyAuthorized,
    PartialChargedAndChargeable,
    Unresolved,
    Pending,  // default
    Failure,
    PaymentMethodAwaited,
    ConfirmationAwaited,
    DeviceDataCollectionPending,
    IntegrityFailure,
    Expired,
}
```

### Status guard methods

Every enum has exhaustive `match` methods that classify states:

```rust
impl AttemptStatus {
    pub fn is_terminal_status(self) -> bool { /* exhaustive match */ }
    pub fn is_payment_terminal_failure(self) -> bool { /* exhaustive match */ }
    pub fn is_success(self) -> bool { matches!(self, Self::Charged | Self::PartialCharged) }
    pub fn should_update_payment_method(self) -> bool { /* exhaustive match */ }
}

impl IntentStatus {
    pub fn is_in_terminal_state(self) -> bool { /* exhaustive match */ }
    pub fn should_force_sync_with_connector(self) -> bool { /* exhaustive match */ }
}
```

Key pattern: **exhaustive matches over all variants** — no `_ =>` wildcard. This forces compile-time review when a new status is added.

### ValidateStatusForOperation trait

Each operation validates whether it can proceed on the current intent status:

```rust
pub trait ValidateStatusForOperation {
    fn validate_status_for_operation(
        &self,
        intent_status: common_enums::IntentStatus,
    ) -> Result<(), errors::ApiErrorResponse>;
}

// Example: PaymentIntentConfirm can only run on RequiresPaymentMethod
impl ValidateStatusForOperation for PaymentIntentConfirm {
    fn validate_status_for_operation(&self, intent_status: IntentStatus) -> Result<(), ApiErrorResponse> {
        match intent_status {
            IntentStatus::RequiresPaymentMethod => Ok(()),
            _ => Err(ApiErrorResponse::PaymentUnexpectedState { ... })
        }
    }
}
```

**Takeaway for us**: This is the cleanest pattern for our order state machine. Each operation (place, cancel, ship, deliver) validates preconditions via a trait impl with exhaustive status matching.

## 3. Connector/Gateway Abstraction

### Three-layer abstraction

1. **`ConnectorCommon`** trait — identity, base URL, auth header, content type, error parsing
2. **`ConnectorIntegration<Flow, Req, Resp>`** trait — per-flow (Authorize, Capture, Sync, Void) with methods:
   - `get_url()` → connector endpoint
   - `get_headers()` → auth + custom headers
   - `get_request_body()` → serialize request
   - `build_request()` → assemble HTTP request
   - `handle_response()` → deserialize + map to `RouterData`
   - `get_error_response()` → parse connector error
3. **`Connector`** supertrait — combines Payment + Refund + Dispute + Webhook + etc.

### RouterData: The universal data carrier

```rust
pub struct RouterData<Flow, Request, Response> {
    pub flow: PhantomData<Flow>,
    pub merchant_id: MerchantId,
    pub connector: String,
    pub payment_id: String,
    pub attempt_id: String,
    pub status: AttemptStatus,
    pub payment_method: PaymentMethod,
    pub connector_auth_type: ConnectorAuthType,
    pub amount_captured: Option<i64>,
    pub request: Request,                                    // flow-specific input
    pub response: Result<Response, ErrorResponse>,           // flow-specific output
    // ... 30+ more fields
}
```

The `Request` and `Response` type parameters are flow-specific:
- `Authorize` flow: `Request = PaymentsAuthorizeData`, `Response = PaymentsResponseData`
- `Capture` flow: `Request = PaymentsCaptureData`, `Response = PaymentsResponseData`

### Concrete connector pattern (Stripe example)

```rust
pub struct Stripe {
    amount_converter: &'static (dyn AmountConvertor<Output = MinorUnit> + Sync),
}

// Mark which flows are supported (empty trait impls)
impl api::Payment for Stripe {}
impl api::PaymentAuthorize for Stripe {}
impl api::PaymentSync for Stripe {}
impl api::PaymentCapture for Stripe {}
impl api::PaymentVoid for Stripe {}

// Implement per-flow integration
impl ConnectorIntegration<Authorize, PaymentsAuthorizeData, PaymentsResponseData> for Stripe {
    fn get_url(&self, _req: &PaymentsAuthorizeRouterData, connectors: &Connectors) -> CustomResult<String, ConnectorError> {
        Ok(format!("{}{}", self.base_url(connectors), "v1/payment_intents"))
    }

    fn get_request_body(&self, req: &PaymentsAuthorizeRouterData, _connectors: &Connectors) -> CustomResult<RequestContent, ConnectorError> {
        let amount = utils::convert_amount(self.amount_converter, req.request.minor_amount, req.request.currency)?;
        let connector_req = stripe::PaymentIntentRequest::try_from((req, amount))?;
        Ok(RequestContent::FormUrlEncoded(Box::new(connector_req)))
    }

    fn handle_response(&self, data: &PaymentsAuthorizeRouterData, event_builder: Option<&mut ConnectorEvent>, res: Response)
        -> CustomResult<PaymentsAuthorizeRouterData, ConnectorError>
    {
        let response: stripe::PaymentIntentResponse = res.response.parse_struct("PaymentIntentResponse")?;
        RouterData::try_from(ResponseRouterData { response, data: data.clone(), http_code: res.status_code })
    }
}
```

### Status mapping per connector

Each connector defines its own status enum and maps it to the universal `AttemptStatus`:

```rust
// In stripe/transformers.rs
pub enum StripePaymentStatus {
    Succeeded, Failed, Processing, RequiresCustomerAction,
    RequiresPaymentMethod, RequiresConfirmation, Canceled,
    RequiresCapture, Chargeable, Consumed, Pending,
}

impl From<StripePaymentStatus> for AttemptStatus {
    fn from(item: StripePaymentStatus) -> Self {
        match item {
            StripePaymentStatus::Succeeded => Self::Charged,
            StripePaymentStatus::Failed => Self::Failure,
            StripePaymentStatus::RequiresCapture => Self::Authorized,
            StripePaymentStatus::Canceled => Self::Voided,
            // ...
        }
    }
}
```

### ConnectorAuthType

Generic auth representation that every connector maps from:

```rust
pub enum ConnectorAuthType {
    TemporaryAuth,
    HeaderKey { api_key: Secret<String> },
    BodyKey { api_key: Secret<String>, key1: Secret<String> },
    SignatureKey { api_key: Secret<String>, key1: Secret<String>, api_secret: Secret<String> },
    MultiAuthKey { api_key: Secret<String>, key1: Secret<String>, api_secret: Secret<String>, key2: Secret<String> },
    CurrencyAuthKey { auth_key_map: HashMap<Currency, SecretSerdeValue> },
    CertificateAuth { certificate: Secret<String>, private_key: Secret<String> },
    NoKey,
}
```

**Takeaway for us**: This three-layer approach is directly usable. For our initial payment service with 1-2 gateways, we can define:
- A `PaymentGateway` trait with `authorize()`, `capture()`, `void()`, `sync()` methods
- Per-gateway adapter structs
- Per-gateway status mapping via `From` impls

## 4. Idempotency Approach

Hyperswitch uses **payment_id as the idempotency key**. At the DB insert level:

```rust
// payment_create.rs — attempt to insert, catch duplicate
let payment_intent = insert_payment_intent(state.store.as_ref(), platform, payment_intent_new)
    .await
    .to_duplicate_response(errors::ApiErrorResponse::DuplicatePayment {
        payment_id: payment_id.clone(),
    })?;

let payment_attempt = insert_payment_attempt(state.store.as_ref(), platform, payment_attempt_new)
    .await
    .to_duplicate_response(errors::ApiErrorResponse::DuplicatePayment {
        payment_id: payment_id.clone(),
    })?;
```

The pattern is:
1. Client provides `payment_id` (or system generates one)
2. Insert with unique constraint on `payment_id`
3. On conflict → return `DuplicatePayment` error (HTTP 409)
4. Some connectors pass their own `idempotency_key` header to the gateway (e.g., Helcim, Amazon Pay, Nexixpay)

For v2, they note: "the merchant reference ID will serve as the idempotency key" via a lookup table.

**Takeaway for us**: Use the order/payment ID as the natural idempotency key with DB unique constraints. Return a `DuplicatePayment`/`AlreadyExists` error on conflict. For downstream gateway calls, pass our payment ID as the gateway's idempotency key.

## 5. Error Handling Patterns

### Layered error types

1. **`ConnectorError`** — errors from connector adapters (transport, parsing, auth, missing fields)
2. **`ApiErrorResponse`** — HTTP-facing errors with structured codes
3. **`error_stack`** — context-rich error chains with `ResultExt::change_context()`

### ConnectorError (connector adapter layer)

```rust
pub enum ConnectorError {
    FailedToObtainIntegrationUrl,
    RequestEncodingFailed,
    ResponseDeserializationFailed,
    MissingRequiredField { field_name: &'static str },
    NotImplemented(String),
    NotSupported { message: String, connector: &'static str },
    FlowNotSupported { flow: String, connector: String },
    // ... ~40 variants total
}
```

### ErrorResponse (connector response errors)

Structured response from failed gateway calls:

```rust
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub reason: Option<String>,
    pub status_code: u16,
    pub attempt_status: Option<AttemptStatus>,
    pub connector_transaction_id: Option<String>,
    pub network_decline_code: Option<String>,
    pub network_advice_code: Option<String>,
    pub network_error_message: Option<String>,
}
```

### ApiErrorResponse (HTTP response errors)

Structured error codes with categories:

```rust
pub enum ErrorType {
    InvalidRequestError,  // IR_xx
    ObjectNotFound,       // HE_02
    RouterError,          // HE_xx
    ProcessingError,      // CE_xx (connector errors)
    BadGateway,
    DuplicateRequest,     // HE_01
    ValidationError,      // HE_03
    ConnectorError,       // CE_00
    LockTimeout,
}

pub enum ApiErrorResponse {
    #[error(error_type = ErrorType::ConnectorError, code = "CE_00", message = "{code}: {message}")]
    ExternalConnectorError { code: String, message: String, connector: String, status_code: u16, reason: Option<String> },

    #[error(error_type = ErrorType::DuplicateRequest, code = "HE_01", message = "...")]
    DuplicatePayment { payment_id: PaymentId },

    #[error(error_type = ErrorType::InvalidRequestError, code = "IR_14", message = "This Payment could not be {current_flow} because it has a {field_name} of {current_value}. The expected state is {states}")]
    PaymentUnexpectedState { current_flow: String, field_name: String, current_value: String, states: String },

    // ... 80+ variants
}
```

### ErrorSwitch trait (cross-layer error mapping)

```rust
impl ErrorSwitch<ApiErrorResponse> for ConnectorError {
    fn switch(&self) -> ApiErrorResponse {
        match self {
            Self::WebhookSourceVerificationFailed => ApiErrorResponse::WebhookAuthenticationFailed,
            _ => ApiErrorResponse::InternalServerError,
        }
    }
}
```

### error_stack usage

```rust
// Chain context through layers
let payment_intent = insert_payment_intent(...)
    .await
    .to_duplicate_response(errors::ApiErrorResponse::DuplicatePayment { ... })?;

let connector_req = stripe::PaymentIntentRequest::try_from((req, amount))
    .change_context(ConnectorError::RequestEncodingFailed)?;

// Attach printable context
operation.to_domain()?
    .get_customer_details(...)
    .await
    .to_not_found_response(errors::ApiErrorResponse::CustomerNotFound)
    .attach_printable("Failed while fetching/creating customer")?;
```

**Takeaway for us**: Our existing `AppError` + per-service error enums pattern is aligned. Key additions to consider:
- Structured error codes (e.g., `PM_01` for payment errors)
- `PaymentUnexpectedState` variant for state machine violations
- `ErrorResponse` struct for gateway error details
- The `error_stack` crate for richer error chains (we currently use `thiserror`)

## 6. Domain Model Structure

### PaymentIntent (v1 — the one closer to our needs)

```rust
pub struct PaymentIntent {
    pub payment_id: PaymentId,
    pub merchant_id: MerchantId,
    pub status: IntentStatus,
    pub amount: MinorUnit,                     // always minor units (cents)
    pub shipping_cost: Option<MinorUnit>,
    pub currency: Option<Currency>,
    pub amount_captured: Option<MinorUnit>,
    pub customer_id: Option<CustomerId>,
    pub description: Option<String>,
    pub return_url: Option<String>,
    pub metadata: Option<Value>,
    pub setup_future_usage: Option<FutureUsage>,
    pub client_secret: Option<String>,
    pub active_attempt: RemoteStorageObject<PaymentAttempt>,
    pub attempt_count: i16,
    pub profile_id: Option<ProfileId>,
    pub merchant_decision: Option<String>,
    pub updated_by: String,
    pub session_expiry: Option<PrimitiveDateTime>,
    pub split_payments: Option<SplitPaymentsRequest>,
    pub created_at: PrimitiveDateTime,
    pub modified_at: PrimitiveDateTime,
    // ... ~40 fields total
}
```

### PaymentAttempt

Separate entity for each payment attempt (a single PaymentIntent can have multiple attempts):

```rust
pub trait PaymentAttemptInterface {
    async fn insert_payment_attempt(...) -> Result<PaymentAttempt, Error>;
    async fn update_payment_attempt(...) -> Result<PaymentAttempt, Error>;
    async fn find_payment_attempt_by_connector_transaction_id_payment_id_merchant_id(...);
    async fn find_payment_attempt_last_successful_attempt_by_payment_id_merchant_id(...);
    // ...
}
```

### MinorUnit (money handling)

All amounts in the codebase use `MinorUnit` (cents/paise), never major units:

```rust
pub struct MinorUnit(i64);  // newtype wrapper
```

Each connector has an `AmountConvertor` to handle gateway-specific formatting.

### Update pattern (enum-based partial updates)

```rust
pub enum PaymentIntentUpdate {
    ResponseUpdate {
        status: IntentStatus,
        amount_captured: Option<MinorUnit>,
        updated_by: String,
        fingerprint_id: Option<String>,
    },
    MetadataUpdate {
        metadata: Option<Value>,
        updated_by: String,
    },
    MerchantStatusUpdate {
        status: IntentStatus,
        shipping_address_id: Option<String>,
    },
    Update(Box<PaymentIntentUpdateFields>),
    // ...
}
```

**Takeaway for us**: The update-enum pattern is useful — rather than one monolithic update struct, define purpose-specific update variants that document which fields each operation can change.

## 7. Operations Pattern (Command Pattern)

The core payment flow uses a command pattern:

```rust
pub trait Operation<F, T> {
    type Data;
    fn to_validate_request(&self) -> &dyn ValidateRequest<F, T, Self::Data>;
    fn to_get_tracker(&self) -> &dyn GetTracker<F, Self::Data, T>;
    fn to_domain(&self) -> &dyn Domain<F, T, Self::Data>;
    fn to_update_tracker(&self) -> &dyn UpdateTracker<F, Self::Data, T>;
    fn to_post_update_tracker(&self) -> &dyn PostUpdateTracker<F, Self::Data, T>;
}
```

Each payment operation (Create, Confirm, Capture, Cancel, Status) implements this trait:

```rust
pub struct PaymentCreate;
pub struct PaymentConfirm;
pub struct PaymentCapture;
pub struct PaymentCancel;
pub struct PaymentStatus;
```

The orchestrator function `payments_operation_core` is generic over the operation:

```rust
pub async fn payments_operation_core<F, Req, Op, FData, D>(
    state: &SessionState,
    operation: Op,
    req: Req,
    ...,
) -> RouterResult<(D, ...)>
where
    Op: Operation<F, Req, Data = D>,
    D: ConstructFlowSpecificData<F, FData, PaymentsResponseData>,
    RouterData<F, FData, PaymentsResponseData>: Feature<F, FData>,
{
    // 1. Validate request
    // 2. Get trackers (load/create payment data)
    // 3. Create/fetch customer
    // 4. Run decision manager
    // 5. Perform routing (select connector)
    // 6. Call connector
    // 7. Update trackers
}
```

**Takeaway for us**: This is over-engineered for our current stage. For our payment service, a simpler approach:
- Direct service functions per operation (`create_payment`, `capture_payment`, `void_payment`)
- Status validation at the top of each function
- Shared helper for the connector call + response handling

## Summary of Patterns to Adopt

| Pattern | Hyperswitch Approach | Our Adaptation |
|---------|---------------------|----------------|
| Status model | Two-level (Intent + Attempt) with exhaustive match | Two-level (Payment + Transaction) with exhaustive match |
| Status guards | `is_terminal_status()`, `ValidateStatusForOperation` trait | Same pattern — trait per operation with exhaustive match |
| Connector abstraction | `ConnectorIntegration<Flow, Req, Resp>` trait | `PaymentGateway` trait with authorize/capture/void/sync |
| Status mapping | `From<ConnectorStatus> for AttemptStatus` per connector | Same — `From<StripeStatus> for TransactionStatus` |
| Auth abstraction | `ConnectorAuthType` enum | `GatewayCredentials` enum |
| Money | `MinorUnit(i64)` newtype | Already using `rust_decimal`; consider `MinorUnit` for gateway communication |
| Idempotency | payment_id unique constraint, `DuplicatePayment` error | Same — payment_id unique constraint, `AlreadyExists` error |
| Error handling | `ConnectorError` + `ApiErrorResponse` + `error_stack` | `PaymentError` + `AppError` + structured codes |
| Updates | Enum-based partial updates | Adopt for payment state transitions |
| Domain model | `PaymentIntent` + `PaymentAttempt` (separate entities) | `Payment` + `Transaction` (same separation) |
