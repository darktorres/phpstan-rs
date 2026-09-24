<?php
            function f($ch): string|false {
                return curl_getinfo($ch, CURLINFO_REDIRECT_URL);
            }
        