# The PHP runtime the runtime oracle runs fixtures under in CI: the official
# PHP 8.5.4 CLI image, pinned by digest, plus the extensions fixtures use that
# the stock image does not compile in. Built locally by CI as
# `phpstan-rs-oracle:8.5.4`; PHPSTAN_RS_ORACLE_PHP is the version it reports.
FROM php@sha256:cfa277c10f9f1859addbbed6739673a50a9c1b5777f64c0441118eca148b8056

RUN docker-php-ext-install bcmath
