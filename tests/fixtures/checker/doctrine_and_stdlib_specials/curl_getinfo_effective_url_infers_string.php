<?php
            function f($ch): string {
                return curl_getinfo($ch, CURLINFO_EFFECTIVE_URL);
            }
        