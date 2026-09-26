<?php
/** @return list<int>&non-empty-array */
function f(): array
{
    return [1];
}

/** @return non-empty-array&list<string> */
function g(): array
{
    return ['a'];
}
