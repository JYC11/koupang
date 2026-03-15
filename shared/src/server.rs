use crate::cache::init_optional_redis;
use crate::config::db_config::DbConfig;
use crate::db::{PgPool, init_db};
use crate::health::health_routes;
use crate::observability::init_tracing;
use axum::Router;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// InfraDep — infrastructure requirements as data (DOP ch.8 pattern)
// ---------------------------------------------------------------------------

/// Declares what infrastructure a service needs. Rules as data: each variant
/// is a requirement, and `init_infra` is the interpreter that materializes them.
#[derive(Debug, Clone)]
pub enum InfraDep {
    Postgres {
        url_env: &'static str,
        migrations_dir: &'static str,
    },
    Redis,
}

impl fmt::Display for InfraDep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postgres { url_env, .. } => write!(f, "Postgres({})", url_env),
            Self::Redis => write!(f, "Redis"),
        }
    }
}

/// Interpreter: describe all declared dependencies as a human-readable string.
fn describe_deps(deps: &[InfraDep]) -> String {
    if deps.is_empty() {
        return "none".to_string();
    }
    deps.iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Infra — initialized resources passed to service build closures
// ---------------------------------------------------------------------------

/// Infrastructure resources initialized by [`ServiceBuilder`].
///
/// Fields are `Option` — only populated for dependencies declared via [`InfraDep`].
/// Use `require_db()` / `require_redis()` for non-optional access with clear panics.
#[derive(Clone)]
pub struct Infra {
    pub db: Option<PgPool>,
    pub redis: Option<redis::aio::ConnectionManager>,
}

impl Infra {
    /// Returns the Postgres pool. Panics if the service did not declare a Postgres dependency.
    pub fn require_db(&self) -> &PgPool {
        self.db.as_ref().expect(
            "BUG: service requires Postgres but ServiceBuilder was not configured with .with_db()",
        )
    }

    /// Returns the Redis connection. Panics if Redis was not initialized.
    pub fn require_redis(&self) -> &redis::aio::ConnectionManager {
        self.redis.as_ref().expect(
            "BUG: service requires Redis but ServiceBuilder was not configured with .with_redis()",
        )
    }
}

// ---------------------------------------------------------------------------
// GrpcConfig
// ---------------------------------------------------------------------------

pub struct GrpcConfig {
    pub port_env_key: &'static str,
    pub default_port: u16,
}

// ---------------------------------------------------------------------------
// ServiceBuilder — composable bootstrap with data-oriented deps
// ---------------------------------------------------------------------------

/// Composable builder for service bootstrap.
///
/// Infrastructure requirements are declared as data via [`InfraDep`] variants.
/// At startup, `init_infra` interprets them — only initializing what was declared.
///
/// # Examples
/// ```ignore
/// // Service with Postgres + Redis
/// ServiceBuilder::new("catalog")
///     .http_port_env("CATALOG_PORT")
///     .with_db("CATALOG_DB_URL")
///     .with_redis()
///     .run(|infra| {
///         let db = infra.require_db().clone();
///         app(AppState::new(db, infra.redis.clone()))
///     })
///     .await
///
/// // Redis-only service (no Postgres)
/// ServiceBuilder::new("cart")
///     .http_port_env("CART_PORT")
///     .with_redis()
///     .run(|infra| {
///         app(AppState::new(infra.require_redis().clone()))
///     })
///     .await
/// ```
pub struct ServiceBuilder {
    name: &'static str,
    http_port_env: &'static str,
    deps: Vec<InfraDep>,
}

impl ServiceBuilder {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            http_port_env: "PORT",
            deps: Vec::new(),
        }
    }

    pub fn http_port_env(mut self, key: &'static str) -> Self {
        self.http_port_env = key;
        self
    }

    /// Declare a Postgres dependency with default migrations dir.
    pub fn with_db(mut self, url_env: &'static str) -> Self {
        self.deps.push(InfraDep::Postgres {
            url_env,
            migrations_dir: "./migrations",
        });
        self
    }

    /// Declare a Postgres dependency with custom migrations dir.
    pub fn with_db_migrations(mut self, url_env: &'static str, dir: &'static str) -> Self {
        self.deps.push(InfraDep::Postgres {
            url_env,
            migrations_dir: dir,
        });
        self
    }

    /// Declare a Redis dependency.
    pub fn with_redis(mut self) -> Self {
        self.deps.push(InfraDep::Redis);
        self
    }

    /// Run HTTP server only (no gRPC sidecar).
    pub async fn run<F>(self, build_app: F) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce(&Infra) -> Router,
    {
        let port = self.parse_http_port();
        let infra = self.init_infra().await?;
        let app = build_app(&infra).merge(health_routes(self.name));

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
        tracing::info!("{} listening on port {port}", self.name);
        axum::serve(listener, app).await?;
        Ok(())
    }

    /// Run HTTP + gRPC servers concurrently.
    pub async fn run_with_grpc<F, G, Fut>(
        self,
        grpc_config: GrpcConfig,
        build_app: F,
        build_grpc: G,
    ) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce(&Infra) -> Router,
        G: FnOnce(Infra, SocketAddr) -> Fut,
        Fut: Future<Output = Result<(), tonic::transport::Error>> + Send,
    {
        let port = self.parse_http_port();
        let grpc_port: u16 = std::env::var(grpc_config.port_env_key)
            .unwrap_or_else(|_| grpc_config.default_port.to_string())
            .parse()
            .expect("gRPC port must be a valid u16");
        assert!(grpc_port > 0, "gRPC port must be positive");
        let grpc_addr: SocketAddr = format!("0.0.0.0:{grpc_port}").parse()?;

        let infra = self.init_infra().await?;
        let app = build_app(&infra).merge(health_routes(self.name));

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
        tracing::info!("{} HTTP listening on port {port}", self.name);
        tracing::info!("{} gRPC listening on port {grpc_port}", self.name);

        tokio::select! {
            result = axum::serve(listener, app) => { result?; }
            result = build_grpc(infra, grpc_addr) => { result?; }
        }
        Ok(())
    }

    /// Interpreter: walk the deps list and initialize each declared resource.
    async fn init_infra(&self) -> Result<Infra, Box<dyn Error>> {
        init_tracing(self.name);
        tracing::info!("{} infra deps: [{}]", self.name, describe_deps(&self.deps));

        let mut db = None;
        let mut redis = None;

        for dep in &self.deps {
            match dep {
                InfraDep::Postgres {
                    url_env,
                    migrations_dir,
                } => {
                    let db_config = DbConfig::new(url_env);
                    db = Some(init_db(db_config, migrations_dir).await?);
                }
                InfraDep::Redis => {
                    redis = init_optional_redis().await;
                }
            }
        }

        Ok(Infra { db, redis })
    }

    fn parse_http_port(&self) -> u16 {
        let port: u16 = std::env::var(self.http_port_env)
            .unwrap_or_else(|_| "3000".to_string())
            .parse()
            .expect("HTTP port must be a valid u16");
        assert!(port > 0, "HTTP port must be positive");
        port
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_defaults() {
        let builder = ServiceBuilder::new("test-svc");
        assert_eq!(builder.name, "test-svc");
        assert_eq!(builder.http_port_env, "PORT");
        assert!(builder.deps.is_empty());
    }

    #[test]
    fn with_db_adds_postgres_dep() {
        let builder = ServiceBuilder::new("svc").with_db("MY_DB_URL");
        assert_eq!(builder.deps.len(), 1);
        assert!(matches!(
            builder.deps[0],
            InfraDep::Postgres {
                url_env: "MY_DB_URL",
                ..
            }
        ));
    }

    #[test]
    fn with_redis_adds_redis_dep() {
        let builder = ServiceBuilder::new("svc").with_redis();
        assert_eq!(builder.deps.len(), 1);
        assert!(matches!(builder.deps[0], InfraDep::Redis));
    }

    #[test]
    fn multiple_deps_accumulate() {
        let builder = ServiceBuilder::new("svc").with_db("DB_URL").with_redis();
        assert_eq!(builder.deps.len(), 2);
    }

    #[test]
    fn redis_only_has_no_postgres_dep() {
        let builder = ServiceBuilder::new("cart")
            .http_port_env("CART_PORT")
            .with_redis();
        assert_eq!(builder.deps.len(), 1);
        assert!(matches!(builder.deps[0], InfraDep::Redis));
    }

    #[test]
    fn describe_deps_empty() {
        assert_eq!(describe_deps(&[]), "none");
    }

    #[test]
    fn describe_deps_formats_correctly() {
        let deps = vec![
            InfraDep::Postgres {
                url_env: "DB_URL",
                migrations_dir: "./migrations",
            },
            InfraDep::Redis,
        ];
        assert_eq!(describe_deps(&deps), "Postgres(DB_URL), Redis");
    }

    #[test]
    #[should_panic(expected = "BUG: service requires Postgres")]
    fn infra_require_db_panics_when_none() {
        let infra = Infra {
            db: None,
            redis: None,
        };
        infra.require_db();
    }

    #[test]
    #[should_panic(expected = "BUG: service requires Redis")]
    fn infra_require_redis_panics_when_none() {
        let infra = Infra {
            db: None,
            redis: None,
        };
        infra.require_redis();
    }

    #[test]
    fn parse_http_port_uses_env_var() {
        let key = "TEST_SB_PORT_1";
        // SAFETY: test-only, single-threaded unit tests
        unsafe { std::env::set_var(key, "8080") };
        let builder = ServiceBuilder::new("svc").http_port_env(key);
        assert_eq!(builder.parse_http_port(), 8080);
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn parse_http_port_defaults_to_3000() {
        let key = "TEST_SB_PORT_UNSET";
        // SAFETY: test-only, single-threaded unit tests
        unsafe { std::env::remove_var(key) };
        let builder = ServiceBuilder::new("svc").http_port_env(key);
        assert_eq!(builder.parse_http_port(), 3000);
    }

    #[test]
    #[should_panic(expected = "HTTP port must be a valid u16")]
    fn parse_http_port_panics_on_invalid_value() {
        let key = "TEST_SB_PORT_BAD";
        // SAFETY: test-only, single-threaded unit tests
        unsafe { std::env::set_var(key, "not-a-number") };
        let builder = ServiceBuilder::new("svc").http_port_env(key);
        builder.parse_http_port();
    }
}
