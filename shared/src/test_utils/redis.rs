use testcontainers_modules::redis::{REDIS_PORT, Redis};
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio::sync::OnceCell;

/// Shared Redis container, initialized once per test binary.
struct SharedRedisContainer {
    _container: ContainerAsync<Redis>,
    url: String,
}

static SHARED_REDIS: OnceCell<SharedRedisContainer> = OnceCell::const_new();

impl SharedRedisContainer {
    async fn init() -> Self {
        let container = Redis::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(REDIS_PORT).await.unwrap();
        let url = format!("redis://{host}:{port}");
        Self {
            _container: container,
            url,
        }
    }
}

pub struct TestRedis {
    pub conn: redis::aio::ConnectionManager,
}

impl TestRedis {
    /// Returns a connection to a shared Redis container, flushed for a clean state.
    ///
    /// The first call starts the container. Subsequent calls reuse it
    /// and flush all data to ensure test isolation.
    pub async fn start() -> Self {
        let shared = SHARED_REDIS
            .get_or_init(|| SharedRedisContainer::init())
            .await;
        let client = redis::Client::open(shared.url.as_str()).unwrap();
        let mut conn = redis::aio::ConnectionManager::new(client).await.unwrap();
        // Flush to ensure clean state for each test
        let _: String = redis::cmd("FLUSHDB")
            .query_async(&mut conn)
            .await
            .unwrap();
        Self { conn }
    }
}
