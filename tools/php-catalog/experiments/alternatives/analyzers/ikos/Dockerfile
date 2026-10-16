FROM ubuntu:22.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      git ca-certificates build-essential cmake libgmp-dev libmpfr-dev libboost-dev libboost-filesystem-dev \
      libboost-thread-dev libboost-test-dev libsqlite3-dev libtbb-dev python3 python3-pygments \
      llvm-14-dev clang-14 libz-dev libedit-dev time && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch v3.5 https://github.com/NASA-SW-VnV/ikos.git /ikos
RUN mkdir /ikos/build && cd /ikos/build \
 && cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/opt/ikos -DLLVM_CONFIG_EXECUTABLE=/usr/bin/llvm-config-14 .. \
 && make -j8 && make install
ENV PATH=/opt/ikos/bin:$PATH
