use crate::errors::AppError;

/// Represents an email to be sent
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub body_html: String,
}

/// Trait for email sending — implement for real providers later
#[async_trait::async_trait]
pub trait EmailService: Send + Sync {
    async fn send_email(&self, message: EmailMessage) -> Result<(), AppError>;
}

/// Mock implementation that logs instead of sending
pub struct MockEmailService;

impl MockEmailService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl EmailService for MockEmailService {
    async fn send_email(&self, message: EmailMessage) -> Result<(), AppError> {
        tracing::info!(
            to = %message.to,
            subject = %message.subject,
            "[MOCK EMAIL] Would send email"
        );
        tracing::debug!(body = %message.body_html, "[MOCK EMAIL] Body");
        Ok(())
    }
}
