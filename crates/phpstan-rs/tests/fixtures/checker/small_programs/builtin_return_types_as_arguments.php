<?php

function needsBool(bool $b): bool
{
    return $b;
}

function needsString(string $s): string
{
    return $s;
}

echo needsBool(ctype_alpha("abc"));
echo needsBool(ctype_digit("123"));

$encoded = json_encode(["a" => 1]);
if ($encoded !== false) {
    echo needsString($encoded);
}

echo json_decode('{"a":1}', true)['a'] ?? 0;
