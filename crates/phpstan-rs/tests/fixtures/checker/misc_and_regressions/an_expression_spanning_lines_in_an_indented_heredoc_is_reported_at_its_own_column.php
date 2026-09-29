<?php
function f(): string
{
    return <<<EOT
        a {${
            $undefinedOne
        }} b {${strtolower(
            $undefinedTwo
        )}}
        EOT;
}
