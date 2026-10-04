<?php
function f(?DateTime $d): ?string
{
    return $d?->format("Y");
}
