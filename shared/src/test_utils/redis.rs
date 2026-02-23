use testcontainers_modules::redis::{REDIS_PORT, Redis};
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

pub struct TestRedis {
    pub conn: redis::aio::ConnectionManager,
    _container: ContainerAsync<Redis>,
}

impl TestRedis {
    pub async fn start() -> Self {
        let container = Redis::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(REDIS_PORT).await.unwrap();
        let url = format!("redis://{host}:{port}");
        let client = redis::Client::open(url.as_str()).unwrap();
        let conn = redis::aio::ConnectionManager::new(client).await.unwrap();
        Self {
            conn,
            _container: container,
        }
    }
}
