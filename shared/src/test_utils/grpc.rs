use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

pub async fn start_test_grpc_server(router: tonic::transport::server::Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    tokio::spawn(async move {
        router
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    url
}
