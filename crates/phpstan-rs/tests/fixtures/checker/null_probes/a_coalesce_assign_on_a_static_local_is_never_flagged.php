<?php
            function curl_version(): array { return []; }
            function create(): void {
                static $curlVersion = null;
                $curlVersion ??= curl_version();
            }
        