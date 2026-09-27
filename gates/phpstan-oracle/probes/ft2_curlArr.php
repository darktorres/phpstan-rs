<?php
function ca(CurlHandle $h): void { curl_setopt_array($h, [CURLOPT_RETURNTRANSFER => "yes"]); }
