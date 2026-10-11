# A version's runtime image, `phpstan-rs/php:<version>`: the official image
# with the build the catalog reads installed over the official one, and the
# libraries that build loads. Its context holds apt-sources.sh and an
# export's runtime-packages.txt and dist.tar.
ARG VERSION
FROM php:${VERSION}-cli
COPY apt-sources.sh runtime-packages.txt dist.tar /opt/php-images/
RUN sh /opt/php-images/apt-sources.sh \
    && DEBIAN_FRONTEND=noninteractive xargs -a /opt/php-images/runtime-packages.txt \
        apt-get install -y -qq --no-install-recommends > /dev/null \
    && rm -rf "$(php-config --extension-dir)" \
    && rm -f /usr/local/etc/php/conf.d/docker-php-ext-*.ini \
    && tar -C / -xf /opt/php-images/dist.tar \
    && rm -rf /opt/php-images /var/lib/apt/lists/* \
    && php -v > /dev/null
