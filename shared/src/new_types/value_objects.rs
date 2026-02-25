#[macro_export]
macro_rules! validated_name {
    ($name:ident, $display:expr, $max_len:expr) => {
        #[derive(Debug, Clone)]
        pub struct $name(String);

        impl $name {
            pub fn new(input: &str) -> Result<Self, shared::errors::AppError> {
                let trimmed = input.trim();

                if trimmed.is_empty() {
                    return Err(shared::errors::AppError::BadRequest(format!(
                        "{} must not be empty",
                        $display
                    )));
                }

                if trimmed.len() > $max_len {
                    return Err(shared::errors::AppError::BadRequest(format!(
                        "{} must not exceed {} characters",
                        $display, $max_len
                    )));
                }

                Ok(Self(trimmed.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

#[macro_export]
macro_rules! valid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(uuid::Uuid);

        impl $name {
            pub fn new(id: uuid::Uuid) -> Self {
                Self(id)
            }

            pub fn value(&self) -> uuid::Uuid {
                self.0
            }

            pub fn into_inner(self) -> uuid::Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}
