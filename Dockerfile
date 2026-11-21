# syntax=docker/dockerfile:1.7

# Compila só as dependências primeiro: camada em cache enquanto o Cargo.lock não mudar.
FROM rust:1-slim-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
 && cargo build --release --locked 2>/dev/null || true
COPY src src
COPY migrations migrations
# touch força recompilar o crate (o build fictício acima deixou artefatos mais novos)
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# Runtime mínimo: sem shell, sem gerenciador de pacotes, usuário não-root.
# TLS via rustls com raízes embutidas, então não precisa de ca-certificates.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /app/target/release/familia-cashflow /usr/local/bin/familia-cashflow
ENV BIND_ADDR=0.0.0.0:8080 \
    LOG_FORMAT=json
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/familia-cashflow"]
