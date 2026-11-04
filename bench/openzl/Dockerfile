FROM debian@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
ARG OPENZL_COMMIT=32246b48faee46807f84183dac4db479089f5445
ARG JOBS=12
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git build-essential cmake file coreutils util-linux procps \
 && rm -rf /var/lib/apt/lists/*
RUN git clone https://github.com/facebook/openzl /opt/openzl \
 && cd /opt/openzl \
 && git checkout "$OPENZL_COMMIT" \
 && test "$(git rev-parse HEAD)" = "$OPENZL_COMMIT" \
 && git submodule update --init --recursive deps/zstd deps/lz4 deps/xgboost \
 && git submodule status
WORKDIR /opt/openzl
RUN make -j"$JOBS" lib zli \
 && ls -l libopenzl.a libopenzl.so zli \
 && ./zli --version || true
RUN mkdir -p /opt/openzl-dist/bin /opt/openzl-dist/lib /opt/openzl-dist/include \
 && cp zli /opt/openzl-dist/bin/ \
 && cp libopenzl.a libopenzl.so /opt/openzl-dist/lib/ \
 && cp -r include/openzl /opt/openzl-dist/include/ \
 && ln -s /opt/openzl-dist/bin/zli /usr/local/bin/zli
ENV LD_LIBRARY_PATH=/opt/openzl-dist/lib
CMD ["sleep", "infinity"]
