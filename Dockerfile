# === Stage 1: build ===
FROM rust:1.75 as builder

WORKDIR /app

# Скопируем манифесты и зависимости отдельно, чтобы кэшировать build deps
COPY Cargo.toml Cargo.lock ./
# Скопируем исходники
COPY src ./src

# Собираем только бинарь ноды
RUN cargo build --bin gvr_hybrid_node --release

# === Stage 2: runtime ===
FROM debian:12-slim

# Установим минимальные зависимости (если нужны)
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем скомпилированный бинарник
COPY --from=builder /app/target/release/gvr_hybrid_node /usr/local/bin/gvr_hybrid_node

# Порт P2P и RPC
EXPOSE 4000/tcp
EXPOSE 8080/tcp

# Папка для данных (state.json, ключи, и т.п.)
VOLUME ["/app/data"]

# По умолчанию запускаем ноду с P2P и RPC на стандартных портах.
# Используем /app/data как рабочую директорию, чтобы state.json сохранялся там.
WORKDIR /app/data

ENTRYPOINT ["/usr/local/bin/gvr_hybrid_node"]
# Пример: можно переопределять адреса и coinbase через args контейнера.
# docker run ... gvr_hybrid_node --p2p-addr 0.0.0.0:4000 --rpc-addr 0.0.0.0:8080 --coinbase-addr alice