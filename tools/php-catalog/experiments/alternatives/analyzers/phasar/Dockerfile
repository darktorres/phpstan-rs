FROM ubuntu:26.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      git ca-certificates build-essential cmake ninja-build zlib1g-dev libzstd-dev \
      llvm-21-dev clang-21 libclang-21-dev python3 time && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch development https://github.com/secure-software-engineering/phasar.git /phasar \
 && cd /phasar && git submodule update --init --depth 1
RUN cd /phasar && mkdir build && cd build \
 && CC=clang-21 CXX=clang++-21 cmake -G Ninja -DCMAKE_BUILD_TYPE=Release -DPHASAR_LLVM_VERSION=21.1 \
      -DPHASAR_BUILD_UNITTESTS=OFF -DPHASAR_ENABLE_INTEGRATIONTESTS=OFF -DPHASAR_BUILD_IR=OFF .. \
 && ninja -j8 phasar-cli
