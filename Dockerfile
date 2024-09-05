FROM rust:1.74.0 as builder

ARG PACKAGE

WORKDIR /app

RUN apt-get update \
    && apt-get install -y cmake

RUN echo '[workspace]\n\
resolver = "2"\n\
\n\
members = [\n\
    "dummy",\n\
    "event-hash",\n\
]\n\
' > Cargo.toml

COPY ./Cargo.lock .
ADD ./event-hash ./event-hash
RUN cargo new dummy
RUN touch dummy/src/generate_token.rs && echo 'fn main() {}' > "dummy/src/generate_token.rs"
COPY ./${PACKAGE}/Cargo.toml ./dummy/Cargo.toml
RUN cargo install --path "./dummy"

# Compile source code
COPY ./Cargo.toml .
ADD ./.sqlx ./.sqlx
ADD ./event-hash ./event-hash
ADD ./experiment-producer ./experiment-producer
ADD ./http-load-generator ./http-load-generator
ADD ./notifications-service ./notifications-service
ADD ./notifier ./notifier
ADD ./test-to-api ./test-to-api

ADD ./config/build ./config/build
ADD ./scripts ./scripts
RUN ./scripts/setup_env.sh build

RUN cargo install --path "./${PACKAGE}"


# Prod stage
FROM debian:bookworm-slim
ARG PACKAGE

WORKDIR /app

COPY --from=builder /usr/local/cargo/bin/${PACKAGE} /usr/local/bin/${PACKAGE}
COPY --from=builder /app/scripts /app/scripts/
COPY --from=builder /app/${PACKAGE} /app/${PACKAGE}

RUN apt-get update \
    && apt-get -y install libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY --chmod=500 entrypoint.sh .
ENV PACKAGE="${PACKAGE}"
ENTRYPOINT ["./entrypoint.sh"]
