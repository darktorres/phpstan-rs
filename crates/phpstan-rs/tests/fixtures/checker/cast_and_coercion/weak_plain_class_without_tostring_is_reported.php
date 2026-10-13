<?php

class WeakPlain {}

function weakPlainToStrlen(WeakPlain $value): int
{
    return strlen($value);
}
