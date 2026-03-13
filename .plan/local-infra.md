# Local Infrastructure (docker-compose.infra.yml)

| Service       | Image                      | Host Port                                      | Purpose                |
| ------------- | -------------------------- | ---------------------------------------------- | ---------------------- |
| Postgres      | postgres:18                | 5432                                           | Primary data store     |
| Redis         | redis:8.6                  | 6379                                           | Cache / session / cart |
| Kafka (KRaft) | apache/kafka-native:3.9    | 29092                                          | Event bus              |
| Kafka UI      | provectuslabs/kafka-ui:0.7 | 8090                                           | Kafka admin UI         |
| Jaeger        | jaegertracing/jaeger:2.4   | 16686 (UI), 4317 (OTLP gRPC), 4318 (OTLP HTTP) | Distributed tracing    |

Start: `make local-infra`
Stop: `make local-infra-down`
